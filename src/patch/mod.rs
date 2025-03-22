use crate::error::prelude::*;
use crate::io::Resize;
use crate::{crc, error, io};
use std::io::{ErrorKind, Read, Seek, Write};
use std::{fmt, path};

pub mod bps;
pub mod ips;
pub mod ppf;
pub mod ups;
mod varint;
pub mod vcd;

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
  VCD,
}

impl fmt::Display for Kind {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Kind::IPS => write!(f, "IPS"),
      Kind::UPS => write!(f, "UPS"),
      Kind::BPS => write!(f, "BPS"),
      Kind::PPF => write!(f, "PPF"),
      Kind::VCD => write!(f, "Vcdiff (a.k.a. xdelta)"),
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

  pub fn patch<R, P, O>(
    &self,
    rom: &mut R,
    patch: &mut P,
    output: &mut O,
    rom_checksum: crc::Crc32,
    patch_checksum: crc::Crc32,
    patch_eof: u64,
  ) -> Result<(), Error>
  where
    R: Read + Seek,
    P: Read + Seek,
    O: Read + Write + Seek + Resize,
  {
    match self.0 {
      Kind::IPS => Patcher::ips(output, patch),
      Kind::UPS => Patcher::ups(output, patch, rom_checksum, patch_checksum),
      Kind::BPS => Patcher::bps(rom, patch, output, rom_checksum, patch_checksum, patch_eof),
      Kind::PPF => Patcher::ppf(output, patch),
      Kind::VCD => Patcher::vcdiff(rom, patch, output),
    }
  }

  fn ips<R, P>(rom: &mut R, patch: &mut P) -> Result<(), Error>
  where
    R: Write + Seek + Resize,
    P: Read + Seek,
  {
    ips::patch(rom, patch)?;
    Ok(())
  }

  fn ups<R, P>(
    rom: &mut R,
    patch: &mut P,
    rom_checksum: crc::Crc32,
    patch_checksum: crc::Crc32,
  ) -> Result<(), crate::patch::Error>
  where
    R: Read + Write + Seek + Resize,
    P: Read + Seek,
  {
    ups::patch(rom, patch, rom_checksum, patch_checksum)?;
    Ok(())
  }

  fn bps<R, P, O>(
    rom: &mut R,
    patch: &mut P,
    output: &mut O,
    rom_checksum: crc::Crc32,
    patch_checksum: crc::Crc32,
    patch_eof: u64,
  ) -> Result<(), crate::patch::err::Error>
  where
    R: Read + Seek,
    P: Read + Seek,
    O: Read + Write + Seek + Resize,
  {
    // bps::patch(rom, patch, file_checksum, patch_checksum, patch_eof)?
    let mut file_contents = vec![];
    rom.seek(io::SeekFrom::Start(0))?;
    io::copy(rom, &mut file_contents)?;
    patch.seek(io::SeekFrom::Start(0))?;
    let mut patch_contents = vec![];
    io::copy(patch, &mut patch_contents)?;
    let bps_output = ::flips::BpsPatch::new(patch_contents)
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
    io::copy(&mut bps_output.as_bytes(), output)?;
    Ok(())
  }

  fn ppf<R, P>(rom: &mut R, ppf: &mut P) -> Result<(), Error>
  where
    R: Read + Write + Seek + Resize,
    P: Read + Seek,
  {
    ppf::patch(rom, ppf).map_err(|err| err.into())
  }

  fn vcdiff<R, P, O>(rom: &mut R, patch: &mut P, output: &mut O) -> Result<(), Error>
  where
    R: Read + Seek,
    P: Read + Seek,
    O: Read + Write + Seek + Resize,
  {
    vcd::patch(rom, patch, output)?;
    Ok(())
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
    #[error("Unsupported patch.")]
    UnsupportedPatchFeature,
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
        io::ErrorKind::InvalidData => Error::BadPatch,
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
