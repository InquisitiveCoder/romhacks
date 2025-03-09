use crate::mem;
use fs_err as fs;
pub use std::io::*;

/// Exports all traits and marker types used by this crate.
pub mod prelude {
  pub use super::{ReadArray, Resize};
  pub use byteorder::{ReadBytesExt, BE, LE};
  pub use std::io::prelude::*;
}

pub trait ReadArray: Read {
  fn read_array<const N: usize>(&mut self) -> Result<[u8; N]> {
    mem::try_init([0u8; N], |arr| self.read_exact(&mut arr[..]))
  }
}

impl<T: Read> ReadArray for T {}

/// File-like types that support resizing.
pub trait Resize {
  /// See [File::set_len](fs::File::set_len).
  fn set_len(&mut self, new_size: u64) -> Result<()>;
}

impl Resize for Vec<u8> {
  /// See [Vec::resize](Vec::<u8>::resize).
  ///
  /// # Errors
  /// If `new_size` doesn't fit into a [usize], the result will be
  /// [ErrorKind::InvalidInput], in keeping with [File::set_len].
  fn set_len(&mut self, new_size: u64) -> Result<()> {
    let new_size: usize = new_size
      .try_into()
      .map_err(|_| Error::from(ErrorKind::InvalidInput))?;
    self.resize(new_size, 0);
    Ok(())
  }
}

impl Resize for fs::File {
  /// See [File::set_len](fs::File::set_len).
  fn set_len(&mut self, new_size: u64) -> Result<()> {
    fs::File::set_len(self, new_size)
  }
}
