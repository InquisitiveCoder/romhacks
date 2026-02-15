use crate::crc::*;
use crate::error::prelude::*;
use crate::fs::HasPath;
use crate::patch::find_patch_kind;
use crate::{filename, hack, manifest, patch};
use fs_err as fs;
use read_write_utils::DEFAULT_BUF_SIZE;
use std::io;
use std::io::prelude::*;
use std::io::{BufReader, BufWriter};
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
}

impl Args {
  pub fn call(self) -> Result<(), Error> {
    let rom = fs::File::open(&self.rom)?;
    let mut patch = fs::File::open(&self.patch)?;
    let temp_file_name = {
      let mut file_name = Ulid::new().to_string();
      file_name.push_str(".tmp");
      file_name
    };
    let temp_file: fs::File = fs::OpenOptions::new()
      .create_new(true)
      .read(true)
      .write(true)
      .open(temp_file_name.as_str())?;

    let patch_kind = find_patch_kind(&mut patch)?;
    let patcher = patch::Patcher::from_patch_kind(patch_kind);

    let mut rom = BufReader::new(rom);
    let mut patch = BufReader::new(patch);
    let mut temp_file = BufWriter::with_capacity(DEFAULT_BUF_SIZE * 3 / 2, temp_file);
    let checksums = match patcher.patch(&mut rom, &mut patch, &mut temp_file, true) {
      Ok(checksums) => checksums,
      Err(e) => {
        drop(temp_file);
        let _ = fs::remove_file(temp_file_name.as_str());
        return Err(e)?;
      }
    };

    let game_name: ffi::OsString = ffi::OsString::from(filename::infer_game_name(&rom.path()));
    let manifest_path: ffi::OsString = {
      let mut buf = ffi::OsString::from(&game_name);
      buf.push(" (patched).romhacks.kdl");
      buf
    };
    let mut doc = manifest::get_or_create(
      &manifest_path,
      &rom.path(),
      Crc32::new(checksums.source_crc32),
      Crc32::new(checksums.patch_crc32),
    )?;

    log::info!("ROM patched successfully.");

    let patched_file_name: ffi::OsString = {
      let mut buf = ffi::OsString::from(&game_name);
      buf.push(" (patched)");
      if let Some(ext) = rom.path().extension() {
        buf.push(".");
        buf.push(ext);
      }
      buf
    };
    manifest::update(
      &mut doc,
      &self.rom,
      &self.patch,
      self.hack,
      Crc32::new(checksums.source_crc32),
      Crc32::new(checksums.patch_crc32),
      Crc32::new(checksums.target_crc32),
    );
    let manifest_string: String = doc.to_string();
    fs::write(&manifest_path, &manifest_string)?;
    println!("{manifest_string}");

    temp_file.flush()?;
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
