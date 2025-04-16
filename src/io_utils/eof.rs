use std::io;
use std::io::prelude::*;

pub struct WithEOF<T> {
  inner: T,
  eof: u64,
}

impl<T> WithEOF<T> {
  pub fn eof(&self) -> u64 {
    self.eof
  }
}

impl<T> WithEOF<T> {
  pub fn from_known_unchecked(inner: T, eof: u64) -> Self {
    Self { inner, eof }
  }
}

impl<E: KnownEOF> WithEOF<E> {
  pub fn from_known(inner: E) -> Self {
    let eof = inner.eof();
    Self { inner, eof }
  }
}

impl<T: Seek> WithEOF<T> {
  /// Seeks to the end of stream and creates a new instance.
  pub fn from_stream(mut inner: T) -> io::Result<Self> {
    let eof = inner.seek(io::SeekFrom::End(0))?;
    Ok(Self { inner, eof })
  }
}

impl<R: Read> Read for WithEOF<R> {
  fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
    self.inner.read(buf)
  }
}

impl<B: BufRead> BufRead for WithEOF<B> {
  fn fill_buf(&mut self) -> io::Result<&[u8]> {
    self.inner.fill_buf()
  }

  fn consume(&mut self, amt: usize) {
    self.inner.consume(amt)
  }
}

pub trait KnownEOF {
  fn eof(&self) -> u64;
}

impl<T> KnownEOF for WithEOF<T> {
  fn eof(&self) -> u64 {
    WithEOF::eof(self)
  }
}

impl KnownEOF for &[u8] {
  fn eof(&self) -> u64 {
    self.len() as u64
  }
}

impl KnownEOF for &mut [u8] {
  fn eof(&self) -> u64 {
    self.len() as u64
  }
}

impl KnownEOF for io::Cursor<&[u8]> {
  fn eof(&self) -> u64 {
    self.get_ref().len() as u64
  }
}

impl KnownEOF for io::Cursor<&mut [u8]> {
  fn eof(&self) -> u64 {
    self.get_ref().len() as u64
  }
}
