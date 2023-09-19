use miette::Diagnostic;
use std::process;
use thiserror::Error;

mod apply;
mod cli;
mod filename;
mod fs;
mod hack;
mod kdl;
mod log;
mod manifest;
mod patch;
mod paths;
mod sha;
mod val;
mod validate;

fn main() -> miette::Result<()> {
  use cli::CommandKind::*;

  log::init();
  let args: cli::Args = clap::Parser::try_parse().map_err(|err| Error::from(err))?;
  return match args.command {
    Apply(args) => args.call().map_err(|err| Error::from(err).into()),
    Validate(args) => args.call().map_err(|err| Error::ValidateError(err).into()),
  };
}

#[non_exhaustive]
#[derive(Debug, Error, Diagnostic)]
enum Error {
  #[error(transparent)]
  CliError(#[from] clap::error::Error),
  #[error(transparent)]
  #[diagnostic(transparent)]
  ApplyPatchError(#[from] apply::Error),
  #[error(transparent)]
  #[diagnostic(transparent)]
  ValidateError(#[from] kdl_schema_check::CheckFailure),
}

impl process::Termination for Error {
  fn report(self) -> process::ExitCode {
    use apply::ErrorKind as K;
    process::ExitCode::from(match self {
      Error::CliError(_) => 1,
      Error::ApplyPatchError(err) => match err.get_kind() {
        K::IOError => 2,
        K::BadManifest => 3,
        K::AlreadyPatched => 4,
        K::ManifestOutdated => 5,
        K::PatchToolError => 6,
      },
      Error::ValidateError(_) => 2,
    })
  }
}
