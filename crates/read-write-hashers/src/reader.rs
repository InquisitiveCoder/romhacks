use read_write_utils::prelude::SeekRelative;
use std::hash::Hasher;
use std::io;
use std::io::prelude::*;

/// A [`Read`] adapter that hashes bytes [`read`][1] or [`consumed`][2] from its
/// underlying reader.
///
/// If you need to hash every byte in a reader while also seeking back and forth,
/// consider using [`MonotonicHashingReader`][3].
///
/// [1]: Read::read
/// [2]: BufRead::consume
/// [3]: crate::MonotonicHashingReader
pub struct HashingReader<R, H> {
  inner: R,
  hasher: H,
}

impl<R, H> HashingReader<R, H>
where
  R: Read,
  H: Hasher,
{
  pub fn new(inner: R, hasher: H) -> Self {
    Self { inner, hasher }
  }
}

impl<R, H> HashingReader<R, H> {
  pub fn inner(&self) -> &R {
    &self.inner
  }

  pub fn hasher(&self) -> &H {
    &self.hasher
  }

  pub fn into_inner(self) -> R {
    self.inner
  }

  pub fn into_hasher(self) -> H {
    self.hasher
  }

  pub fn into_parts(self) -> (R, H) {
    (self.inner, self.hasher)
  }
}

impl<R, H> Read for HashingReader<R, H>
where
  R: Read,
  H: Hasher,
{
  /// Calls [`read`](Read::read) on the inner reader and hashes the bytes.
  fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
    let amt = self.inner.read(buf)?;
    self.hasher.write(&buf[..amt]);
    Ok(amt)
  }
}

impl<R, H> BufRead for HashingReader<R, H>
where
  R: BufRead,
  H: Hasher,
{
  fn fill_buf(&mut self) -> io::Result<&[u8]> {
    self.inner.fill_buf()
  }

  fn consume(&mut self, amt: usize) {
    if amt == 0 {
      return;
    }
    // Since amt > 0, the reader must've returned a non-empty buffer during the
    // previous call to fill_buf, and must return it again without attempting to
    // refill it.
    let buf = self.inner.fill_buf().expect(
      "consume must be called after fill_buf and amt must be <= \
              the number of bytes in the buffer.",
    );
    self.hasher.write(&buf[..amt]);
    self.inner.consume(amt)
  }
}

impl<W: Write, H> Write for HashingReader<W, H> {
  fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
    self.inner.write(buf)
  }

  fn flush(&mut self) -> io::Result<()> {
    self.inner.flush()
  }
}

impl<R, H> Seek for HashingReader<R, H>
where
  R: Seek,
{
  fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
    self.inner.seek(pos)
  }
}

impl<S, H> SeekRelative for HashingReader<S, H>
where
  S: SeekRelative,
  H: Hasher,
{
  fn relative_seek(&mut self, offset: i64) -> io::Result<()> {
    self.inner.relative_seek(offset)
  }
}

pub trait HashingReaderExt: Read + Sized {
  fn hash_reads<H: Hasher>(self, hasher: H) -> HashingReader<Self, H> {
    HashingReader::new(self, hasher)
  }
}
impl<R: Read + Sized> HashingReaderExt for R {}
