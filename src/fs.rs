use crate::paths;
use miette::Diagnostic;
use std::io;
use thiserror::Error;

pub use std::fs::*;

#[non_exhaustive]
#[derive(Debug, Error, Diagnostic)]
pub enum Error {
  #[error("Encountered an IO error while reading file \"{1}\": {0}")]
  ReadError(#[source] io::Error, paths::FilePathBuf),
  #[error("Encountered an IO error while writing to file \"{1}\": {0}")]
  WriteError(#[source] io::Error, paths::FilePathBuf),
  #[error("Encountered an IO error while executing binary \"{1}\": {0}")]
  ExecError(#[source] io::Error, &'static paths::FilePath),
  #[error("Failed to copy file \"{1}\" to \"{2}\": {0}")]
  CopyError(#[source] io::Error, paths::FilePathBuf, paths::FilePathBuf),
  #[error("Failed to rename file \"{1}\" to \"{2}\": {0}")]
  RenameError(#[source] io::Error, paths::FilePathBuf, paths::FilePathBuf),
}
