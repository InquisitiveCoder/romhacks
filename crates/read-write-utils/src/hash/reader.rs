use super::*;

/// A [`Read`] adapter that hashes the bytes read from its underlying reader.
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
    // Since amt > 0 and amt <= than the number of bytes in the buffer, this
    // call to fill_buf() must return the buffer without refilling it.
    let buf = self.inner.fill_buf().unwrap();
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

pub trait HashingReaderExt: Read + Sized {
  fn hash_reads<H: Hasher>(self, hasher: H) -> HashingReader<Self, H> {
    HashingReader::new(self, hasher)
  }
}
impl<R: Read + Sized> HashingReaderExt for R {}
