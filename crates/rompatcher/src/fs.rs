use crate::crc::WithCrc32;
use fs_err as fs;
use read_write_utils::prelude::*;
use std::io::{BufReader, BufWriter, Take, Write};
use std::path::Path;

pub trait HasPath {
  fn path(&self) -> &Path;
}

impl HasPath for fs::File {
  fn path(&self) -> &Path {
    self.path()
  }
}

impl<T: HasPath> HasPath for BufReader<T> {
  fn path(&self) -> &Path {
    self.get_ref().path()
  }
}

impl<T: HasPath + Write> HasPath for BufWriter<T> {
  fn path(&self) -> &Path {
    self.get_ref().path()
  }
}

impl<T: HasPath> HasPath for Take<T> {
  fn path(&self) -> &Path {
    self.get_ref().path()
  }
}

impl<T: HasPath> HasPath for PositionTracker<T> {
  fn path(&self) -> &Path {
    self.inner().path()
  }
}

impl<T: HasPath> HasPath for WithCrc32<T> {
  fn path(&self) -> &Path {
    self.get_ref().path()
  }
}
