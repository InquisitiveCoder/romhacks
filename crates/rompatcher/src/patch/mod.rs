use crate::crc::{Crc32, HasCrc32};
use crate::error::prelude::*;
use crate::{crc, error};
use read_write_utils::prelude::*;
use std::io;
use std::io::prelude::*;
use std::io::{Read, Seek, Write};
use std::ops::Deref;
use std::{fmt, num};

pub mod bps;
mod byuu;
pub mod ips;
pub mod ppf;
pub mod ups;
pub mod vcd;

pub use self::err::*;

fn map_io_err(e: io::Error) -> io::Error {
  match e.kind() {
    io::ErrorKind::InvalidInput => io::ErrorKind::InvalidData.into(),
    io::ErrorKind::UnexpectedEof => io::ErrorKind::InvalidData.into(),
    _ => e,
  }
}

fn rom_err(err: io::Error) -> Error {
  // InvalidInput will occur when a smaller file offset is encountered in a
  // patch format where input file offsets should only increase.
  // See PositionTracker::take_from_inner_until
  match err.kind() {
    io::ErrorKind::InvalidInput => Error::BadPatch,
    io::ErrorKind::UnexpectedEof => Error::WrongInputFile,
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
pub struct Patch<R> {
  file: R,
  end_of_data: u64,
  crc32: Crc32,
  kind: Kind,
}

impl<P: Read + Seek> Patch<P> {
  pub fn new(patch: P) -> io::Result<Self> {
    let mut patch = PositionTracker::from_start(patch);
    let mut hasher = crc::CRC32Hasher::new();
    let magic = patch.read_array::<3>()?;
    let (kind, is_delta_file) = match &magic[..] {
      ips::MAGIC => (Kind::IPS, false),
      ups::MAGIC => (Kind::UPS, false),
      bps::MAGIC => (Kind::BPS, true),
      ppf::MAGIC => (Kind::PPF, false),
      vcd::MAGIC => (Kind::VCD, true),
      _ => return Err(io::Error::from(io::ErrorKind::InvalidData)),
    };
    hasher.write(&magic)?;
    let (mut last_4, _) = read_write_utils::copy_all_but_last::<4>(&mut patch, &mut hasher)?;
    let eof = patch.position();
    let internal_crc32 = hasher.finish();
    hasher.update(&mut last_4[..]);
    let file_crc32 = hasher.finish();
    Ok(Self {
      file: patch.into_inner(),
      kind,
      end_of_data: eof,
      is_delta_file,
      internal_crc32,
      file_crc32,
    })
  }
}

impl<P> Patch<P> {
  pub fn kind(&self) -> Kind {
    self.kind
  }

  pub fn file(&self) -> &P {
    &self.file
  }

  pub fn internal_crc32(&self) -> Crc32 {
    self.internal_crc32
  }

  pub fn crc32(&self) -> Crc32 {
    self.file_crc32
  }

  pub fn is_delta_file(&self) -> bool {
    self.is_delta_file
  }

  pub fn eof(&self) -> u64 {
    self.end_of_data
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

impl<T> HasInternalCrc32 for Patch<T> {
  fn internal_crc32(&self) -> Crc32 {
    self.internal_crc32
  }
}

#[derive(Copy, Clone, Debug)]
pub enum Kind {
  IPS {
    new_file_size: Option<num::NonZeroU32>,
  },
  UPS {
    rom_crc32: Crc32,
    result_crc32: Crc32,
  },
  BPS {
    rom_crc32: Crc32,
    result_crc32: Crc32,
  },
  PPF,
  VCD,
}

impl fmt::Display for Kind {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Kind::IPS { .. } => write!(f, "IPS"),
      Kind::UPS { .. } => write!(f, "UPS"),
      Kind::BPS { .. } => write!(f, "BPS"),
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
  ) -> Result<(), Error>
  where
    R: BufRead + Seek + HasCrc32,
    P: BufRead + Seek + HasInternalCrc32,
    O: BufWrite + Seek,
    for<'a> &'a mut O::Inner: Read + Write + Seek,
  {
    match self.0 {
      Kind::IPS { .. } => Patcher::ips(rom, patch, output),
      Kind::UPS { .. } => Patcher::ups(rom, patch, output, strict),
      Kind::BPS { .. } => Patcher::bps(rom, patch, output, strict),
      Kind::PPF => Patcher::ppf(rom, patch, output),
      Kind::VCD => Patcher::vcdiff(rom, patch, output),
    }
  }

  fn ips<R, P, O>(rom: &mut R, patch: &mut P, output: &mut O) -> Result<(), Error>
  where
    R: BufRead + Seek,
    P: BufRead,
    O: BufWrite,
  {
    ips::patch(rom, patch, output)?;
    Ok(())
  }

  fn ups<R, P, O>(
    rom: &mut R,
    patch: &mut P,
    output: &mut O,
    strict: bool,
  ) -> Result<(), crate::patch::Error>
  where
    R: BufRead + Seek + HasCrc32,
    P: BufRead + Seek + HasInternalCrc32,
    O: BufWrite + Seek,
    for<'a> &'a mut O::Inner: Read + Write + Seek,
  {
    ups::patch(rom, patch, output, strict).map(|_| ())
  }

  fn bps<R, P, O>(
    rom: &mut R,
    patch: &mut P,
    output: &mut O,
    strict: bool,
  ) -> Result<(), crate::patch::Error>
  where
    R: BufRead + Seek + HasCrc32,
    P: BufRead + Seek + HasInternalCrc32,
    O: BufWrite + Seek,
    for<'a> &'a mut O::Inner: Read + Write + Seek,
  {
    bps::patch(rom, patch, output, strict).map(|_| ())
  }

  fn ppf<R, P, O>(rom: &mut R, patch: &mut P, output: &mut O) -> Result<(), Error>
  where
    R: BufRead + Write + Seek,
    P: BufRead + Seek,
    O: BufWrite + Seek,
    for<'a> &'a mut O::Inner: Read + Write + Seek + Resize,
  {
    ppf::patch(rom, patch, output).map_err(|err| err.into())
  }

  fn vcdiff<R, P, O>(rom: &mut R, patch: &mut P, output: &mut O) -> Result<(), Error>
  where
    R: BufRead + Seek,
    P: BufRead + Seek,
    O: BufWrite + Seek,
    for<'a> &'a mut O::Inner: Read + Write + Seek,
  {
    vcd::patch(rom, patch, output)?;
    Ok(())
  }
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
    #[error("The patch is not intended for the input file.")]
    WrongInputFile,
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
