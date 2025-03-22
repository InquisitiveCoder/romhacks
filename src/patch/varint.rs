use crate::io;
use crate::io::prelude::*;
use checked::Checked;

pub trait ReadByuuVarInt: Read {
  /// Reads a UPS or BPS varint. If the value overflows, returns
  /// an [InvalidData](std::io::ErrorKind::InvalidData) error.
  fn read_varint(&mut self) -> Result<u64, io::Error> {
    let mut data: u64 = 0;
    let mut shift = Checked::<u64>::new(1);
    loop {
      let byte = self.read_u8()?;
      let new_value: u64 = ((byte as u64 & 0x7F) * shift + data) //
        .ok_or_else(overflow_err)?;
      if is_msb_set(byte) {
        return Ok(new_value);
      }
      // equivalent to `shift << 7`, but multiplication will check for overflow
      shift = shift * 128;
      // BPS and UPS subtract 1 after encoding each byte.
      // Adding the shift after decoding each byte reverses that operation.
      data = (new_value + shift).ok_or_else(overflow_err)?;
    }
  }
}

impl<R> ReadByuuVarInt for R where R: Read {}

pub fn overflow_err() -> io::Error {
  io::Error::from(io::ErrorKind::InvalidData)
}

fn is_msb_set(byte: u8) -> bool {
  byte & 0x80 == 0x80
}
