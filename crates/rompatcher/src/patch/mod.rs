use crate::crc::{CRC32Hasher, Crc32};
use crate::error;
use crate::error::prelude::*;
use read_write_utils::hash::{HashingReader, HashingWriter, MonotonicHashingReader};
use read_write_utils::prelude::*;
use std::fmt;
use std::io;
use std::io::prelude::*;
use std::io::{Read, Seek, SeekFrom, Write};
use std::ops::Deref;

pub mod bps;
mod byuu;
pub mod ips;
pub mod ppf;
pub mod ups;
pub mod vcd;

pub use self::err::*;

fn rom_err(err: io::Error) -> Error {
  // InvalidInput will occur when a smaller file offset is encountered in a
  // patch format where input file offsets should only increase.
  // See PositionTracker::take_from_inner_until
  match err.kind() {
    io::ErrorKind::InvalidInput => Error::BadPatch,
    io::ErrorKind::UnexpectedEof => Error::InputFileTooSmall,
    _ => Error::IO(err),
  }
}

fn patch_err(err: io::Error) -> Error {
  match err.kind() {
    io::ErrorKind::InvalidInput => Error::BadPatch,
    io::ErrorKind::InvalidData => Error::BadPatch,
    io::ErrorKind::UnexpectedEof => Error::BadPatch,
    _ => Error::IO(err),
  }
}

#[derive(Clone, Debug)]
pub struct Patch<F> {
  file: F,
  kind: Kind,
}

impl<F: Read + Seek> Patch<F> {
  pub fn new(mut file: F) -> io::Result<Self> {
    let magic = file.read_array::<3>()?;
    file.seek(SeekFrom::Start(0))?;
    let kind = match &magic[..] {
      ips::MAGIC => Kind::IPS,
      ups::MAGIC => Kind::UPS,
      bps::MAGIC => Kind::BPS,
      ppf::MAGIC => Kind::PPF,
      vcd::MAGIC => Kind::VCD,
      _ => return Err(io::Error::from(io::ErrorKind::InvalidData)),
    };
    Ok(Self { file, kind })
  }
}

pub fn find_patch_kind(file: &mut (impl Read + Seek)) -> io::Result<Kind> {
  let magic = file.read_array::<3>()?;
  file.seek(SeekFrom::Start(0))?;
  let kind = match &magic[..] {
    ips::MAGIC => Kind::IPS,
    ups::MAGIC => Kind::UPS,
    bps::MAGIC => Kind::BPS,
    ppf::MAGIC => Kind::PPF,
    vcd::MAGIC => Kind::VCD,
    _ => return Err(io::Error::from(io::ErrorKind::InvalidData)),
  };
  Ok(kind)
}

impl<P> Patch<P> {
  pub fn kind(&self) -> Kind {
    self.kind
  }

  pub fn file(&self) -> &P {
    &self.file
  }
}

impl<P> Deref for Patch<P> {
  type Target = P;

  fn deref(&self) -> &Self::Target {
    &self.file
  }
}

impl<R: Read> Read for Patch<R> {
  fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
    self.file.read(buf)
  }
}

impl<R: BufRead> BufRead for Patch<R> {
  fn fill_buf(&mut self) -> io::Result<&[u8]> {
    self.file.fill_buf()
  }

  fn consume(&mut self, amt: usize) {
    self.file.consume(amt)
  }
}

impl<W: Write> Write for Patch<W> {
  fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
    self.file.write(buf)
  }

  fn flush(&mut self) -> io::Result<()> {
    self.file.flush()
  }
}

impl<S: Seek> Seek for Patch<S> {
  fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
    self.file.seek(pos)
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
    strict: bool,
  ) -> Result<Checksums, Error>
  where
    R: BufRead + Seek,
    P: BufRead + Seek,
    O: BufWrite + Seek,
    for<'a> &'a mut O::Inner: Read + Write + Seek,
  {
    match self.0 {
      Kind::IPS => Patcher::ips(rom, patch, output),
      Kind::UPS => Patcher::ups(rom, patch, output, strict),
      Kind::BPS => Patcher::bps(rom, patch, output, strict),
      Kind::PPF => Patcher::ppf(rom, patch, output, strict),
      Kind::VCD => Patcher::vcdiff(rom, patch, output),
    }
  }

  fn ips<R, P, O>(rom: &mut R, patch: &mut P, output: &mut O) -> Result<Checksums, Error>
  where
    R: BufRead + Seek,
    P: BufRead,
    O: BufWrite,
  {
    let mut rom = MonotonicHashingReader::new(rom, CRC32Hasher::new());
    let mut patch = HashingReader::new(patch, CRC32Hasher::new());
    let mut output = HashingWriter::new(output, CRC32Hasher::new());
    ips::patch(&mut rom, &mut patch, &mut output)?;
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
    O: BufWrite,
    for<'a> &'a mut O::Inner: Read + Write + Seek,
  {
    ups::patch(rom, patch, output, strict).map(|report| Checksums {
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
    O: BufWrite + Seek,
    for<'a> &'a mut O::Inner: Read + Write + Seek,
  {
    bps::patch(rom, patch, output, strict).map(|report| Checksums {
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
    P: BufRead + Seek,
    O: BufWrite,
    for<'a> &'a mut O::Inner: Read + Write + Seek,
  {
    let mut rom = MonotonicHashingReader::new(rom, CRC32Hasher::new());
    let mut patch = HashingReader::new(patch, CRC32Hasher::new());
    let mut output = HashingWriter::new(output, CRC32Hasher::new());
    ppf::patch(&mut rom, &mut patch, &mut output, strict)?;
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
    P: BufRead + Seek,
    O: BufWrite + Seek,
    for<'a> &'a mut O::Inner: Read + Write + Seek,
  {
    let mut rom = MonotonicHashingReader::new(rom, CRC32Hasher::new());
    let mut patch = HashingReader::new(patch, CRC32Hasher::new());
    let mut output = HashingWriter::new(output, CRC32Hasher::new());
    vcd::patch(&mut rom, &mut patch, &mut output)?;
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
  use crate::error::prelude::*;
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
    #[error("The patch or ROM file is too large.")]
    FileTooLarge,
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
}

pub trait HasInternalCrc32 {
  fn internal_crc32(&self) -> Crc32;
}
