use crate::error::prelude::*;
use crate::{error, fs, io, path};
use std::{ffi, fmt};

pub mod ips;
pub mod ppf;

#[derive(Clone, Debug)]
pub struct Patch {
  pub kind: Kind,
  pub path: path::FilePathBuf,
}

impl Patch {
  pub fn new(kind: Kind, path: path::FilePathBuf) -> Self {
    Self { kind, path }
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

impl std::str::FromStr for Patch {
  type Err = UnknownPatchKindError;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    let path = path::FilePathBuf::from_str(s)?;
    let ext = path
      .extension()
      .and_then(ffi::OsStr::to_str)
      .map(|str| str.to_ascii_lowercase())
      .ok_or(UnknownPatchKindError(()))?;
    match ext.as_str() {
      "ips" => Ok(Patch::new(Kind::IPS, path)),
      "ups" => Ok(Patch::new(Kind::UPS, path)),
      "bps" => Ok(Patch::new(Kind::BPS, path)),
      "ppf" => Ok(Patch::new(Kind::PPF, path)),
      "xdelta" => Ok(Patch::new(Kind::XDELTA, path)),
      _ => Err(UnknownPatchKindError(())),
    }
  }
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

impl From<path::Error> for UnknownPatchKindError {
  fn from(_value: path::Error) -> Self {
    UnknownPatchKindError(())
  }
}

#[derive(Clone, Copy, Debug)]
pub struct Patcher(PatchFn);

impl Patcher {
  pub const FLIPS: Self = flips::TOOL;
  pub fn from_patch_kind(patch_kind: Kind) -> Self {
    match patch_kind {
      Kind::IPS => Patcher(Patcher::ips),
      Kind::UPS => Patcher::FLIPS,
      Kind::BPS => Patcher::FLIPS,
      Kind::PPF => Patcher(Patcher::ppf),
      Kind::XDELTA => Patcher(Patcher::xdelta3),
    }
  }

  pub fn patch(&self, file: impl AsRef<path::FilePath>, patch: &Patch) -> Result<(), Error> {
    (self.0)(file.as_ref(), patch)
  }

  pub fn ips(file: &path::FilePath, patch: &Patch) -> Result<(), Error> {
    let mut rom = fs::OpenOptions::new().write(true).open(&file)?;
    let mut ips = io::BufReader::new(fs::File::open(&patch.path)?);
    ips::patch(&mut rom, &mut ips)?;
    Ok(())
  }

  fn ppf(file: &path::FilePath, patch: &Patch) -> Result<(), Error> {
    let mut rom = fs::OpenOptions::new().read(true).write(true).open(&file)?;
    let mut ppf = io::BufReader::new(fs::File::open(&patch.path)?);
    ppf::patch(&mut rom, &mut ppf).map_err(|err| err.into())
  }

  fn xdelta3(file: &path::FilePath, patch: &Patch) -> Result<(), Error> {
    let rom = fs::read(file)?;
    let patch = fs::read(&patch.path)?;
    let patched = xdelta3::decode(&patch, &rom).ok_or(Error::XDelta3)?;
    Ok(fs::write(file, &patched)?)
  }
}

type PatchFn = fn(&path::FilePath, &Patch) -> Result<(), Error>;

mod flips {
  use super::*;
  use ::flips;

  pub const TOOL: Patcher = Patcher(command);

  fn command(file: &path::FilePath, patch: &Patch) -> Result<(), Error> {
    let patch_kind = patch.kind;
    let rom = fs::read(file)?;
    let patch = fs::read(&patch.path)?;
    match patch_kind {
      Kind::IPS => {
        let output = flips::IpsPatch::new(patch).apply(rom)?;
        fs::write(file, output)?;
      }
      Kind::UPS => {
        let output = flips::UpsPatch::new(patch).apply(rom)?;
        fs::write(file, output)?
      }
      Kind::BPS => {
        let output = flips::BpsPatch::new(patch).apply(rom)?;
        fs::write(file, output)?;
      }
      _ => unreachable!(),
    }
    Ok(())
  }
}

#[non_exhaustive]
#[derive(Debug, Error)]
pub enum Error {
  #[error(transparent)]
  IO(#[from] io::Error),
  #[error(transparent)]
  File(#[from] fs::Error),
  #[error(transparent)]
  Flips(#[from] ::flips::Error),
  #[error(transparent)]
  IPS(#[from] ips::Error),
  #[error(transparent)]
  PPF(#[from] ppf::Error),
  #[error("Failed to apply XDelta 3 patch.")]
  XDelta3,
}
