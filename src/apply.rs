use crate::crc::Crc32;
use crate::error::prelude::*;
use crate::io::prelude::*;
use crate::path::Utf8NativeFilePath;
use crate::{crc, filename, hack, manifest, mem, patch, path};
use fs_err as fs;
use std::io;
use typed_path::Utf8NativePathBuf;

#[derive(Clone, Debug, clap::Args)]
pub struct Args {
  #[arg(short, long)]
  pub rom: path::Utf8NativeFilePath,
  #[arg(short, long)]
  pub patch: path::Utf8NativeFilePath,
  #[command(flatten)]
  pub hack: hack::RomHack,
  #[arg(short, long)]
  pub no_backup: bool,
}

impl Args {
  pub fn call(self) -> Result<(), Error> {
    let manifest_path = {
      let mut buf = Utf8NativePathBuf::from(self.rom.as_str());
      buf.set_file_name(filename::game_name(&self.rom));
      let mut buf = buf.into_string();
      buf.push_str(".romhacks.kdl");
      let path = Utf8NativePathBuf::from(buf).into_boxed_path();
      Utf8NativeFilePath::try_new(path).unwrap()
    };

    let mut patch_file = fs::File::open(self.patch.as_path())?;
    let patch_eof = patch_file.seek(io::SeekFrom::End(0))?;
    assert!(patch_eof <= i64::MAX as u64);

    patch_file.seek(io::SeekFrom::Start(0))?;
    let (patch_kind, checksum_limit) = match &patch_file.read_array::<3>()?[..] {
      b"PAT" => (patch::Kind::IPS, patch_eof),
      b"UPS" => (patch::Kind::UPS, patch_eof - 4),
      b"BPS" => (patch::Kind::BPS, patch_eof - 4),
      b"PPF" => (patch::Kind::PPF, patch_eof),
      &[0xD6, 0xC3, 0xC4] => (patch::Kind::XDELTA, patch_eof),
      _ => {
        return Err(Error::IO(io::Error::new(
          io::ErrorKind::InvalidData,
          "Unknown patch format",
        )))
      }
    };

    let mut rom = fs::OpenOptions::new()
      .read(true)
      .write(true)
      .open(&self.rom)?;
    let rom_digest = Crc32::read_and_hash(&mut rom)?;
    let patch_digest = Crc32::read_and_hash(&mut (&mut patch_file).take(checksum_limit))?;

    let mut doc = manifest::get_or_create(&manifest_path, &self.rom, rom_digest, patch_digest)?;

    let backup_file: Utf8NativeFilePath =
      mem::try_map(self.rom.as_str(), |s: &mut String| s.push_str(".bak")).unwrap();
    if !self.no_backup && !backup_file.as_path().exists() {
      fs::copy(&self.rom, &backup_file)?;
      log::info!(r#"Created backup file "{}""#, backup_file.as_str());
    }

    let patcher = patch::Patcher::from_patch_kind(patch_kind);
    patcher.patch(
      &mut rom,
      &mut patch_file,
      rom_digest,
      patch_digest,
      patch_eof,
    )?;

    log::info!("ROM patched successfully.");

    rom.seek(io::SeekFrom::Start(0))?;
    let patched_digest = Crc32::read_and_hash(&mut rom)?;
    manifest::update(
      &mut doc,
      &self.rom,
      &self.patch,
      self.hack,
      rom_digest,
      patch_digest,
      patched_digest,
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
  FileTooLarge,
  BadManifest,
  AlreadyPatched,
  ManifestOutdated,
  Patching,
}
