pub use super::seek::{PositionTracker, PositionTrackerReadExt};
use crate::DEFAULT_BUF_SIZE;
use std::cmp::Ordering;
use std::collections::VecDeque;
use std::io;
use std::io::ErrorKind::{Interrupted, InvalidInput, UnexpectedEof};
use std::io::{
  copy, BufRead, BufReader, BufWriter, Cursor, Empty, Error, Read, Seek, Sink, StderrLock,
  StdoutLock, Take, Write,
};

pub trait ReadExt: Read {
  /// Equivalent to [`BufReader::new(self)`].
  fn buffer_reads(self) -> BufReader<Self>
  where
    Self: Sized,
  {
    BufReader::new(self)
  }

  /// Calls [`Self::read`] repeatedly until `slice` is full or EOF is reached.
  ///
  /// This is equivalent to using [`take`](Self::take) and [`copy`], but the
  /// latter may allocate a redundant buffer if `Self`'s type isn't capable of
  /// serving as a buffer for `copy`.
  ///
  /// # Errors
  /// Like [`copy`], if `read` fails due to an [`Interrupted`] error, this
  /// function will retry the operation. If `read` returns any other error kind,
  /// this function returns it immediately.
  fn copy_to_slice(&mut self, mut slice: &mut [u8]) -> io::Result<u64> {
    let mut total: u64 = 0;
    loop {
      match self.read(&mut slice) {
        Ok(0) => return Ok(total),
        Ok(read_amount) => {
          total += read_amount as u64;
          slice = &mut slice[read_amount..];
        }
        Err(e) if e.kind() == Interrupted => {}
        Err(e) => return Err(e),
      }
    }
  }

  /// Uses [`Self::copy_to_slice`] to fill and return an array.
  ///
  /// # Errors
  /// Returns [`UnexpectedEof`] if the array couldn't be filled.
  fn read_array<const N: usize>(&mut self) -> io::Result<[u8; N]> {
    let mut arr = [0u8; N];
    self
      .take(N as u64)
      .exactly(|reader| reader.copy_to_slice(&mut arr[..]))
      .map(|_| arr)
  }
}
impl<R: Read> ReadExt for R {}

pub trait BufReadExt: BufRead {
  /// Checks if `self` has reached EOF.
  ///
  /// Equivalent to `self.fill_buf()?.is_empty()`
  ///
  /// # Errors
  /// This function returns any errors from [`Self::fill_buf()`].
  fn has_reached_eof(&mut self) -> io::Result<bool> {
    Ok(self.fill_buf()?.is_empty())
  }

  /// Performs an I/O operation iff `self` hasn't reached EOF.
  ///
  /// If `self` is at EOF, returns `Ok(None)`;
  /// otherwise, returns `Ok(Some(f(self)?))`.
  ///
  /// # Errors
  /// This function returns any errors from [`Self::fill_buf()`] or `f`.
  fn optionally<R>(&mut self, f: impl FnOnce(&mut Self) -> io::Result<R>) -> io::Result<Option<R>> {
    if self.has_reached_eof()? {
      return Ok(None);
    }
    Ok(Some(f(self)?))
  }

  /// Looks ahead by `amount` bytes.
  ///
  /// This function attempts to copy up to `amount` bytes to `writer`, then does
  /// a [`seek_relative`][1] by the number of bytes copied to return `self` to
  /// its original position.
  ///
  /// This function is intended for relatively small lookahead amounts, such
  /// that the [`copy`][2] and seek are likely to fall within the reader's internal
  /// buffer. Additionally, in many use cases the lookahead amount is derived
  /// from the length of a buffer or the size of the data type at the end
  /// of the byte stream. For these reasons, `usize` was chosen as the parameter
  /// and return type over `u64`.
  ///
  /// # Errors
  /// This function returns [`InvalidInput`] if `amount` can't be converted to
  /// an `i64`. Otherwise, see [`std::io::copy`] and [`Seek::seek_relative`].
  ///
  /// [1]: Seek::seek_relative
  /// [2]: std::io::copy
  fn look_ahead(&mut self, amount: usize, writer: &mut impl Write) -> io::Result<usize>
  where
    Self: Seek,
  {
    let amount: i64 = i64::try_from(amount).map_err(|_| InvalidInput)?;
    // The cast to u64 is safe since amount started out as an unsigned type.
    let bytes_read = copy(&mut self.take(amount as u64), writer)?;
    self.seek_relative(-amount)?;
    // This cast is also safe since bytes_read <= amount.
    Ok(bytes_read as usize)
  }

  /// Uses [`look_ahead`][1] to check if the number of remaining bytes are less
  /// than, equal to or greater than `amount`.
  ///
  /// [1]: BufReadExt::look_ahead
  fn cmp_remaining_len(&mut self, amount: usize) -> io::Result<Ordering>
  where
    Self: Seek,
  {
    self
      .look_ahead(amount + 1, &mut io::sink())
      .map(|bytes| bytes.cmp(&amount))
  }
}
impl<R: BufRead> BufReadExt for R {}

pub trait TakeExt {
  /// Performs an I/O operation that reads exactly [`Take::limit`] bytes.
  ///
  /// # Errors
  /// Returns [`UnexpectedEof`] if `self.limit() > 0` after `f` is called.
  fn exactly<R>(&mut self, f: impl FnOnce(&mut Self) -> io::Result<R>) -> io::Result<R>;
}

impl<I> TakeExt for Take<I> {
  fn exactly<R>(&mut self, f: impl FnOnce(&mut Self) -> io::Result<R>) -> io::Result<R> {
    let result = f(self)?;
    if self.limit() > 0 {
      return Err(Error::from(UnexpectedEof));
    }
    Ok(result)
  }
}

pub trait CopyTo<W>: Read
where
  W: Write,
{
  fn copy_to(&mut self, writer: &mut W) -> io::Result<u64>;
}

pub trait WriteExt: Write {
  /// Wraps `self` in a `BufWriter` with 1.5 times the default buffer size.
  ///
  /// This works around a bug in [`std::io::copy`] that flushes the `BufWriter` if its
  /// remaining capacity after a `write` falls below the default buffer size.
  fn buffer_writes(self) -> BufWriter<Self>
  where
    Self: Sized,
  {
    BufWriter::with_capacity(DEFAULT_BUF_SIZE * 3 / 2, self)
  }
}

/// File-like types that support resizing.
pub trait Resize {
  /// See [File::set_len](std::fs::File::set_len).
  fn set_len(&mut self, new_size: u64) -> io::Result<()>;
}

impl Resize for Vec<u8> {
  /// See [Vec::resize](Vec::<u8>::resize).
  ///
  /// # Errors
  /// If `new_size` doesn't fit into a [usize], the result will be
  /// [InvalidInput], in keeping with [File::set_len()][1].
  ///
  /// [1]: std::fs::File::set_len
  fn set_len(&mut self, new_size: u64) -> io::Result<()> {
    let new_size: usize = new_size.try_into().map_err(|_| Error::from(InvalidInput))?;
    self.resize(new_size, 0);
    Ok(())
  }
}

#[cfg(feature = "fs-err")]
use fs_err as fs;

#[cfg(feature = "fs-err")]
impl Resize for fs::File {
  /// Equivalent to [File::set_len](fs::File::set_len).
  fn set_len(&mut self, new_size: u64) -> std::io::Result<()> {
    fs::File::set_len(self, new_size)
  }
}

/// Writers that either have an internal buffer or don't perform I/O.
///
/// The presence of this trait indicates that a writer is suitable for frequent
/// small writes without significant performance overhead due to system calls
/// or acquiring a lock.
pub trait BufWrite: Write {
  type Inner: Write + ?Sized;

  fn get_ref(&self) -> &Self::Inner;

  fn get_mut(&mut self) -> &mut Self::Inner;
}

macro_rules! trivial_buf_write {
  ($type_name:ty) => {
    type Inner = $type_name;

    fn get_ref(&self) -> &Self::Inner {
      self
    }

    fn get_mut(&mut self) -> &mut Self::Inner {
      self
    }
  };
}

impl BufWrite for &mut [u8] {
  trivial_buf_write! { Self }
}

impl BufWrite for Cursor<&mut [u8]> {
  trivial_buf_write! { Self }
}
impl BufWrite for Empty {
  trivial_buf_write! { Self }
}
impl BufWrite for Sink {
  trivial_buf_write! { Self }
}
impl<'a> BufWrite for StderrLock<'a> {
  trivial_buf_write! { Self }
}

impl<'a> BufWrite for StdoutLock<'a> {
  trivial_buf_write! { Self }
}

impl BufWrite for Cursor<&mut Vec<u8>> {
  trivial_buf_write! { Self}
}

impl BufWrite for Cursor<Box<[u8]>> {
  trivial_buf_write! { Self }
}

impl BufWrite for Cursor<Vec<u8>> {
  trivial_buf_write! { Self }
}

impl BufWrite for VecDeque<u8> {
  trivial_buf_write! { Self}
}

impl BufWrite for Vec<u8> {
  trivial_buf_write! { Self }
}

impl<W: Write> BufWrite for BufWriter<W> {
  /// The underlying writer. May be `Self` if the implementing type is an
  /// in-memory buffer, such as [`Cursor<Vec<u8>>`].
  type Inner = W;

  /// See [`BufWriter::get_ref`].
  fn get_ref(&self) -> &Self::Inner {
    BufWriter::get_ref(self)
  }

  /// See [`BufWriter::get_mut`].
  fn get_mut(&mut self) -> &mut Self::Inner {
    BufWriter::get_mut(self)
  }
}

impl<const N: usize> BufWrite for Cursor<[u8; N]> {
  trivial_buf_write! { Self }
}

impl<W: BufWrite> BufWrite for Box<W> {
  trivial_buf_write! { Self }
}

impl<W: BufWrite> BufWrite for &mut W {
  type Inner = W::Inner;

  fn get_ref(&self) -> &Self::Inner {
    <W as BufWrite>::get_ref(self)
  }

  fn get_mut(&mut self) -> &mut Self::Inner {
    <W as BufWrite>::get_mut(self)
  }
}
