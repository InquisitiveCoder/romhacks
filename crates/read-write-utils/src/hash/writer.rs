use super::*;

/// A [`Write`] adapter that hashes the bytes written to its underlying writer.
pub struct HashingWriter<W, H> {
  inner: W,
  hasher: H,
}

impl<W, H> HashingWriter<W, H>
where
  W: Write,
  H: Hasher,
{
  pub fn new(inner: W, hasher: H) -> Self {
    Self { inner, hasher }
  }
}

impl<W, H> HashingWriter<W, H> {
  /// Gets a reference to the underlying writer.
  pub fn inner(&self) -> &W {
    &self.inner
  }

  /// Gets a mutable reference to the underlying writer.
  pub fn inner_mut(&mut self) -> &mut W {
    &mut self.inner
  }

  pub fn hasher(&self) -> &H {
    &self.hasher
  }

  pub fn into_inner(self) -> W {
    self.inner
  }

  pub fn into_hasher(self) -> H {
    self.hasher
  }

  pub fn into_parts(self) -> (W, H) {
    (self.inner, self.hasher)
  }
}

impl<R: Read, H> Read for HashingWriter<R, H> {
  fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
    self.inner.read(buf)
  }
}

impl<R: BufRead, H> BufRead for HashingWriter<R, H> {
  fn fill_buf(&mut self) -> io::Result<&[u8]> {
    self.inner.fill_buf()
  }

  fn consume(&mut self, amt: usize) {
    self.inner.consume(amt)
  }
}

impl<W, H> Write for HashingWriter<W, H>
where
  W: Write,
  H: Hasher,
{
  /// Calls [`write`](Write::write) on the inner writer, then hashes the bytes
  /// that were successfully written.
  fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
    let amt = self.inner.write(buf)?;
    self.hasher.write(&buf[..amt]);
    Ok(amt)
  }

  fn flush(&mut self) -> io::Result<()> {
    self.inner.flush()
  }
}

impl<W: BufWrite, H: Hasher> BufWrite for HashingWriter<W, H> {
  type Inner = W;

  fn get_ref(&self) -> &Self::Inner {
    self.inner()
  }

  fn get_mut(&mut self) -> &mut Self::Inner {
    self.inner_mut()
  }
}

impl<W: Seek, H> Seek for HashingWriter<W, H> {
  fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
    self.inner.seek(pos)
  }
}

pub trait WriteAndHashExt: Write + Sized {
  fn hash_writes<H: Hasher>(self, hasher: H) -> HashingWriter<Self, H> {
    HashingWriter::new(self, hasher)
  }
}
impl<W: Write + Sized> WriteAndHashExt for W {}
