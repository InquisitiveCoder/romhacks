use std::{error, ffi, fmt, path, process};

#[derive(Clone, Debug)]
pub struct Patch {
  pub kind: Kind,
  pub path: crate::paths::FilePathBuf,
}

impl Patch {
  pub fn new(kind: Kind, path: crate::paths::FilePathBuf) -> Self {
    Self { kind, path }
  }
}

#[derive(Copy, Clone, Debug)]
pub enum Kind {
  IPS,
  BPS,
  PPF,
  XDelta,
  Ninja2,
  FFP,
  GDIFF,
}

impl std::str::FromStr for Patch {
  type Err = UnknownPatchKindError;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    let path = crate::paths::FilePathBuf::from_str(s)?;
    let ext = path
      .extension()
      .and_then(ffi::OsStr::to_str)
      .map(|str| str.to_ascii_lowercase())
      .ok_or(UnknownPatchKindError(()))?;
    match ext.as_str() {
      "ips" => Ok(Patch::new(Kind::IPS, path)),
      "bps" => Ok(Patch::new(Kind::BPS, path)),
      "ppf" => Ok(Patch::new(Kind::PPF, path)),
      "xdelta" => Ok(Patch::new(Kind::XDelta, path)),
      "rup" => Ok(Patch::new(Kind::Ninja2, path)),
      "ffp" => Ok(Patch::new(Kind::FFP, path)),
      "pat" => Ok(Patch::new(Kind::FFP, path)),
      "gdiff" => Ok(Patch::new(Kind::GDIFF, path)),
      _ => Err(UnknownPatchKindError(())),
    }
  }
}

impl fmt::Display for Kind {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Kind::IPS => write!(f, "IPS"),
      Kind::BPS => write!(f, "BPS"),
      Kind::PPF => write!(f, "PPF"),
      Kind::XDelta => write!(f, "Xdelta"),
      Kind::Ninja2 => write!(f, "NINJA 2.0"),
      Kind::FFP => write!(f, "FireFlower Patch"),
      Kind::GDIFF => write!(f, "Generic Diff Format"),
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

impl From<crate::paths::Error> for UnknownPatchKindError {
  fn from(_value: crate::paths::Error) -> Self {
    UnknownPatchKindError(())
  }
}

#[derive(Clone, Copy, Debug)]
pub struct Tool {
  name: &'static str,
  program: &'static str,
  builder: CommandBuilder,
}

impl Tool {
  pub const FLIPS: Self = flips::TOOL;
  pub const APPLY_PPF_3: Self = applyppf3::TOOL;
  pub const X_DELTA_3: Self = xdelta3::TOOL;
  pub const NINJA_2: Self = ninja2::TOOL;

  pub fn from_patch_kind(patch_kind: Kind) -> Self {
    match patch_kind {
      Kind::IPS => Tool::FLIPS,
      Kind::BPS => Tool::FLIPS,
      Kind::PPF => Tool::APPLY_PPF_3,
      Kind::XDelta => Tool::X_DELTA_3,
      Kind::Ninja2 => Tool::NINJA_2,
      Kind::FFP => Tool::NINJA_2,
      Kind::GDIFF => Tool::NINJA_2,
    }
  }

  pub fn name(&self) -> &'static str {
    self.name
  }

  pub fn program(&self) -> &'static crate::paths::FilePath {
    unsafe { crate::paths::FilePath::from_str_unchecked(self.program) }
  }

  pub fn command_builder(&self) -> &CommandBuilder {
    &self.builder
  }
}

#[derive(Clone, Copy, Debug)]
pub enum CommandBuilder {
  PatchCopy(fn(&path::Path, &path::Path, &path::Path) -> process::Command),
  PatchInPlace(fn(&path::Path, &path::Path) -> process::Command),
}

impl fmt::Display for Tool {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{}", self.name())
  }
}

mod flips {
  use std::{path, process};

  pub const TOOL: super::Tool = super::Tool {
    name: "Floating IPS",
    program: "flips",
    builder: super::CommandBuilder::PatchInPlace(command),
  };

  fn command(file: &path::Path, patch: &path::Path) -> process::Command {
    let mut command = process::Command::new(TOOL.program);
    command.arg(patch).arg(file);
    command
  }
}

mod applyppf3 {
  use std::{path, process};

  pub const TOOL: super::Tool = super::Tool {
    name: "ApplyPPF3",
    program: "applyppf3",
    builder: super::CommandBuilder::PatchInPlace(command),
  };

  fn command(file: &path::Path, patch: &path::Path) -> process::Command {
    let mut command = process::Command::new(TOOL.program);
    command.arg("a").arg(file).arg(patch);
    command
  }
}

mod xdelta3 {
  use std::{path, process};

  pub const TOOL: super::Tool = super::Tool {
    name: "Xdelta3",
    program: "xdelta3",
    builder: super::CommandBuilder::PatchCopy(command),
  };

  fn command(file: &path::Path, patch: &path::Path, output: &path::Path) -> process::Command {
    let mut command = process::Command::new(TOOL.program);
    command.arg("-ds").arg(file).arg(patch).arg(output);
    command
  }
}

mod ninja2 {
  use std::{path, process};

  pub const TOOL: super::Tool = super::Tool {
    name: "NINJA 2.0",
    program: if cfg!(windows) { "ninja.bat" } else { "ninja2.php" },
    builder: super::CommandBuilder::PatchInPlace(command),
  };

  fn command(file: &path::Path, patch: &path::Path) -> process::Command {
    let mut command = process::Command::new(TOOL.program);
    // The argument order here is intentional
    command.arg(patch).arg(file);
    command
  }
}
