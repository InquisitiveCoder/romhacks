use ErrorKind::{Interrupted, UnexpectedEof};
use std::io::*;

mod repeat_slice;

pub use repeat_slice::*;
use std::collections::VecDeque;

mod eof;
pub use eof::*;

mod append;
mod hash;
pub use hash::*;

mod position;
pub use position::*;

mod ro;

pub mod copy;

use fs_err as fs;
use num_traits::ToPrimitive;

/// Exports all traits and [`PositionTracker`].
pub mod prelude {
  pub use super::{
    BufReadExt, BufWrite, KnownEOF, PositionTracker, ReadExt, Resize, TakeExt, copy::CopyReadBuf,
    copy::CopyWriteBuf,
  };
  pub use byteorder::{BE, LE, ReadBytesExt};
}
pub use prelude::*;

/// The buffer size constant used internally by `std::io_utils` since Rust 1.9.0,
/// copied verbatim.
pub const DEFAULT_BUF_SIZE: usize = if cfg!(target_os = "espidf") { 512 } else { 8 * 1024 };

pub trait ReadExt: Read {
  /// Consumes `self` and returns [`BufReader::new(self)`].
  fn buffer_reads(self) -> BufReader<Self>
  where
    Self: Sized,
  {
    BufReader::new(self)
  }

  /// Calls [`Self::read`] repeatedly until `slice` is full or EOF is reached.
  ///
  /// This is equivalent to using [`take`](Self::take) and [`copy`] but the
  /// latter may allocate a redundant buffer if `Self` doesn't implement
  /// [`CopyReadBuf`].
  ///
  /// # Errors
  /// Like [`copy`], if `read` fails due to an [`Interrupted`] error, this
  /// function will retry the operation. If `read` returns any other error kind,
  /// this function returns it immediately.
  fn copy_to_slice(&mut self, mut slice: &mut [u8]) -> Result<u64> {
    let mut copied: u64 = 0;
    loop {
      match self.read(&mut slice) {
        Ok(0) => return Ok(copied),
        Ok(amount) => {
          copied += amount as u64;
          slice = &mut slice[amount..];
        }
        Err(e) if e.kind() == Interrupted => continue,
        Err(e) => return Err(e),
      }
    }
  }

  /// Uses [`Self::copy_to_slice`] to fill and return an array.
  ///
  /// # Errors
  /// Returns [`UnexpectedEof`] if the array couldn't be filled.
  fn read_array<const N: usize>(&mut self) -> Result<[u8; N]> {
    self.take(N as u64).exact(|reader| {
      let mut arr = [0u8; N];
      reader.copy_to_slice(&mut arr[..]).map(|_| arr)
    })
  }
}
impl<R: Read> ReadExt for R {}

pub trait BufReadExt: BufRead {
  /// Optionally performs some operation.
  ///
  /// If `self` is at EOF, returns `Ok(None)`. Otherwise, returns
  /// `Ok(Some(f(self)?))`
  fn optionally<R>(&mut self, f: impl FnOnce(&mut Self) -> Result<R>) -> Result<Option<R>> {
    if self.fill_buf()?.is_empty() {
      return Ok(None);
    }
    Ok(Some(f(self)?))
  }

  /// Equivalent to `copy(self, writer)` but guarantees that the reader's type
  /// is capable of lending its internal buffer to [`copy`].
  fn copy_to(&mut self, writer: &mut impl Write) -> Result<u64> {
    copy(self, writer)
  }

  /// Looks ahead by `amount` bytes.
  ///
  /// This function attempts to copy up to `amount` bytes to `writer`, then does
  /// a [`seek_relative`][1] by the number of bytes copied to return `self` back
  /// to its original position.
  ///
  /// This function is intended for relatively small lookahead amounts, such
  /// that the copy and seek are likely to fall within the reader's internal
  /// buffer. Additionally, in many use cases the lookahead amount is derived
  /// from the length of a buffer or the size of the type of the data at the end
  /// of the byte stream. For these reasons, `usize` was chosen as the parameter
  /// and return type over `u64`.
  ///
  /// # Panics
  /// This function panics if `amount` doesn't fit in an i64.
  ///
  /// # Errors
  /// See [`copy`] and [`Seek::seek_relative`].
  ///
  /// [1]: Seek::seek_relative
  fn look_ahead(&mut self, amount: usize, writer: &mut impl Write) -> Result<usize>
  where
    Self: Seek,
  {
    let bytes_read = self.take(amount.to_u64().unwrap()).consuming_copy(writer)?;
    self.seek_relative(-amount.to_i64().unwrap())?;
    Ok(bytes_read as usize)
  }
}
impl<R: BufRead> BufReadExt for R {}

pub trait TakeExt<I> {
  fn exact<R>(self, f: impl FnOnce(&mut Self) -> Result<R>) -> Result<R>;
}
impl<I> TakeExt<I> for Take<I> {
  fn exact<R>(mut self, f: impl FnOnce(&mut Self) -> Result<R>) -> Result<R> {
    let result = f(&mut self)?;
    if self.limit() > 0 {
      return Err(Error::from(UnexpectedEof));
    }
    Ok(result)
  }
}

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

/// Writers that either have an internal buffer or don't perform I/O.
///
/// The presence of this trait indicates that a writer is suitable for frequent
/// small writes without significant performance overhead due to system calls
/// or acquiring a lock.
pub trait BufWrite: Write {}
impl BufWrite for &Empty {}
impl BufWrite for &Sink {}
impl BufWrite for &mut [u8] {}
impl BufWrite for Cursor<&mut [u8]> {}
impl BufWrite for Empty {}
impl BufWrite for Sink {}
impl<'a> BufWrite for StderrLock<'a> {}
impl<'a> BufWrite for StdoutLock<'a> {}
impl BufWrite for Cursor<&mut Vec<u8>> {}
impl BufWrite for Cursor<Box<[u8]>> {}
impl BufWrite for Cursor<Vec<u8>> {}
impl BufWrite for VecDeque<u8> {}
impl BufWrite for Vec<u8> {}
impl<W: Write> BufWrite for BufWriter<W> {}
impl<const N: usize> BufWrite for Cursor<[u8; N]> {}
impl<W: BufWrite> BufWrite for &mut W {}
impl<W: BufWrite> BufWrite for Box<W> {}
