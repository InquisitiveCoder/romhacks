use std::io::Read;

pub struct ReadOnly<R> {
  inner: R,
}

impl<R: Read> ReadOnly<R> {
  pub fn new(inner: R) -> Self {
    Self { inner }
  }
}
