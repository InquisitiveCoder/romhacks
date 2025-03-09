use crate::error::prelude::*;
use crate::io::Resize;
use crate::{crc, error, io, path};
use std::io::{ErrorKind, Read, Seek, Write};
use std::{fmt, fs};
use thiserror::__private::AsDynError;

pub mod bps;
pub mod ips;
pub mod ppf;
pub mod ups;
mod varint;

pub use self::err::*;

#[derive(Clone, Debug)]
pub struct Patch<P> {
  pub kind: Kind,
  pub file: P,
}

impl<P> Patch<P> {
  pub fn new(kind: Kind, file: P) -> Self {
    Self { kind, file }
  }
}

#[derive(Copy, Clone, Debug)]
pub enum Kind {
  IPS,
  UPS,
  BPS,
  PPF,
  XDELTA,
}

impl fmt::Display for Kind {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Kind::IPS => write!(f, "IPS"),
      Kind::UPS => write!(f, "UPS"),
      Kind::BPS => write!(f, "BPS"),
      Kind::PPF => write!(f, "PPF"),
      Kind::XDELTA => write!(f, "VCDIFF (a.k.a. xdelta)"),
    }
  }
}

#[derive(Clone, Copy, Debug)]
pub struct UnknownPatchKindError(pub(crate) ());

impl fmt::Display for UnknownPatchKindError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "Unknown patch type.")
  }
}

impl error::Error for UnknownPatchKindError {}

#[derive(Clone, Copy, Debug)]
pub struct Patcher(Kind);

impl Patcher {
  pub fn from_patch_kind(patch_kind: Kind) -> Self {
    Self(patch_kind)
  }

  pub fn patch<F, P>(
    &self,
    rom: &mut F,
    patch: &mut P,
    file_checksum: crc::Crc32,
    patch_checksum: crc::Crc32,
    patch_eof: u64,
  ) -> Result<(), Error>
  where
    F: Read + Write + Seek + Resize,
    P: Read + Seek,
  {
    match self.0 {
      Kind::IPS => Patcher::ips(rom, patch, patch_eof),
      Kind::UPS => Patcher::ups(rom, patch, file_checksum, patch_checksum),
      Kind::BPS => Patcher::bps(rom, patch, file_checksum, patch_checksum, patch_eof),
      Kind::PPF => Patcher::ppf(rom, patch),
      Kind::XDELTA => Patcher::xdelta3(rom, patch),
    }
  }

  fn ips<F, P>(rom: &mut F, patch: &mut P, patch_eof: u64) -> Result<(), Error>
  where
    F: Read + Write + Seek + Resize,
    P: Read + Seek,
  {
    ips::patch(rom, patch, patch_eof)?;
    Ok(())
  }

  fn ups<F, P>(
    file: &mut F,
    patch: &mut P,
    file_checksum: crc::Crc32,
    patch_checksum: crc::Crc32,
  ) -> Result<(), crate::patch::Error>
  where
    F: Read + Write + Seek + Resize,
    P: Read + Seek,
  {
    ups::patch(file, patch, file_checksum, patch_checksum)?;
    Ok(())
  }

  fn bps<F, P>(
    file: &mut F,
    patch: &mut P,
    file_checksum: crc::Crc32,
    patch_checksum: crc::Crc32,
    patch_eof: u64,
  ) -> Result<(), crate::patch::err::Error>
  where
    F: Read + Write + Seek + Resize,
    P: Read + Seek,
  {
    // bps::patch(rom, patch, file_checksum, patch_checksum, patch_eof)?
    let mut file_contents = vec![];
    file.seek(io::SeekFrom::Start(0))?;
    io::copy(file, &mut file_contents)?;
    patch.seek(io::SeekFrom::Start(0))?;
    let mut patch_contents = vec![];
    io::copy(patch, &mut patch_contents)?;
    let output = ::flips::BpsPatch::new(patch_contents)
      .apply(&file_contents)
      .map_err(|err| {
        use crate::patch::err::Error as P;
        use ::flips::Error as F;
        match err {
          F::NotThis => P::WrongInputFile,
          F::ToOutput => P::AlreadyPatched,
          F::Invalid => P::BadPatch,
          F::Scrambled => P::BadPatch,
          F::Identical => unreachable!(),
          F::TooBig => P::FileTooLarge,
          F::OutOfMem => P::IO(io::Error::from(ErrorKind::OutOfMemory)),
          F::Canceled => P::IO(io::Error::from(ErrorKind::Interrupted)),
        }
      })?;
    file.seek(io::SeekFrom::Start(0))?;
    io::copy(&mut output.as_bytes(), file)?;
    Ok(())
  }

  fn ppf<F, P>(rom: &mut F, ppf: &mut P) -> Result<(), Error>
  where
    F: Read + Write + Seek + Resize,
    P: Read + Seek,
  {
    ppf::patch(rom, ppf).map_err(|err| err.into())
  }

  fn xdelta3<F, P>(file: &mut F, patch: &mut P) -> Result<(), Error>
  where
    F: Read + Write,
    P: Read + Seek,
  {
    let mut rom: Vec<u8> = vec![];
    io::copy(file, &mut rom)?;
    let mut patch_contents: Vec<u8> = vec![];
    io::copy(patch, &mut patch_contents)?;
    let patched: Vec<u8> = xdelta3::decode(&patch_contents, &rom).ok_or(io::Error::new(
      io::ErrorKind::InvalidData,
      "xdelta3 patching failed",
    ))?;
    Ok(file.write_all(&patched)?)
  }
}

pub struct Args<'f, 'p, F, P> {
  pub file: &'f mut F,
  pub patch: &'p mut P,
  pub file_checksum: crc::Crc32,
  pub patch_checksum: crc::Crc32,
}

type PatchFn<P> = fn(&path::Path, &Patch<P>) -> Result<(), Error>;

mod err {
  use crate::error::prelude::*;
  use std::io;

  #[derive(Debug, Error)]
  #[error(transparent)]
  pub enum Error {
    #[error(transparent)]
    IO(io::Error),
    #[error("The patch file is corrupt.")]
    BadPatch,
    #[error("The patch or ROM file is too large.")]
    FileTooLarge,
    #[error("The patch is not intended for the input file.")]
    WrongInputFile,
    #[error("This patch has already been applied to the input file.")]
    AlreadyPatched,
  }

  impl From<io::Error> for Error {
    fn from(err: io::Error) -> Error {
      match err.kind() {
        io::ErrorKind::UnexpectedEof => Error::BadPatch,
        _ => Error::IO(err),
      }
    }
  }

  impl From<flips::Error> for Error {
    fn from(value: flips::Error) -> Self {
      match value {
        flips::Error::NotThis => Error::WrongInputFile,
        flips::Error::ToOutput => Error::AlreadyPatched,
        _ => Error::BadPatch,
      }
    }
  }
}
