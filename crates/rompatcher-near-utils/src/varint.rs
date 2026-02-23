use byteorder::ReadBytesExt;
use checked::Checked;
use result_result_try::try2;
use std::io;
use std::io::prelude::*;

pub trait ReadNumber: Read {
  /// Reads a UPS or BPS variable-length integer.
  ///
  /// In the specification for the UPS and BPS formats, this function is
  /// called `decode`.
  ///
  /// # Errors
  /// If the value overflows, this function returns an
  /// [InvalidData](io::ErrorKind::InvalidData) error.
  fn read_number(&mut self) -> io::Result<Result<u64, DecodingError>> {
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
      shift = shift * 128;
      // BPS and UPS subtract 1 after encoding each byte.
      // Adding the shift after decoding each byte reverses that operation.
      data = try2!((new_value + shift).ok_or_else(DecodingError::new));
    }
  }
}

impl<R> ReadNumber for R where R: Read {}

#[derive(Debug)]
pub struct DecodingError(());

impl DecodingError {
  pub fn new() -> Self {
    DecodingError(())
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
    let mut reader = Cursor::new(vec![0x0E, 0xB0, 0x80, 0x00u8]);
    let offset: u64 = reader.read_number().unwrap().unwrap();
    // Expected value obtained from the RomPatcher.js implementation.
    assert_eq!(offset, 6286);
    assert_eq!(reader.position(), 2);
  }
}
