pub use super::seek::{PositionTracker, PositionTrackerReadExt};
use crate::DEFAULT_BUF_SIZE;
use std::cmp::Ordering;
use std::collections::VecDeque;
use std::io;
use std::io::prelude::*;
use std::io::ErrorKind::{Interrupted, InvalidInput, UnexpectedEof};
use std::io::{copy, BufWriter, Cursor, Empty, Error, Sink, StderrLock, StdoutLock, Take};

pub trait ReadExt: Read {
  fn copy_to(&mut self, writer: &mut impl Write) -> io::Result<u64> {
    copy(self, writer)
  }

  /// Calls [`read`][1] repeatedly until `slice` is full or EOF is reached.
  ///
  /// This is equivalent to using [`take`](Self::take) and [`copy`], but the
  /// latter may allocate a redundant buffer if `self`'s type isn't capable of
  /// serving as a buffer for `copy`.
  ///
  /// # Errors
  /// Like [`copy`], if `read` fails due to an [`Interrupted`] error, this
  /// function will retry the operation. If `read` returns any other error kind,
  /// this function returns it immediately.
  ///
  /// # Examples
  /// The code below demonstrates the function's behavior when there isn't
  /// enough data in the reader to fill the buffer.
  /// ```
  /// use std::io::prelude::*;
  /// use std::io::{BufReader, Cursor};
  /// use read_write_utils::prelude::*;
  ///
  /// let mut vec_cursor = Cursor::new(vec![2u8, 3, 5, 7, 11]);
  /// let mut buffer = [13u8; 6];
  ///
  /// let bytes_copied = vec_cursor.copy_to_slice(&mut buffer[..]);
  ///
  /// // The return value is the number of bytes in the vector.
  /// assert_eq!(
  ///    bytes_copied.unwrap() as usize,
  ///    vec_cursor.get_ref().len()
  /// );
  ///
  /// // The first 5 indexes of the buffer have been overwritten,
  /// assert_eq!(&buffer[..], &[2, 3, 5, 7, 11, 13]);
  ///
  /// // The cursor is at the end of the vector.
  /// assert_eq!(
  ///    vec_cursor.position() as usize,
  ///    vec_cursor.get_ref().len()
  /// );
  /// ```
  ///
  /// The code below demonstrates filling a buffer.
  /// ```
  /// use std::io::prelude::*;
  /// use std::io::{BufReader, Cursor};
  /// use read_write_utils::prelude::*;
  ///
  /// // Because the vector is larger than the BufReader's capacity,
  /// // multiple reads will be required to copy the vector.
  /// let mut vec_cursor = Cursor::new(vec![2u8, 3, 5]);
  /// let mut buffer = [0u8; 2];
  ///
  /// let bytes_copied = vec_cursor.copy_to_slice(&mut buffer[..]);
  ///
  /// // The return value is the size of the buffer.
  /// assert_eq!(
  ///    bytes_copied.unwrap() as usize,
  ///    buffer.len()
  /// );
  ///
  /// // The buffer matches the first two bytes of the vector.
  /// assert_eq!(&buffer[..], &(vec_cursor.get_ref())[..buffer.len()]);
  ///
  /// // The cursor position matches the length of the buffer.
  /// assert_eq!(
  ///    vec_cursor.position() as usize,
  ///    buffer.len()
  /// );
  /// ```
  ///
  /// [1]: Read::read
  fn copy_to_slice(&mut self, mut slice: &mut [u8]) -> io::Result<u64> {
    let mut total: u64 = 0;
    loop {
      match self.read(&mut slice) {
        Ok(0) => return Ok(total),
        Ok(read_amount) => {
          total = u64::try_from(read_amount)
            .ok()
            .and_then(|read_amount| u64::checked_add(total, read_amount))
            .expect("copy_to_slice result overflowed");
          slice = &mut slice[read_amount..];
        }
        Err(e) if e.kind() == Interrupted => {}
        Err(e) => return Err(e),
      }
    }
  }

  /// Uses [`copy_to_slice`][1] to fill and return an array of length `N`.
  ///
  /// # Errors
  /// In addition to any errors returned by [`copy_to_slice`][1], this function
  /// returns [`UnexpectedEof`] if there aren't enough bytes left in the reader
  /// to fill the array.
  ///
  /// # Examples
  /// ```
  /// use std::io::Cursor;
  /// use std::io::ErrorKind::UnexpectedEof;
  /// use std::io::prelude::*;
  /// use read_write_utils::prelude::*;
  ///
  /// let mut reader = Cursor::new(vec![1u8, 2, 3, 4, 5]);
  /// // Successful read.
  /// assert_eq!(reader.read_array::<3>().unwrap(), [1u8, 2, 3]);
  /// // Not enough bytes left.
  /// let err = reader.read_array::<3>();
  /// assert_eq!(err.err().unwrap().kind(), UnexpectedEof);
  /// ```
  ///
  /// [1]: ReadExt::copy_to_slice
  fn read_array<const N: usize>(&mut self) -> io::Result<[u8; N]> {
    let mut arr = [0u8; N];
    self
      .take(N as u64)
      .exactly(|reader| reader.copy_to_slice(&mut arr[..]))
      .map(|_| arr)
  }
}
impl<R: Read> ReadExt for R {}

#[cfg(test)]
mod test {
  use super::*;
  use std::io::BufReader;

  #[test]
  fn copy_to_slice_multiple_reads() {
    let mut cursor = BufReader::with_capacity(2, Cursor::new(vec![1u8, 2, 3, 4, 5]));
    let mut buf = [0u8; 5];
    let bytes_copied = cursor.copy_to_slice(&mut buf).unwrap();
    assert_eq!(bytes_copied as usize, buf.len());
    assert_eq!(cursor.get_ref().get_ref().as_slice(), &buf[..])
  }
}

pub trait BufReadExt: BufRead {
  /// Checks if `self` has reached EOF.
  ///
  /// Equivalent to `self.fill_buf()?.is_empty()`
  ///
  /// # Errors
  /// This function returns any errors from [`Self::fill_buf()`].
  ///
  /// # Examples
  /// ```
  /// use std::io::prelude::*;
  /// use std::io::Cursor;
  /// use read_write_utils::prelude::*;
  ///
  /// let mut cursor = Cursor::new(vec![0u8; 3]);
  /// assert!(!cursor.has_reached_eof().unwrap());
  /// cursor.set_position(3);
  /// assert!(cursor.has_reached_eof().unwrap());
  /// ```
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
  /// that the [`copy`][2] and seek are likely to fall within the reader's
  /// internal buffer. Additionally, in many use cases the lookahead amount is
  /// derived from the length of a buffer or the size of a footer structure at
  /// the end of the byte stream. For these reasons, `usize` was chosen as the
  /// parameter and return type over `u64`.
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
    // The cast to u64 is safe since 0 <= amount <= i64::MAX < u64::MAX.
    let bytes_read = copy(&mut self.take(amount as u64), writer)?;
    self.seek_relative(-amount)?;
    // This cast is also safe since bytes_read <= amount.
    Ok(bytes_read as usize)
  }

  /// Uses [`look_ahead`][1] to compare the number of remaining bytes to
  /// `amount`.
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
  /// Performs an I/O operation that reads exactly [`self.limit()`][1] bytes.
  ///
  /// # Errors
  /// If `f` fails, the error will be returned. If `f` succeeds but
  /// `self.limit() > 0` afterward, [`UnexpectedEof`] will be returned.
  ///
  /// [1]: Take::limit
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

/// Writers that have an internal buffer or don't perform I/O.
///
/// This trait indicates that a writer is suitable for small and repeated
/// writes, as explained in the documentation for [`BufWriter`].
pub trait BufWrite: Write {
  /// The type of the underlying writer, if any. May be `Self` for writers that
  /// don't perform I/O, such as [`Cursor`] or [`Sink`].
  type Inner: Write + ?Sized;

  /// Returns a reference to the underlying writer.
  fn inner(&self) -> &Self::Inner;

  /// Returns a mutable reference to the underlying writer.
  ///
  /// This method is intended for cases where the inner writer has capabilities
  /// that `self` doesn't (e.g. [`Read`].) Writing directly to the inner writer
  /// without calling [`flush`][1] is likely to result in the data being
  /// written in an unintended order.
  fn inner_mut(&mut self) -> &mut Self::Inner;
}

macro_rules! trivial_buf_write {
  ($type_name:ty) => {
    type Inner = $type_name;

    fn inner(&self) -> &Self::Inner {
      self
    }

    fn inner_mut(&mut self) -> &mut Self::Inner {
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
  fn inner(&self) -> &Self::Inner {
    BufWriter::get_ref(self)
  }

  /// See [`BufWriter::get_mut`].
  fn inner_mut(&mut self) -> &mut Self::Inner {
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

  fn inner(&self) -> &Self::Inner {
    <W as BufWrite>::inner(self)
  }

  fn inner_mut(&mut self) -> &mut Self::Inner {
    <W as BufWrite>::inner_mut(self)
  }
}
