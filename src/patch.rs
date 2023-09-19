use crate::{fs, paths};
use std::{error, ffi, fmt};
use thiserror::Error;

#[derive(Clone, Debug)]
pub struct Patch {
  pub kind: Kind,
  pub path: paths::FilePathBuf,
}

impl Patch {
  pub fn new(kind: Kind, path: paths::FilePathBuf) -> Self {
    Self { kind, path }
  }
}

#[derive(Copy, Clone, Debug)]
pub enum Kind {
  IPS,
  UPS,
  BPS,
}

impl std::str::FromStr for Patch {
  type Err = UnknownPatchKindError;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    let path = paths::FilePathBuf::from_str(s)?;
    let ext = path
      .extension()
      .and_then(ffi::OsStr::to_str)
      .map(|str| str.to_ascii_lowercase())
      .ok_or(UnknownPatchKindError(()))?;
    match ext.as_str() {
      "ips" => Ok(Patch::new(Kind::IPS, path)),
      "ups" => Ok(Patch::new(Kind::UPS, path)),
      "bps" => Ok(Patch::new(Kind::BPS, path)),
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

impl From<paths::Error> for UnknownPatchKindError {
  fn from(_value: paths::Error) -> Self {
    UnknownPatchKindError(())
  }
}

#[derive(Clone, Copy, Debug)]
pub enum Tool {
  PatchCopy(fn(&paths::FilePath, &Patch, &paths::FilePath) -> Result<(), Error>),
  PatchInPlace(fn(&paths::FilePath, &Patch) -> Result<(), Error>),
}

impl Tool {
  pub const FLIPS: Self = flips::TOOL;
  pub fn from_patch_kind(patch_kind: Kind) -> Self {
    match patch_kind {
      Kind::IPS => Tool::FLIPS,
      Kind::UPS => Tool::FLIPS,
      Kind::BPS => Tool::FLIPS,
    }
  }
}

mod flips {
  use super::*;
  use ::flips;

  pub const TOOL: Tool = Tool::PatchInPlace(command);

  fn command(file: &paths::FilePath, patch: &Patch) -> Result<(), Error> {
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
    }
    Ok(())
  }
}

#[non_exhaustive]
#[derive(Debug, Error)]
pub enum Error {
  #[error(transparent)]
  IOError(#[from] fs::Error),
  #[error(transparent)]
  FlipsError(#[from] ::flips::Error),
}
