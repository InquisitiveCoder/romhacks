use crate::error::prelude::*;
use crate::{crc, filename, fs, hack, manifest, patch, path};

#[derive(Clone, Debug, clap::Args)]
pub struct Args {
  #[arg(short, long)]
  pub rom: path::FilePathBuf,
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

    let rom_digest = crc::try_hash(&self.rom)?;
    let patch_digest = crc::try_hash(&self.patch.path)?;

    let mut doc = manifest::get_or_create(&manifest_path, &self.rom, rom_digest, patch_digest)?;

    let backup_file = self.rom.clone().push_str(".bak").unwrap();
    if !self.no_backup && !backup_file.exists() {
      fs::copy(&self.rom, &backup_file)?;
      log::info!(r#"Created backup file "{backup_file}""#);
    }

    let patcher = patch::Patcher::from_patch_kind(self.patch.kind);
    patcher.patch(&self.rom, &self.patch)?;

    log::info!("ROM patched successfully.");

    let patched_digests = crc::try_hash(&self.rom)?;
    manifest::update(
      &mut doc,
      self.rom,
      self.patch,
      self.hack,
      rom_digest,
      patch_digest,
      patched_digests,
    );
    fs::write(&manifest_path, doc.to_string())?;
    println!("{doc}");
    Ok(())
  }
}

#[non_exhaustive]
#[derive(Debug, Error, Diagnostic)]
pub enum Error {
  #[error(transparent)]
  #[diagnostic(transparent)]
  Manifest(#[from] manifest::Error),
  #[error(transparent)]
  #[diagnostic(transparent)]
  IO(#[from] fs::Error),
  #[error(transparent)]
  Patching(#[from] patch::Error),
}

impl Error {
  pub fn get_kind(&self) -> ErrorKind {
    use ErrorKind as K;
    match &self {
      Error::Manifest(e) => match e {
        manifest::Error::IO(_) => K::IOError,
        manifest::Error::Kdl(_) => K::BadManifest,
        manifest::Error::AlreadyPatched => K::AlreadyPatched,
        manifest::Error::ManifestOutdated => K::ManifestOutdated,
      },
      Error::IO(_) => K::IOError,
      Error::Patching(_) => K::PatchingError,
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
  PatchingError,
}
