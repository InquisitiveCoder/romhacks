use crate::mem;
pub use std::io::*;

/// Exports all traits and marker types used by this crate.
pub mod prelude {
  pub use super::ReadArray;
  pub use byteorder::{ReadBytesExt, LE};
  pub use std::io::prelude::*;
}

pub trait ReadArray: Read {
  fn read_array<const N: usize>(&mut self) -> Result<[u8; N]> {
    mem::try_init([0u8; N], |arr| self.read_exact(&mut arr[..]))
  }
}

impl<T: Read> ReadArray for T {}
