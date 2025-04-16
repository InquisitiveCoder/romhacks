use std::io::Write;
use std::ops::Deref;

/// An adapter that removes Read and Seek from its underlying writer.
#[derive(Debug)]
pub struct AppendOnly<W> {
  inner: W,
}

impl<W: Write> AppendOnly<W> {
  pub fn new(inner: W) -> Self {
    Self { inner }
  }
}

impl<W> AppendOnly<W> {
  pub fn into_inner(self) -> W {
    self.inner
  }
}

impl<W: Write> Write for AppendOnly<W> {
  fn write(&mut self, buf: &[u8]) -> Result<usize, std::io::Error> {
    self.inner.write(buf)
  }

  fn flush(&mut self) -> std::io::Result<()> {
    self.inner.flush()
  }
}

impl<W: Write> From<W> for AppendOnly<W> {
  fn from(inner: W) -> Self {
    Self::new(inner)
  }
}

impl<W> Deref for AppendOnly<W> {
  type Target = W;

  fn deref(&self) -> &Self::Target {
    &self.inner
  }
}
