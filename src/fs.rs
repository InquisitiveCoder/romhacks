use crate::error::prelude::*;
use crate::{io, path};
pub use std::fs::*;

pub fn read(path: impl AsRef<path::FilePath>) -> Result<Vec<u8>, Error> {
  std::fs::read(path.as_ref().as_path()).map_err(|err| Error::File(err, path.as_ref().into()))
}

pub fn write<P, C>(path: P, contents: C) -> Result<(), Error>
where
  P: AsRef<path::FilePath>,
  C: AsRef<[u8]>,
{
  std::fs::write(path.as_ref().as_path(), contents)
    .map_err(|err| Error::File(err, path.as_ref().into()))
}

pub fn copy(
  from: impl AsRef<path::FilePath>,
  to: impl AsRef<path::FilePath>,
) -> Result<u64, Error> {
  std::fs::copy(from.as_ref().as_path(), to.as_ref().as_path())
    .map_err(|err| Error::Copy(err, from.as_ref().into(), to.as_ref().into()))
}

pub fn rename(
  from: impl AsRef<path::FilePath>,
  to: impl AsRef<path::FilePath>,
) -> Result<(), Error> {
  std::fs::rename(from.as_ref().as_path(), to.as_ref().as_path())
    .map_err(|err| Error::Rename(err, from.as_ref().into(), to.as_ref().into()))
}

#[non_exhaustive]
#[derive(Debug, Error, Diagnostic)]
pub enum Error {
  #[error("Encountered an IO error for file \"{1}\": {0}")]
  File(#[source] io::Error, path::FilePathBuf),
  #[error("Failed to copy file \"{1}\" to \"{2}\": {0}")]
  Copy(#[source] io::Error, path::FilePathBuf, path::FilePathBuf),
  #[error("Failed to rename file \"{1}\" to \"{2}\": {0}")]
  Rename(#[source] io::Error, path::FilePathBuf, path::FilePathBuf),
}
