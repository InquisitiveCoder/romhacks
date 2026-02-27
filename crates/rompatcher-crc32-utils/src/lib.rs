use std::hash::Hasher;

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct Crc32(u32);

impl Crc32 {
  pub fn new(value: u32) -> Self {
    Self(value)
  }

  pub fn value(&self) -> u32 {
    self.0
  }
}

impl From<u32> for Crc32 {
  fn from(value: u32) -> Self {
    Self(value)
  }
}

pub struct CRC32Hasher(crc32fast::Hasher);

impl CRC32Hasher {
  pub fn new() -> Self {
    Self(crc32fast::Hasher::new())
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

impl Hasher for CRC32Hasher {
  fn finish(&self) -> u64 {
    self.finish().value() as u64
  }

  fn write(&mut self, bytes: &[u8]) {
    self.update(bytes);
  }
}
