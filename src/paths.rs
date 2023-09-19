use std::ops::Deref;
use std::{ffi, fmt, path, str};

#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct StrPath(path::Path);

impl StrPath {
  pub fn new<S: AsRef<str> + ?Sized>(s: &S) -> &Self {
    unsafe { Self::from_path_unchecked(path::Path::new(s.as_ref())) }
  }

  pub fn from_os_str<S: AsRef<ffi::OsStr> + ?Sized>(s: &S) -> Result<&Self, Error> {
    path::Path::new(s).try_into()
  }

  pub unsafe fn from_path_unchecked<P: AsRef<path::Path> + ?Sized>(path: &P) -> &Self {
    unsafe { &*(path.as_ref() as *const path::Path as *const Self) }
  }

  pub fn as_path(&self) -> &path::Path {
    &self.0
  }

  pub fn as_str(&self) -> &str {
    unsafe { self.0.to_str().unwrap_unchecked() }
  }

  pub fn file_name(&self) -> Option<&str> {
    (self.0)
      .file_name()
      .map(|os_str| unsafe { os_str.to_str().unwrap_unchecked() })
  }

  pub fn file_stem(&self) -> Option<&str> {
    (self.0)
      .file_stem()
      .map(|os_str| unsafe { os_str.to_str().unwrap_unchecked() })
  }

  pub fn parent(&self) -> Option<&StrPath> {
    (self.0)
      .parent()
      .map(|path| unsafe { StrPath::from_path_unchecked(path) })
  }
}

impl<'a> From<&'a str> for &'a StrPath {
  fn from(value: &'a str) -> Self {
    StrPath::new(value)
  }
}

impl<'a> TryFrom<&'a ffi::OsStr> for &'a StrPath {
  type Error = Error;

  fn try_from(value: &'a ffi::OsStr) -> Result<Self, Self::Error> {
    path::Path::new(value).try_into()
  }
}

impl<'a> TryFrom<&'a path::Path> for &'a StrPath {
  type Error = Error;
  fn try_from(value: &'a path::Path) -> Result<Self, Self::Error> {
    match value.to_str() {
      Some(_) => Ok(unsafe { StrPath::from_path_unchecked(value) }),
      None => Err(Error(Repr::NotUtf8)),
    }
  }
}

impl Deref for StrPath {
  type Target = path::Path;

  fn deref(&self) -> &Self::Target {
    self.as_path()
  }
}

impl fmt::Display for StrPath {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{}", self.as_str())
  }
}

impl AsRef<StrPath> for StrPath {
  fn as_ref(&self) -> &StrPath {
    self
  }
}

impl AsRef<path::Path> for StrPath {
  fn as_ref(&self) -> &path::Path {
    self.as_path()
  }
}

impl AsRef<str> for StrPath {
  fn as_ref(&self) -> &str {
    self.as_str()
  }
}

impl AsRef<ffi::OsStr> for StrPath {
  fn as_ref(&self) -> &ffi::OsStr {
    self.as_os_str()
  }
}

#[derive(Clone, Debug)]
pub struct StrPathBuf(path::PathBuf);

impl StrPathBuf {
  pub fn new() -> Self {
    Self(path::PathBuf::new())
  }

  pub fn from_path_buf(path_buf: path::PathBuf) -> Result<Self, path::PathBuf> {
    match path_buf.to_str() {
      Some(_) => Ok(Self(path_buf)),
      None => Err(path_buf),
    }
  }

  pub unsafe fn from_path_buf_unchecked(path_buf: path::PathBuf) -> Self {
    Self(path_buf)
  }

  pub unsafe fn from_path_unchecked<P: AsRef<path::Path> + ?Sized>(path: &P) -> Self {
    Self(path::PathBuf::from(path.as_ref()))
  }

  pub fn from_str<S: AsRef<str> + ?Sized>(s: &S) -> Self {
    StrPath::new(s).into()
  }

  pub fn from_os_str<S: AsRef<ffi::OsStr> + ?Sized>(os_str: &S) -> Result<Self, Error> {
    path::Path::new(os_str).try_into()
  }

  pub unsafe fn from_os_str_unchecked<S: AsRef<ffi::OsStr> + ?Sized>(os_str: &S) -> Self {
    Self::from_path_unchecked(&path::Path::new(os_str))
  }

  pub fn as_str_path(&self) -> &StrPath {
    unsafe { &*(self.0.as_path() as *const path::Path as *const StrPath) }
  }

  pub fn into_path_buf(self) -> path::PathBuf {
    self.0
  }

  pub fn into_string(self) -> String {
    unsafe { self.0.into_os_string().into_string().unwrap_unchecked() }
  }
}

impl From<String> for StrPathBuf {
  fn from(string: String) -> Self {
    Self(path::PathBuf::from(string))
  }
}

impl From<StrPathBuf> for String {
  fn from(str_path_buf: StrPathBuf) -> Self {
    str_path_buf.into_string()
  }
}

impl TryFrom<&path::Path> for StrPathBuf {
  type Error = Error;

  fn try_from(value: &path::Path) -> Result<Self, Self::Error> {
    match value.to_str() {
      Some(_) => Ok(Self(path::PathBuf::from(value))),
      None => Err(Error(Repr::NotUtf8)),
    }
  }
}

impl From<&StrPath> for StrPathBuf {
  fn from(value: &StrPath) -> Self {
    unsafe { StrPathBuf::from_path_buf_unchecked(path::PathBuf::from(value)) }
  }
}

impl Deref for StrPathBuf {
  type Target = StrPath;

  fn deref(&self) -> &Self::Target {
    self.as_str_path()
  }
}

impl<T> AsRef<T> for StrPathBuf
where
  T: ?Sized,
  <StrPathBuf as Deref>::Target: AsRef<T>,
{
  fn as_ref(&self) -> &T {
    self.deref().as_ref()
  }
}

impl fmt::Display for StrPathBuf {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{}", self.as_str())
  }
}

#[derive(Debug)]
pub struct FilePath(StrPath);

impl FilePath {
  pub unsafe fn from_str_path_unchecked<P: AsRef<StrPath> + ?Sized>(path: &P) -> &Self {
    unsafe { &*(path.as_ref() as *const StrPath as *const Self) }
  }

  pub unsafe fn from_str_unchecked<S: AsRef<str> + ?Sized>(s: &S) -> &Self {
    FilePath::from_str_path_unchecked(StrPath::new(s.as_ref()))
  }

  pub fn file_name(&self) -> &str {
    unsafe { self.0.file_name().unwrap_unchecked() }
  }

  pub fn file_stem(&self) -> &str {
    unsafe { self.0.file_stem().unwrap_unchecked() }
  }
}

impl<'a> TryFrom<&'a StrPath> for &'a FilePath {
  type Error = Error;

  fn try_from(value: &'a StrPath) -> Result<Self, Self::Error> {
    match value.file_name() {
      Some(_) => Ok(unsafe { FilePath::from_str_path_unchecked(value) }),
      None => Err(Error(Repr::NotAFile)),
    }
  }
}

impl Deref for FilePath {
  type Target = StrPath;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl<T> AsRef<T> for FilePath
where
  T: ?Sized,
  <FilePath as Deref>::Target: AsRef<T>,
{
  fn as_ref(&self) -> &T {
    self.deref().as_ref()
  }
}

impl AsRef<FilePath> for FilePath {
  fn as_ref(&self) -> &FilePath {
    self
  }
}

impl fmt::Display for FilePath {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    self.0.fmt(f)
  }
}

#[derive(Clone, Debug)]
pub struct FilePathBuf(StrPathBuf);

impl FilePathBuf {
  pub fn new(str_path_buf: StrPathBuf) -> Result<Self, StrPathBuf> {
    match path::Path::file_name(&str_path_buf) {
      Some(_) => Ok(Self(str_path_buf)),
      None => Err(str_path_buf),
    }
  }

  pub fn from_path(path: &path::Path) -> Result<Self, Error> {
    let str_path_buf =
      StrPathBuf::from_path_buf(path.to_path_buf()).map_err(|_| Error(Repr::NotUtf8))?;
    Self::new(str_path_buf).map_err(|_| Error(Repr::NotAFile))
  }

  pub fn from_path_buf(path_buf: path::PathBuf) -> Result<Self, path::PathBuf> {
    let str_path_buf = StrPathBuf::from_path_buf(path_buf)?;
    Self::new(str_path_buf).map_err(StrPathBuf::into_path_buf)
  }

  pub fn as_file_path(&self) -> &FilePath {
    unsafe { &*(self.0.as_str_path() as *const StrPath as *const FilePath) }
  }

  pub fn set_file_name<S: AsRef<str>>(&mut self, file_name: S) {
    self.0 .0.set_file_name(file_name.as_ref())
  }

  pub fn set_extension<S: AsRef<str>>(&mut self, ext: S) -> bool {
    self.0 .0.set_extension(ext.as_ref())
  }

  pub fn into_str_path_buf(self) -> StrPathBuf {
    self.0
  }

  pub fn push_str<S: AsRef<str> + ?Sized>(self, s: &S) -> Result<Self, StrPathBuf> {
    let mut str_path_buf = self.into_str_path_buf().into_string();
    str_path_buf.push_str(s.as_ref());
    return FilePathBuf::new(StrPathBuf::from(str_path_buf));
  }
}

impl From<&FilePath> for FilePathBuf {
  fn from(value: &FilePath) -> Self {
    Self(unsafe { StrPathBuf::from_path_unchecked(value) })
  }
}

impl TryFrom<path::PathBuf> for FilePathBuf {
  type Error = path::PathBuf;

  fn try_from(value: path::PathBuf) -> Result<Self, Self::Error> {
    Self::from_path_buf(value)
  }
}

impl Deref for FilePathBuf {
  type Target = FilePath;

  fn deref(&self) -> &Self::Target {
    self.as_file_path()
  }
}

impl fmt::Display for FilePathBuf {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{}", self.0)
  }
}

impl<T> AsRef<T> for FilePathBuf
where
  T: ?Sized,
  <FilePathBuf as Deref>::Target: AsRef<T>,
{
  fn as_ref(&self) -> &T {
    self.deref().as_ref()
  }
}

impl str::FromStr for FilePathBuf {
  type Err = Error;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    FilePathBuf::from_path(path::Path::new(s))
  }
}

#[derive(Debug)]
pub struct Error(Repr);

#[derive(Clone, Copy, Debug)]
enum Repr {
  NotUtf8,
  NotAFile,
}

impl fmt::Display for Error {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self.0 {
      Repr::NotUtf8 => write!(f, "path is not UTF-8"),
      Repr::NotAFile => write!(f, "path does not end in a file name"),
    }
  }
}

impl std::error::Error for Error {}
