use crate::io;
use crate::io::prelude::*;
use checked::Checked;

pub trait ReadVarInt: Read {
  /// Reads a varint from this reader. If the value overflows, an
  /// [InvalidData](std::io::ErrorKind::InvalidData) error will be returned.
  fn read_varint(&mut self) -> Result<u64, io::Error> {
    let mut value: u64 = 0;
    let mut shift = Checked::<u64>::new(1);
    loop {
      let byte = self.read_u8()?;
      let new_value: u64 = ((byte as u64 & 0x7F) * shift + value) //
        .ok_or_else(overflow_err)?;
      if is_msb_set(byte) {
        return Ok(new_value);
      }
      // equivalent to `shift << 7`, but multiplication will check for overflow
      shift = shift * 128;
      value = (new_value + shift).ok_or_else(overflow_err)?;
    }
  }
}

impl<R> ReadVarInt for R where R: Read {}

pub fn overflow_err() -> io::Error {
  io::Error::new(io::ErrorKind::InvalidData, "varint overflowed")
}

fn is_msb_set(byte: u8) -> bool {
  byte & 0x80 == 0x80
}
