use crate::error::prelude::*;
use crate::{io, path};
pub use std::fs::*;

pub fn read(path: impl AsRef<path::FilePath>) -> Result<Vec<u8>, Error> {
  std::fs::read(path.as_ref().as_path()).map_err(|err| Error::file(err, path))
}

pub fn write<P, C>(path: P, contents: C) -> Result<(), Error>
where
  P: AsRef<path::FilePath>,
  C: AsRef<[u8]>,
{
  std::fs::write(path.as_ref().as_path(), contents).map_err(|err| Error::file(err, path))
}

pub fn copy(
  from: impl AsRef<path::FilePath>,
  to: impl AsRef<path::FilePath>,
) -> Result<u64, Error> {
  std::fs::copy(from.as_ref().as_path(), to.as_ref().as_path())
    .map_err(|err| Error::copy(err, from, to))
}

pub fn rename(
  from: impl AsRef<path::FilePath>,
  to: impl AsRef<path::FilePath>,
) -> Result<(), Error> {
  std::fs::rename(from.as_ref().as_path(), to.as_ref().as_path())
    .map_err(|err| Error::rename(err, from, to))
}

#[derive(Debug, Error, Diagnostic)]
#[error(transparent)]
pub struct Error(anyhow::Error);

impl Error {
  pub fn file(error: io::Error, file: impl AsRef<path::FilePath>) -> Self {
    Self(anyhow::Error::new(error).context(format!(
      "Encountered an IO error for file \"{}\"",
      file.as_ref()
    )))
  }

  pub fn copy(
    error: io::Error,
    from: impl AsRef<path::FilePath>,
    to: impl AsRef<path::FilePath>,
  ) -> Self {
    Self(anyhow::Error::new(error).context(format!(
      "Failed to copy file \"{}\" to \"{}\"",
      from.as_ref(),
      to.as_ref()
    )))
  }

  pub fn rename(
    error: io::Error,
    from: impl AsRef<path::FilePath>,
    to: impl AsRef<path::FilePath>,
  ) -> Self {
    Self(anyhow::Error::new(error).context(format!(
      "Failed to rename file \"{}\" to \"{}\"",
      from.as_ref(),
      to.as_ref()
    )))
  }
}
