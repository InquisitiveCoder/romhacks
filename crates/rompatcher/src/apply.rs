use crate::crc::*;
use crate::error::prelude::*;
use crate::patch::{bps, ips, ppf, ups, vcd};
use crate::{filename, hack, manifest, patch};
use fs_err as fs;
use std::io;
use std::io::prelude::*;
use std::{ffi, path};
use ulid::Ulid;

#[derive(Clone, Debug, clap::Args)]
pub struct Args {
  #[arg(short, long)]
  pub rom: path::PathBuf,
  #[arg(short, long)]
  pub patch: path::PathBuf,
  #[command(flatten)]
  pub hack: hack::RomHack,
  #[arg(short, long)]
  pub no_backup: bool,
}

impl Args {
  pub fn call(self) -> Result<(), Error> {
    let mut rom = WithCrc32::new(fs::File::open(&self.rom)?)?;
    rom.seek(io::SeekFrom::Start(0))?;
    let mut patch = patch::Patch::new(fs::File::open(&self.patch)?)?;
    assert!(patch.eof() <= i64::MAX as u64);
    patch.seek(io::SeekFrom::Start(0))?;

    let game_name: ffi::OsString = ffi::OsString::from(filename::infer_game_name(&rom.path()));
    let manifest_path: ffi::OsString = {
      let mut buf = ffi::OsString::from(&game_name);
      buf.push(" (patched).romhacks.kdl");
      buf
    };
    let mut doc = manifest::get_or_create(&manifest_path, &rom, patch.crc32())?;

    let mut temp_file: fs::File = {
      let mut file_name = Ulid::new().to_string();
      file_name.push_str(".tmp");
      fs::OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(file_name)?
    };

    if !patch.is_delta_file() {
      // Some formats modify the file to be patched in place,
      // rather than build up the result from scratch.
      io::copy(&mut rom, &mut temp_file)?;
    };

    let patcher = patch::Patcher::from_patch_kind(patch.kind());
    if let Err(e) = patcher.patch(&mut rom, &mut patch, &mut temp_file) {
      let (file, path) = temp_file.into_parts();
      drop(file);
      let _ = fs::remove_file(path);
      return Err(e)?;
    }

    log::info!("ROM patched successfully.");

    temp_file.seek(io::SeekFrom::Start(0))?;
    let (patched_digest, _) = CRC32Hasher::new().hash(&mut temp_file)?;
    let patched_file_name: ffi::OsString = {
      let mut buf = ffi::OsString::from(&game_name);
      buf.push(" (patched)");
      if let Some(ext) = rom.path().extension() {
        buf.push(ext);
      }
      buf
    };
    manifest::update(
      &mut doc,
      &self.rom,
      &self.patch,
      self.hack,
      rom.crc32(),
      patch.internal_crc32(),
      patched_digest,
    );
    let manifest_string: String = doc.to_string();
    fs::write(&manifest_path, &manifest_string)?;
    println!("{manifest_string}");

    let (temp_file, temp_file_name) = temp_file.into_parts();
    drop(temp_file); // close the file prior to renaming
    fs::rename(&temp_file_name, &patched_file_name)?;

    Ok(())
  }
}

#[non_exhaustive]
#[derive(Debug, Error, Diagnostic)]
pub enum Error {
  #[error(transparent)]
  #[diagnostic(transparent)]
  Manifest(#[from] manifest::GetOrCreateError),
  #[error(transparent)]
  IO(#[from] io::Error),
  #[error(transparent)]
  Patching(#[from] patch::Error),
}

impl Error {
  pub fn get_kind(&self) -> ErrorKind {
    use ErrorKind as K;
    match &self {
      Error::Manifest(e) => match e {
        manifest::GetOrCreateError::IO(_) => K::IOError,
        manifest::GetOrCreateError::Kdl(_) => K::BadManifest,
        manifest::GetOrCreateError::AlreadyPatched => K::AlreadyPatched,
        manifest::GetOrCreateError::ManifestOutdated => K::ManifestOutdated,
      },
      Error::IO(_) => K::IOError,
      Error::Patching(_) => K::Patching,
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
  Patching,
}
