use crate::{filename, fs, hack, manifest, patch, paths, sha};
use miette::Diagnostic;
use patch::CommandBuilder as C;
use std::process;
use thiserror::Error;
use Error as E;

#[derive(Clone, Debug, clap::Args)]
pub struct Args {
  #[arg(short, long)]
  pub rom: paths::FilePathBuf,
  #[arg(short, long)]
  pub patch: patch::Patch,
  #[command(flatten)]
  pub hack: hack::RomHack,
  #[arg(short, long)]
  pub no_backup: bool,
}

impl Args {
  pub fn call(self) -> Result<(), Error> {
    let manifest_path = {
      let mut path = self.rom.clone();
      path.set_file_name(filename::game_name(&self.rom));
      path.push_str(".romhacks.kdl").unwrap()
    };

    let file_digests = sha::try_hash(&self.rom)?;
    let patch_digests = sha::try_hash(&self.patch.path)?;

    let mut doc =
      manifest::get_or_create(&manifest_path, &self.rom, &file_digests, &patch_digests)?;

    let backup_file = self.rom.clone().push_str(".bak").unwrap();
    let backup_created = if self.no_backup || backup_file.exists() {
      false
    } else {
      fs::copy(&self.rom, &backup_file)
        .map_err(|err| fs::Error::CopyError(err, self.rom.clone(), backup_file.clone()))?;
      log::info!(r#"Created backup file "{backup_file}""#);
      true
    };
    let tool = patch::Tool::from_patch_kind(self.patch.kind);
    let (mut command, temp_file_path) = match tool.command_builder() {
      C::PatchInPlace(builder) => (builder(&self.rom, &self.patch.path), None),
      C::PatchCopy(builder) => {
        if backup_created {
          (builder(&backup_file, &self.patch.path, &self.rom), None)
        } else {
          let mut temp_file = backup_file;
          temp_file.set_extension("tmp");
          log::info!(
            r#"{tool} can't patch files in place. Renaming "{rom}" to "{temp_file}"."#,
            tool = tool.name(),
            rom = &self.rom
          );
          fs::rename(&self.rom, &temp_file)
            .map_err(|err| fs::Error::RenameError(err, self.rom.clone(), temp_file.clone()))?;
          (
            builder(&temp_file, &self.patch.path, &self.rom),
            Some(temp_file),
          )
        }
      }
    };
    log::info!("Patching ROM with command: {command:?}");

    let output = command
      .spawn()
      .and_then(|child| child.wait_with_output())
      .map_err(|err| {
        if let Some(temp_file_path) = temp_file_path {
          if let Err(err) = fs::remove_file(&temp_file_path) {
            log::warn!("Failed to remove temporary file \"{temp_file_path}\": {err}");
          }
        }
        fs::Error::ExecError(err, tool.program())
      })?;

    if !output.status.success() {
      return Err(E::PatchingError(output.status));
    }

    log::info!("ROM patched successfully.");

    let patched_digests = sha::try_hash(&&self.rom)?;
    manifest::update(
      &mut doc,
      self.rom,
      self.patch,
      self.hack,
      file_digests,
      patch_digests,
      patched_digests,
    );
    fs::write(&manifest_path, doc.to_string())
      .map_err(|err| fs::Error::WriteError(err, manifest_path))?;
    println!("{doc}");
    Ok(())
  }
}

#[non_exhaustive]
#[derive(Debug, Error, Diagnostic)]
pub enum Error {
  #[error(transparent)]
  #[diagnostic(transparent)]
  ManifestError(#[from] manifest::Error),
  #[error(transparent)]
  #[diagnostic(transparent)]
  IOError(#[from] fs::Error),
  #[error("Patch tool failed with exit status: {0}")]
  PatchingError(process::ExitStatus),
}

impl Error {
  pub fn get_kind(&self) -> ErrorKind {
    use ErrorKind as K;
    match &self {
      E::ManifestError(e) => match e {
        manifest::Error::IOError(_) => K::IOError,
        manifest::Error::KdlError(_) => K::BadManifest,
        manifest::Error::AlreadyPatched => K::AlreadyPatched,
        manifest::Error::ManifestOutdated => K::ManifestOutdated,
      },
      E::IOError(_) => K::IOError,
      E::PatchingError(_) => K::PatchToolError,
    }
  }
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorKind {
  IOError,
  BadManifest,
  AlreadyPatched,
  ManifestOutdated,
  PatchToolError,
}
