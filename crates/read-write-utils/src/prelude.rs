pub use super::pos::{PositionTracker, PositionTrackerReadExt};
pub use crate::next_bytes_eq;
use crate::DEFAULT_BUF_SIZE;
use polonius_the_crab::prelude::*;
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
  /// This is equivalent to using [`take`][2] and [`copy`], but the
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
  /// [2]: Take::take
  fn copy_to_slice(&mut self, mut slice: &mut [u8]) -> io::Result<u64> {
    let mut total: u64 = 0;
    loop {
      match self.read(slice) {
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
    let limit = u64::try_from(N).map_err(|_| InvalidInput)?;
    let mut arr = [0u8; N];
    self
      .take(limit)
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
  /// This function returns any errors from [`fill_buf`][1].
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
  ///
  /// [1]: BufRead::fill_buf
  fn has_reached_eof(&mut self) -> io::Result<bool> {
    Ok(self.fill_buf()?.is_empty())
  }

  /// Performs an I/O operation iff `self` hasn't reached EOF.
  ///
  /// If `self` is at EOF, returns `Ok(None)`;
  /// otherwise, returns `Ok(Some(f(self)?))`.
  ///
  /// # Errors
  /// This function returns any errors from [`fill_buf`][1] or `f`.
  ///
  /// [1]: BufRead::fill_buf
  fn optionally<R>(&mut self, f: impl FnOnce(&mut Self) -> io::Result<R>) -> io::Result<Option<R>> {
    if self.has_reached_eof()? {
      return Ok(None);
    }
    Ok(Some(f(self)?))
  }

  /// Peeks at the next `buf.len()` bytes in the reader. The returned slice's
  /// length can be smaller if EOF is reached.
  ///
  /// This method first attempts to return `buf.len()` bytes from `self`'s
  /// [internal buffer][1]. Otherwise, [`self.copy_to_slice(buf)`][2] will be
  /// called, followed by a backwards [`seek_relative`][3] to return `self` to
  /// its former position.
  ///
  /// For peeks much smaller than the size of the reader's internal buffer,
  /// there's a high probability that the copy and seek are avoided.
  ///
  /// The [`next_bytes_eq!`] macro provides a convenient way to call `peek`
  /// and compare the result to a `const` slice.
  ///
  /// # Errors
  /// This function returns [`InvalidInput`] if `amount` can't be converted to
  /// an `i64`. Otherwise, see [`std::io::copy`] and [`seek_relative`][3].
  ///
  /// # Examples
  /// ```
  /// use std::io::{Cursor, SeekFrom};
  /// use std::io::prelude::*;
  /// use read_write_utils::prelude::*;
  ///
  /// let mut reader = Cursor::new([0u8, 1, 2, 3, 4, 5, 6, 7]);
  /// let mut buf = [0u8; 3];
  /// assert_eq!(reader.peek(&mut buf[..]).unwrap(), &[0u8, 1, 2]);
  /// reader.set_position(6);
  /// assert_eq!(reader.peek(&mut buf[..]).unwrap(), &[6u8, 7]);
  /// ```
  ///
  /// [1]: BufRead::fill_buf
  /// [2]: ReadExt::copy_to_slice
  /// [3]: Seek::seek_relative
  /// [4]: next_bytes_eq
  fn peek<'a>(&'a mut self, buf: &'a mut [u8]) -> io::Result<&'a [u8]>
  where
    Self: Seek,
  {
    i64::try_from(buf.len()).map_err(|_| InvalidInput)?;
    // the polonius macro seems to misbehave when its pseudo-parameter is self.
    let mut reader = self;
    polonius!(|reader| -> Result<&'polonius [u8], io::Error> {
      let inner_buf = polonius_try!(reader.fill_buf());
      if let Some(slice) = inner_buf.get(0..buf.len()) {
        polonius_return!(Ok(slice));
      }
    });
    let copy_amt = reader.copy_to_slice(buf)?;
    // copy_amt <= buf.len() <= (i64::MAX and usize::MAX)
    reader.seek_relative(-(copy_amt as i64))?;
    Ok(&buf[0..copy_amt as usize])
  }

  /// [Compares][1] the remaining number of bytes in the reader to `amt`.
  ///
  /// This method first compares the length of the slice returned by
  /// [`fill_buf`][2]; if it's greater than `amt`, that result is returned.
  /// Otherwise, [up to][3] `amt + 1` bytes are [copied][4] to a [`Sink`],
  /// followed by a backwards [`seek_relative`][5] to return `self` to its
  /// former position. The number of bytes copied is then compared to `amt`.
  ///
  /// If `amt` is much smaller than the size of the reader's internal buffer,
  /// there's a high probability that the copy and seek are avoided.
  ///
  /// # Errors
  /// If `amt + 1` can't be converted to an `i64`, this method returns
  /// [`InvalidInput`]. Otherwise, see [`std::io::copy`] and
  /// [`seek_relative`][5].
  ///
  /// [1]: Ord::cmp
  /// [2]: BufRead::fill_buf
  /// [3]: Take::take
  /// [4]: io::copy
  /// [5]: Seek::seek_relative
  fn peek_len(&mut self, amt: usize) -> io::Result<Ordering>
  where
    Self: Seek,
  {
    use Ordering::Greater;
    let take_limit: i64 = i64::try_from(amt)
      .ok()
      .and_then(|x| i64::checked_add(x, 1))
      .ok_or(InvalidInput)?;
    if let Greater = self.fill_buf()?.len().cmp(&amt) {
      return Ok(Greater);
    }
    // copy_amt <= amt + 1 <= i64::MAX < u64::MAX
    let copy_amt = io::copy(&mut self.take(take_limit as u64), &mut io::sink())?;
    self.seek_relative(-(copy_amt as i64))?;
    Ok(copy_amt.cmp(&(amt as u64)))
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
pub trait BufWrite: Write {}

/// [`BufWrite`] implementations that support reading from their underlying
/// stream.
pub trait AsRead: BufWrite {
  /// Returns a mutable reference to the underlying writer.
  ///
  /// This method is intended for cases where the inner writer has capabilities
  /// that `self` doesn't (e.g. [`Read`].) Writing directly to the inner writer
  /// without calling [`flush`][1] is likely to result in the data being
  /// written in an unintended order.
  ///
  /// [1]: Write::flush
  fn as_read(&mut self) -> io::Result<&mut dyn Read>;
}

impl BufWrite for &mut [u8] {}

impl BufWrite for Cursor<&mut [u8]> {}
impl AsRead for Cursor<&mut [u8]> {
  fn as_read(&mut self) -> io::Result<&mut dyn Read> {
    self.flush()?;
    Ok(self)
  }
}

impl BufWrite for Empty {}
impl BufWrite for Sink {}
impl BufWrite for StderrLock<'_> {}
impl BufWrite for StdoutLock<'_> {}

impl BufWrite for Cursor<&mut Vec<u8>> {}
impl AsRead for Cursor<&mut Vec<u8>> {
  fn as_read(&mut self) -> io::Result<&mut dyn Read> {
    self.flush()?;
    Ok(self)
  }
}

impl BufWrite for Cursor<Box<[u8]>> {}
impl AsRead for Cursor<Box<[u8]>> {
  fn as_read(&mut self) -> io::Result<&mut dyn Read> {
    self.flush()?;
    Ok(self)
  }
}

impl BufWrite for Cursor<Vec<u8>> {}
impl AsRead for Cursor<Vec<u8>> {
  fn as_read(&mut self) -> io::Result<&mut dyn Read> {
    self.flush()?;
    Ok(self)
  }
}

impl BufWrite for VecDeque<u8> {}
impl AsRead for VecDeque<u8> {
  fn as_read(&mut self) -> io::Result<&mut dyn Read> {
    self.flush()?;
    Ok(self)
  }
}

impl BufWrite for Vec<u8> {}

impl<W: Write> BufWrite for BufWriter<W> {}

impl<I: Read + Write> AsRead for BufWriter<I> {
  fn as_read(&mut self) -> io::Result<&mut dyn Read> {
    self.flush()?;
    Ok(self.get_mut())
  }
}

impl<const N: usize> BufWrite for Cursor<[u8; N]> {}
impl<const N: usize> AsRead for Cursor<[u8; N]> {
  fn as_read(&mut self) -> io::Result<&mut dyn Read> {
    self.flush()?;
    Ok(self)
  }
}

impl<W: BufWrite> BufWrite for Box<W> {}
impl<W: AsRead> AsRead for Box<W> {
  fn as_read(&mut self) -> io::Result<&mut dyn Read> {
    self.flush()?;
    self.as_mut().as_read()
  }
}

impl<W: BufWrite> BufWrite for &mut W {}
impl<W: AsRead> AsRead for &mut W {
  fn as_read(&mut self) -> io::Result<&mut dyn Read> {
    self.flush()?;
    <W as AsRead>::as_read(self)
  }
}

pub trait CursorExt {
  fn slice_until_pos(&self) -> &[u8];
}

impl<T: AsRef<[u8]>> CursorExt for Cursor<T> {
  fn slice_until_pos(&self) -> &[u8] {
    let position = usize::try_from(self.position()).unwrap();
    &self.get_ref().as_ref()[..position]
  }
}

#[cfg(test)]
mod test {
  use super::*;
  use std::io::BufReader;

  #[test]
  fn copy_to_slice_multiple_reads() -> io::Result<()> {
    let mut cursor = BufReader::with_capacity(2, Cursor::new(vec![1u8, 2, 3, 4, 5]));
    let mut buf = [0u8; 5];
    let bytes_copied = cursor.copy_to_slice(&mut buf)?;
    assert_eq!(bytes_copied as usize, buf.len());
    assert_eq!(cursor.get_ref().get_ref().as_slice(), &buf[..]);
    Ok(())
  }

  #[test]
  fn test_cursor_as_slice() {
    let mut cursor = Cursor::new(vec![1u8, 2, 3, 4, 5]);
    cursor.set_position(3);
    assert_eq!(cursor.slice_until_pos(), &[1u8, 2, 3]);
  }

  #[test]
  fn peek_full_read() -> io::Result<()> {
    let mut reader = Cursor::new([0u8, 1, 2, 3, 4]);
    let mut buf = [0u8; 3];
    reader.set_position(1);
    assert_eq!(reader.peek(&mut buf)?, &[1, 2, 3]);
    assert_eq!(reader.position(), 1);
    Ok(())
  }

  #[test]
  fn peek_eof() -> io::Result<()> {
    let mut reader = Cursor::new([0u8, 1, 2, 3, 4]);
    let mut buf = [0u8; 3];
    reader.set_position(3);
    assert_eq!(reader.peek(&mut buf[..])?, &[3, 4]);
    assert_eq!(reader.position(), 3);
    Ok(())
  }
}
