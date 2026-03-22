extern crate core;

use crate::error::prelude::*;
use std::process;

mod apply;
mod cli;
mod error;
mod filename;
mod hack;
mod kdl;
mod log;
mod manifest;
mod mem;
mod patch;
mod validate;

fn main() -> miette::Result<()> {
  use cli::CommandKind::*;

  log::init();
  let args: cli::Args = clap::Parser::try_parse().map_err(Error::from)?;
  match args.command {
    Apply(args) => args.call().map_err(|err| Error::from(err).into()),
    Validate(args) => args.call().map_err(|err| Error::Validation(err).into()),
  }
}

#[non_exhaustive]
#[derive(Debug, Error, Diagnostic)]
enum Error {
  #[error(transparent)]
  Cli(#[from] clap::error::Error),
  #[error(transparent)]
  #[diagnostic(transparent)]
  ApplyPatch(#[from] apply::Error),
  #[error(transparent)]
  #[diagnostic(transparent)]
  Validation(Box<kdl_schema_check::CheckFailure>),
}

impl From<kdl_schema_check::CheckFailure> for Error {
  fn from(err: kdl_schema_check::CheckFailure) -> Error {
    Error::Validation(Box::new(err))
  }
}

impl process::Termination for Error {
  fn report(self) -> process::ExitCode {
    use apply::ErrorKind as K;
    process::ExitCode::from(match self {
      Error::Cli(_) => 1,
      Error::ApplyPatch(err) => match err.get_kind() {
        K::IOError => 2,
        K::UnknownPatchKind => 3,
        K::Patching => 4,
        K::BadManifest => 5,
        K::AlreadyPatched => 6,
        K::ManifestOutdated => 7,
      },
      Error::Validation(_) => 4,
    })
  }
}
