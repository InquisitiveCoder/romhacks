use crate::io_utils::prelude::*;
use std::hash::Hasher as StdHasher;
use std::io;
use std::io::prelude::*;
use std::ops::Deref;

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct Crc32(u32);

pub fn crc32<R: Read>(reader: &mut R) -> io::Result<(Crc32, u64)> {
  Hasher::new().hash(reader)
}

impl Crc32 {
  pub fn new(value: u32) -> Self {
    Self(value)
  }

  pub fn value(&self) -> u32 {
    self.0
  }
}

impl From<Hasher> for Crc32 {
  fn from(hasher: Hasher) -> Self {
    Self(hasher.into_inner().finalize())
  }
}

impl From<u32> for Crc32 {
  fn from(value: u32) -> Self {
    Self(value)
  }
}

/// A [`Read`] adapter that calculates the checksum of the data it reads from the
/// underlying reader.
pub struct ReadAndCrc32<R> {
  inner: R,
  hasher: Hasher,
}

impl<R: Read> ReadAndCrc32<R> {
  pub fn new(inner: R) -> Self {
    Self { inner, hasher: Hasher::new() }
  }
}

impl<R> ReadAndCrc32<R> {
  pub fn into_inner(self) -> R {
    self.inner
  }

  /// Returns the checksum of the data read so far.
  pub fn crc32(&self) -> Crc32 {
    self.hasher.finish()
  }
}

impl<R: Read> Read for ReadAndCrc32<R> {
  fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
    let len = self.inner.read(buf)?;
    self.hasher.update(&buf[..len]);
    Ok(len)
  }
}

impl<R: BufRead> BufRead for ReadAndCrc32<R> {
  fn fill_buf(&mut self) -> io::Result<&[u8]> {
    self.inner.fill_buf()
  }

  fn consume(&mut self, amt: usize) {
    self.inner.consume(amt)
  }
}

impl<S: Seek> Seek for ReadAndCrc32<S> {
  fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
    self.inner.seek(pos)
  }
}

pub struct WithCrc32<R> {
  inner: R,
  crc32: Crc32,
  eof: u64,
}

impl<R: Read> WithCrc32<R> {
  pub fn new(mut inner: R) -> io::Result<Self> {
    let (crc32, eof) = Hasher::new().hash(&mut inner)?;
    Ok(Self { inner, crc32, eof })
  }
}

impl<R> WithCrc32<R> {
  pub fn get_ref(&self) -> &R {
    &self.inner
  }

  pub fn into_inner(self) -> R {
    self.inner
  }

  pub fn crc32(&self) -> Crc32 {
    self.crc32
  }

  pub fn eof(&self) -> u64 {
    self.eof
  }
}

impl<R: Read> Read for WithCrc32<R> {
  fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
    self.inner.read(buf)
  }
}

impl<B: BufRead> BufRead for WithCrc32<B> {
  fn fill_buf(&mut self) -> io::Result<&[u8]> {
    self.inner.fill_buf()
  }

  fn consume(&mut self, amt: usize) {
    self.inner.consume(amt)
  }
}

impl<S: Seek> Seek for WithCrc32<S> {
  fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
    self.inner.seek(pos)
  }
}

impl<I> Deref for WithCrc32<I> {
  type Target = I;

  fn deref(&self) -> &Self::Target {
    &self.inner
  }
}

/// Types that include a crc32 checksum of their own content.
pub trait HasCrc32 {
  fn crc32(&self) -> Crc32;
}

impl<R: Read> HasCrc32 for WithCrc32<R> {
  fn crc32(&self) -> Crc32 {
    self.crc32
  }
}

pub struct Hasher(crc32fast::Hasher);

impl Hasher {
  pub fn new() -> Self {
    Self(crc32fast::Hasher::new())
  }

  pub fn hash<R: Read>(&mut self, reader: &mut R) -> io::Result<(Crc32, u64)> {
    let len = io::copy(reader, self)?;
    let crc32 = Crc32::new(self.0.finish() as u32);
    Ok((crc32, len))
  }

  pub fn update(&mut self, bytes: &[u8]) {
    self.0.update(bytes);
  }

  pub fn finish(&self) -> Crc32 {
    Crc32(self.0.finish() as u32)
  }

  pub fn into_inner(self) -> crc32fast::Hasher {
    self.0
  }
}

impl Write for Hasher {
  fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
    self.0.update(buf);
    Ok(buf.len())
  }

  fn flush(&mut self) -> io::Result<()> {
    Ok(())
  }
}

impl<R> KnownEOF for WithCrc32<R> {
  fn eof(&self) -> u64 {
    self.eof
  }
}
