use crate::paths;
use miette::Diagnostic;
use std::io;
use thiserror::Error;
use Error as E;

pub use std::fs::*;

pub fn read(path: impl AsRef<paths::FilePath>) -> Result<Vec<u8>, Error> {
  std::fs::read(path.as_ref().as_path()).map_err(|err| E::ReadError(err, path.as_ref().into()))
}

pub fn write<P, C>(path: P, contents: C) -> Result<(), Error>
where
  P: AsRef<paths::FilePath>,
  C: AsRef<[u8]>,
{
  std::fs::write(path.as_ref().as_path(), contents)
    .map_err(|err| E::WriteError(err, path.as_ref().into()))
}

pub fn copy(
  from: impl AsRef<paths::FilePath>,
  to: impl AsRef<paths::FilePath>,
) -> Result<u64, Error> {
  std::fs::copy(from.as_ref().as_path(), to.as_ref().as_path())
    .map_err(|err| E::CopyError(err, from.as_ref().into(), to.as_ref().into()))
}

pub fn rename(
  from: impl AsRef<paths::FilePath>,
  to: impl AsRef<paths::FilePath>,
) -> Result<(), Error> {
  std::fs::rename(from.as_ref().as_path(), to.as_ref().as_path())
    .map_err(|err| E::RenameError(err, from.as_ref().into(), to.as_ref().into()))
}

#[non_exhaustive]
#[derive(Debug, Error, Diagnostic)]
pub enum Error {
  #[error("Encountered an IO error while reading file \"{1}\": {0}")]
  ReadError(#[source] io::Error, paths::FilePathBuf),
  #[error("Encountered an IO error while writing to file \"{1}\": {0}")]
  WriteError(#[source] io::Error, paths::FilePathBuf),
  #[error("Failed to copy file \"{1}\" to \"{2}\": {0}")]
  CopyError(#[source] io::Error, paths::FilePathBuf, paths::FilePathBuf),
  #[error("Failed to rename file \"{1}\" to \"{2}\": {0}")]
  RenameError(#[source] io::Error, paths::FilePathBuf, paths::FilePathBuf),
}
