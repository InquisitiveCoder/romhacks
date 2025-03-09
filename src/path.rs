use std::ffi::OsStr;
use std::ops::Deref;
use std::{fmt, marker, str};
use thiserror::Error;

pub use std::path::*;
use std::str::FromStr;
pub use typed_path::{Utf8Encoding, Utf8NativeEncoding, Utf8Path, Utf8PathBuf};

pub type Utf8FilePath<E> = FilePath<Box<Utf8Path<E>>, E>;
pub type Utf8NativeFilePath = Utf8FilePath<Utf8NativeEncoding>;

#[derive(Clone, Copy, Debug, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct FilePath<P, E>(P, marker::PhantomData<E>);

impl<P, E> FilePath<P, E>
where
  P: AsRef<Utf8Path<E>>,
  E: for<'enc> Utf8Encoding<'enc>,
{
  pub fn try_new(utf8_path: P) -> Result<Self, FilePathError> {
    match utf8_path.as_ref().file_name() {
      Some(_) => Ok(Self(utf8_path, marker::PhantomData)),
      None => Err(FilePathError(())),
    }
  }

  pub unsafe fn new_unchecked(path: P) -> Self {
    Self(path, marker::PhantomData)
  }

  pub fn file_name(&self) -> &str {
    unsafe { self.0.as_ref().file_name().unwrap_unchecked() }
  }

  pub fn file_stem(&self) -> &str {
    unsafe { self.0.as_ref().file_stem().unwrap_unchecked() }
  }

  pub fn as_str(&self) -> &str {
    self.0.as_ref().as_str()
  }

  pub fn as_path(&self) -> &Path {
    Path::new(self.as_str())
  }
}

impl<E> TryFrom<Box<Utf8Path<E>>> for Utf8FilePath<E>
where
  E: for<'enc> Utf8Encoding<'enc>,
{
  type Error = FilePathError;

  fn try_from(utf8_path: Box<Utf8Path<E>>) -> Result<Self, Self::Error> {
    Self::try_new(utf8_path)
  }
}

impl<E> TryFrom<Utf8PathBuf<E>> for FilePath<Utf8PathBuf<E>, E>
where
  E: for<'enc> Utf8Encoding<'enc>,
{
  type Error = FilePathError;

  fn try_from(utf8_path: Utf8PathBuf<E>) -> Result<Self, Self::Error> {
    Self::try_new(utf8_path)
  }
}

impl<P, E> Deref for FilePath<P, E> {
  type Target = P;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl<P, E> AsRef<P> for FilePath<P, E>
where
  <FilePath<P, E> as Deref>::Target: AsRef<P>,
{
  fn as_ref(&self) -> &P {
    self.deref().as_ref()
  }
}

impl<P, E> AsRef<Path> for FilePath<P, E>
where
  P: AsRef<Utf8Path<E>>,
  E: for<'enc> Utf8Encoding<'enc>,
{
  fn as_ref(&self) -> &Path {
    self.as_path()
  }
}

impl<P, E> AsRef<OsStr> for FilePath<P, E>
where
  P: AsRef<Utf8Path<E>>,
  E: for<'enc> Utf8Encoding<'enc>,
{
  fn as_ref(&self) -> &OsStr {
    OsStr::new(self.0.as_ref().as_str())
  }
}

impl<P, E> AsRef<Self> for FilePath<P, E> {
  fn as_ref(&self) -> &Self {
    self
  }
}

impl<P, E> From<FilePath<P, E>> for String
where
  P: AsRef<Utf8Path<E>>,
  E: for<'enc> Utf8Encoding<'enc>,
{
  fn from(file_path: FilePath<P, E>) -> Self {
    file_path.0.as_ref().as_str().into()
  }
}

impl<E> TryFrom<String> for Utf8FilePath<E>
where
  E: for<'enc> Utf8Encoding<'enc>,
{
  type Error = FilePathError;

  fn try_from(string: String) -> Result<Self, Self::Error> {
    Self::try_new(Utf8PathBuf::from(string).into_boxed_path())
  }
}

impl From<Utf8NativeFilePath> for PathBuf {
  fn from(file_path: Utf8NativeFilePath) -> Self {
    PathBuf::from(file_path.as_path())
  }
}

impl<P, E> fmt::Display for FilePath<P, E>
where
  P: fmt::Display,
{
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    self.0.fmt(f)
  }
}

impl<E> FromStr for Utf8FilePath<E>
where
  E: for<'enc> Utf8Encoding<'enc>,
{
  type Err = FilePathError;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    Self::try_new(Utf8PathBuf::from(s).into_boxed_path())
  }
}

#[derive(Clone, Debug, Error)]
#[error("path does not end in a file name")]
pub struct FilePathError(());
