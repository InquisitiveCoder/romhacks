//! Code shared by Near's (a.k.a. Byuu) two patch formats, UPS and BPS.

use byteorder::ReadBytesExt;
use checked::Checked;
use result_result_try::try2;
use rompatcher_crc32_utils::Crc32;
use std::io;
use std::io::BufRead;
use std::ops::{Deref, DerefMut};

pub const FOOTER_LEN: usize = 3 * size_of::<u32>();

pub struct NearPatch<R>(R);

impl<R: BufRead> NearPatch<R> {
  pub fn new(reader: R) -> NearPatch<R> {
    Self(reader)
  }

  /// Reads a UPS or BPS variable-length integer.
  ///
  /// In the specification for the UPS and BPS formats, this function is
  /// called `decode`.
  ///
  /// # Errors
  /// If the value overflows, this function returns an
  /// [InvalidData](io::ErrorKind::InvalidData) error.
  pub fn read_number(&mut self) -> io::Result<Result<u64, DecodingError>> {
    let mut data: u64 = 0;
    let mut shift = Checked::<u64>::new(1);
    loop {
      let byte = self.read_u8()?;
      let new_value: u64 = try2!(
        (u64::from(byte & 0x7F) * shift + data) //
          .ok_or(DecodingError::new())
      );
      if is_msb_set(byte) {
        return Ok(Ok(new_value));
      }
      // equivalent to `shift << 7`, but multiplication will check for overflow
      shift *= 128;
      // BPS and UPS subtract 1 after encoding each byte.
      // Adding the shift after decoding each byte reverses that operation.
      data = try2!((new_value + shift).ok_or_else(DecodingError::new));
    }
  }
}

impl<T> NearPatch<T> {
  pub fn inner(&self) -> &T {
    &self.0
  }

  pub fn inner_mut(&mut self) -> &mut T {
    &mut self.0
  }
}

impl<T> Deref for NearPatch<T> {
  type Target = T;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl<T> DerefMut for NearPatch<T> {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.0
  }
}

#[derive(Debug)]
pub struct DecodingError(());

impl DecodingError {
  pub fn new() -> Self {
    DecodingError(())
  }
}

impl Default for DecodingError {
  fn default() -> Self {
    Self::new()
  }
}

fn is_msb_set(byte: u8) -> bool {
  byte & 0x80 == 0x80
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::io::Cursor;

  #[test]
  pub fn test_read_number() {
    let mut reader = NearPatch::new(Cursor::new(vec![0x0E, 0xB0, 0x80, 0x00u8]));
    let offset: u64 = reader.read_number().unwrap().unwrap();
    // Expected value obtained from the RomPatcher.js implementation.
    assert_eq!(offset, 6286);
    assert_eq!(reader.position(), 2);
  }
}

pub struct PatchReport {
  pub expected_source_crc32: Crc32,
  pub actual_source_crc32: Crc32,
  pub expected_source_size: u64,
  pub actual_source_size: u64,
  pub expected_target_crc32: Crc32,
  pub actual_target_crc32: Crc32,
  pub expected_target_size: u64,
  pub actual_target_size: u64,
  pub patch_internal_crc32: Crc32,
  pub patch_whole_file_crc32: Crc32,
}
