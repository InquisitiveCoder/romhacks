use crate::error;
use crate::error::prelude::*;
use patch_bps as bps;
use patch_ips as ips;
use patch_ppf as ppf;
use patch_ups as ups;
use patch_vcd as vcd;
use read_write_hashers::{HashingReader, HashingWriter, MonotonicHashingReader};
use read_write_utils::prelude::*;
use read_write_utils::seek::SeekRelative;
use rompatcher_crc32_utils::CRC32Hasher;
use std::fmt;
use std::io;
use std::io::prelude::*;
use std::io::SeekFrom;

pub use self::err::*;

pub fn find_patch_kind<F>(mut file: &mut F) -> io::Result<Result<Kind, UnknownPatchKindError>>
where
  F: Read + Seek + ?Sized,
{
  let magic = file.read_n::<3>()?;
  file.seek(SeekFrom::Start(0))?;
  let kind = match &magic[..] {
    ips::MAGIC => Kind::Ips,
    ups::MAGIC => Kind::Ups,
    bps::MAGIC => Kind::Bps,
    ppf::MAGIC => Kind::Ppf,
    vcd::MAGIC => Kind::Vcd,
    _ => return Ok(Err(UnknownPatchKindError(()))),
  };
  Ok(Ok(kind))
}

#[derive(Copy, Clone, Debug)]
pub enum Kind {
  Ips,
  Ups,
  Bps,
  Ppf,
  Vcd,
}

impl fmt::Display for Kind {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Kind::Ips => write!(f, "IPS"),
      Kind::Ups => write!(f, "UPS"),
      Kind::Bps => write!(f, "BPS"),
      Kind::Ppf => write!(f, "PPF"),
      Kind::Vcd => write!(f, "Vcdiff (a.k.a. xdelta)"),
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
    strict: bool,
  ) -> Result<Checksums, Error>
  where
    R: AmortizedRead + BufRead + SeekRelative + Seek,
    P: BufRead + Seek + Peek,
    O: BufWrite + AsRead + Seek,
  {
    match self.0 {
      Kind::Ips => Patcher::ips(rom, patch, output),
      Kind::Ups => Patcher::ups(rom, patch, output, strict),
      Kind::Bps => Patcher::bps(rom, patch, output, strict),
      Kind::Ppf => Patcher::ppf(rom, patch, output, strict),
      Kind::Vcd => Patcher::vcdiff(rom, patch, output),
    }
  }

  fn ips<R, P, O>(rom: &mut R, patch: &mut P, output: &mut O) -> Result<Checksums, Error>
  where
    R: AmortizedRead + SeekRelative,
    P: BufRead,
    O: BufWrite,
  {
    let mut rom = MonotonicHashingReader::from_start(rom, CRC32Hasher::new());
    let mut patch = HashingReader::new(patch, CRC32Hasher::new());
    let mut output = HashingWriter::new(output, CRC32Hasher::new());
    ips::patch(&mut rom, &mut patch, &mut output)??;
    io::copy(&mut rom, &mut io::sink())?;
    Ok(Checksums {
      source_crc32: rom.hasher().finish().value(),
      patch_crc32: patch.hasher().finish().value(),
      target_crc32: output.hasher().finish().value(),
    })
  }

  fn ups<R, P, O>(
    rom: &mut R,
    patch: &mut P,
    output: &mut O,
    strict: bool,
  ) -> Result<Checksums, crate::patch::Error>
  where
    R: BufRead,
    P: BufRead + Seek,
    O: BufWrite + AsRead,
  {
    let report = ups::patch(rom, patch, output, strict)??;
    Ok(Checksums {
      source_crc32: report.actual_source_crc32.value(),
      patch_crc32: report.patch_whole_file_crc32.value(),
      target_crc32: report.actual_target_crc32.value(),
    })
  }

  fn bps<R, P, O>(
    rom: &mut R,
    patch: &mut P,
    output: &mut O,
    strict: bool,
  ) -> Result<Checksums, crate::patch::Error>
  where
    R: BufRead + Seek,
    P: BufRead + Seek,
    O: BufWrite + AsRead + Seek,
  {
    let report = bps::patch(rom, patch, output, strict)??;
    Ok(Checksums {
      source_crc32: report.actual_source_crc32.value(),
      patch_crc32: report.patch_whole_file_crc32.value(),
      target_crc32: report.actual_target_crc32.value(),
    })
  }

  fn ppf<R, P, O>(
    rom: &mut R,
    patch: &mut P,
    output: &mut O,
    strict: bool,
  ) -> Result<Checksums, Error>
  where
    R: BufRead + Seek,
    P: BufRead + Seek + Peek,
    O: BufWrite + AsRead,
  {
    let mut rom = MonotonicHashingReader::from_start(rom, CRC32Hasher::new());
    // Need to look ahead in the patch file for magic strings, so some bytes
    // will be read more than once.
    let mut patch = MonotonicHashingReader::from_start(patch, CRC32Hasher::new());
    let mut output = HashingWriter::new(output, CRC32Hasher::new());
    ppf::patch(&mut rom, &mut patch, &mut output, strict)??;
    io::copy(&mut rom, &mut io::sink())?;
    Ok(Checksums {
      source_crc32: rom.hasher().finish().value(),
      patch_crc32: patch.hasher().finish().value(),
      target_crc32: output.hasher().finish().value(),
    })
  }

  fn vcdiff<R, P, O>(rom: &mut R, patch: &mut P, output: &mut O) -> Result<Checksums, Error>
  where
    R: BufRead + Seek,
    P: BufRead + Seek + Peek,
    O: BufWrite + AsRead + Seek,
  {
    let mut rom = MonotonicHashingReader::from_start(rom, CRC32Hasher::new());
    let mut patch = HashingReader::new(patch, CRC32Hasher::new());
    let mut output = HashingWriter::new(output, CRC32Hasher::new());
    vcd::patch(&mut rom, &mut patch, &mut output)??;
    io::copy(&mut rom, &mut io::sink())?;
    Ok(Checksums {
      source_crc32: rom.hasher().finish().value(),
      patch_crc32: patch.hasher().finish().value(),
      target_crc32: output.hasher().finish().value(),
    })
  }
}

pub struct Checksums {
  pub source_crc32: u32,
  pub patch_crc32: u32,
  pub target_crc32: u32,
}

mod err {
  use super::*;
  use std::io;
  use std::io::IntoInnerError;

  #[derive(Debug, Error)]
  #[error(transparent)]
  pub enum Error {
    #[error(transparent)]
    IO(io::Error),
    #[error("The patch file is corrupt.")]
    BadPatch,
    #[error("Unsupported patch.")]
    UnsupportedPatchFeature,
    #[error("The patch is not meant for this file.")]
    WrongInputFile,
    #[error(
      "The patch is not meant for this file, and can't be applied due to the file being too small."
    )]
    InputFileTooSmall,
    #[error("This patch has already been applied to the input file.")]
    AlreadyPatched,
  }

  impl<W> From<IntoInnerError<W>> for Error {
    fn from(into_inner_error: IntoInnerError<W>) -> Self {
      Error::from(into_inner_error.into_error())
    }
  }

  impl From<io::Error> for Error {
    fn from(err: io::Error) -> Error {
      use io::ErrorKind::*;
      // These errors arise from violated expectations.
      match err.kind() {
        InvalidInput => Error::BadPatch,
        InvalidData => Error::BadPatch,
        UnexpectedEof => Error::BadPatch,
        WriteZero => Error::BadPatch,
        _ => Error::IO(err),
      }
    }
  }

  impl From<ips::PatchingError> for Error {
    fn from(err: ips::PatchingError) -> Self {
      match err {
        ips::PatchingError::BadPatch => Self::BadPatch,
        ips::PatchingError::InputFileTooSmall => Self::InputFileTooSmall,
      }
    }
  }

  impl From<bps::PatchingError> for Error {
    fn from(value: bps::PatchingError) -> Self {
      match value {
        bps::PatchingError::BadPatch => Self::BadPatch,
        bps::PatchingError::WrongInputFile => Self::WrongInputFile,
        bps::PatchingError::InputFileTooSmall => Self::InputFileTooSmall,
        bps::PatchingError::AlreadyPatched => Self::AlreadyPatched,
      }
    }
  }

  impl From<ups::PatchingError> for Error {
    fn from(value: ups::PatchingError) -> Self {
      match value {
        ups::PatchingError::BadPatch => Self::BadPatch,
        ups::PatchingError::InputFileTooSmall => Self::InputFileTooSmall,
        ups::PatchingError::AlreadyPatched => Self::AlreadyPatched,
        ups::PatchingError::WrongInputFile => Self::WrongInputFile,
      }
    }
  }

  impl From<ppf::PatchingError> for Error {
    fn from(value: ppf::PatchingError) -> Self {
      match value {
        ppf::PatchingError::BadPatch => Self::BadPatch,
        ppf::PatchingError::InputFileTooSmall => Self::InputFileTooSmall,
        ppf::PatchingError::WrongInputFile => Self::WrongInputFile,
      }
    }
  }

  impl From<vcd::PatchingError> for Error {
    fn from(value: vcd::PatchingError) -> Self {
      match value {
        vcd::PatchingError::BadPatch => Self::BadPatch,
        vcd::PatchingError::InputFileTooSmall => Self::InputFileTooSmall,
        vcd::PatchingError::UnsupportedPatchFeature => Self::UnsupportedPatchFeature,
      }
    }
  }
}
