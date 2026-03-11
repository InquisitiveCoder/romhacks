use polonius_the_crab::prelude::*;
use std::cmp::Ordering;
use std::io;
use std::io::prelude::*;
use std::io::ErrorKind::{Interrupted, InvalidInput, UnexpectedEof};

pub trait ReadExt: Read {
  /// Equivalent to `io::copy(self, writer)`. Useful if you need to call
  /// [`io::copy`] at the end of a method chain.
  fn copy_to(&mut self, writer: &mut impl Write) -> io::Result<u64> {
    io::copy(self, writer)
  }

  /// Calls [`read`][1] until `slice` is full or EOF is reached.
  ///
  /// This is equivalent to using [`take`][2] and [`io::copy`], except it won't
  /// allocate an additional buffer and perform redundant copies. It also
  /// differs from [`read_exact`][3] in that it won't return [`UnexpectedEof`].
  ///
  /// # Errors
  /// Like [`io::copy`], if [`read`][1] fails due to an [`Interrupted`] error,
  /// this function will retry the operation. If [`io::read`] returns any other
  /// error kind, this function returns it immediately.
  ///
  /// # Examples
  /// The code below demonstrates the function's behavior when there aren't
  /// enough bytes left in the reader to fill the buffer.
  /// ```
  /// use std::io::Cursor;
  /// use std::io::prelude::*;
  /// use read_write_utils::prelude::*;
  ///
  /// let mut reader = Cursor::new(vec![1, 2, 3]);
  /// let mut buffer = [0u8; 5];
  ///
  /// // All bytes were copied.
  /// assert_eq!(
  ///    reader.copy_to_slice(&mut buffer[..])?,
  ///    reader.get_ref().len() as u64
  /// );
  /// assert!(reader.reached_eof()?);
  ///
  /// // The first 3 indexes of the buffer have been overwritten.
  /// assert_eq!(&buffer[..], &[1, 2, 3, 0, 0]);
  ///
  /// # Ok::<(), std::io::Error>(())
  /// ```
  ///
  /// The code below demonstrates filling a buffer.
  /// ```
  /// use std::io::prelude::*;
  /// use std::io::{BufReader, Cursor};
  /// use read_write_utils::prelude::*;
  ///
  /// let mut reader = Cursor::new(vec![1, 2, 3u8]);
  /// let mut buffer = [0u8; 2];
  ///
  /// // The number of bytes copied is the size of the buffer.
  /// assert_eq!(
  ///   reader.copy_to_slice(&mut buffer[..])?,
  ///   buffer.len() as u64
  /// );
  ///
  /// // The buffer matches the first two bytes of the vector.
  /// assert_eq!(&buffer[..], &[1, 2]);
  ///
  /// // The cursor position matches the length of the buffer.
  /// assert_eq!(reader.position(), buffer.len() as u64);
  /// # Ok::<(), std::io::Error>(())
  /// ```
  ///
  /// [1]: Read::read
  /// [2]: io::Take::take
  /// [3]: Read::read_exact
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
  /// This method provides a stable alternative to [`read_array`][2], which is
  /// still nightly-only as of Rust 1.94.0.
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
  ///
  /// // Successful read.
  /// assert_eq!(reader.read_n::<3>()?, [1u8, 2, 3]);
  ///
  /// // Not enough bytes left.
  /// assert_eq!(reader.position(), 3);
  /// assert!(reader.read_n::<3>().is_err_and(|x| x.kind() == UnexpectedEof));
  /// # Ok::<(), std::io::Error>(())
  /// ```
  ///
  /// [1]: ReadExt::copy_to_slice
  /// [2]: Read::read_array
  fn read_n<const N: usize>(&mut self) -> io::Result<[u8; N]> {
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
  /// assert!(!cursor.reached_eof()?);
  /// cursor.set_position(3);
  /// assert!(cursor.reached_eof()?);
  /// # Ok::<(), std::io::Error>(())
  /// ```
  ///
  /// [1]: BufRead::fill_buf
  fn reached_eof(&mut self) -> io::Result<bool> {
    Ok(self.fill_buf()?.is_empty())
  }

  /// Performs an I/O operation if and only if `self` hasn't reached EOF.
  ///
  /// If `self` is at EOF, returns `Ok(None)`;
  /// otherwise, returns `Ok(Some(f(self)?))`.
  ///
  /// # Errors
  /// This function returns any errors from [`fill_buf`][1] or `f`.
  ///
  /// [1]: BufRead::fill_buf
  fn optionally<R>(&mut self, f: impl FnOnce(&mut Self) -> io::Result<R>) -> io::Result<Option<R>> {
    if self.reached_eof()? {
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
  /// The [`peek_eq!`] macro provides a convenient way to call `peek`
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
  /// [4]: peek_eq
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
      return Ok(std::cmp::Ordering::Greater);
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
  /// If `f` fails, its error will be returned. If `f` succeeds but
  /// `self.limit() > 0` afterward, [`UnexpectedEof`] will be returned.
  ///
  /// # Examples
  /// ```
  /// use std::io;
  /// use std::io::prelude::*;
  /// use std::io::ErrorKind::*;
  /// use read_write_utils::prelude::*;
  ///
  /// # fn main() -> io::Result<()> {
  /// let mut reader = io::Cursor::new(vec![0xFF, 0xFF, 0xFF]);
  ///
  /// // A function that reads 2 bytes and returns a value.
  /// fn read_u16(reader: &mut impl Read) -> io::Result<u16> {
  ///   let mut buf = [0u8; 2];
  ///   reader.read_exact(&mut buf)?;
  ///   Ok(u16::from_be_bytes(buf))
  /// }
  ///
  /// // The reader has 3 bytes, so the first read_u16 succeeds.
  /// let result: u16 = (&mut reader).take(2).exactly(read_u16)?;
  /// assert_eq!(result, u16::MAX);
  /// assert_eq!(reader.position(), 2);
  ///
  /// // Error! Expected to read exactly 2 bytes, but there was only 1 left.
  /// let result = (&mut reader).take(2).exactly(read_u16);
  /// assert!(result.is_err_and(|err| err.kind() == UnexpectedEof));
  /// assert!(reader.reached_eof()?);
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// [1]: io::Take::limit
  fn exactly<R>(&mut self, f: impl FnOnce(&mut Self) -> io::Result<R>) -> io::Result<R>;
}

impl<I> TakeExt for io::Take<I> {
  fn exactly<R>(&mut self, f: impl FnOnce(&mut Self) -> io::Result<R>) -> io::Result<R> {
    let result = f(self)?;
    if self.limit() > 0 {
      return Err(io::Error::from(UnexpectedEof));
    }
    Ok(result)
  }
}

#[cfg(test)]
mod test {
  use super::*;
  use std::io::BufReader;

  #[test]
  fn copy_to_slice_multiple_reads() -> io::Result<()> {
    let mut cursor = BufReader::with_capacity(2, io::Cursor::new(vec![1u8, 2, 3, 4, 5]));
    let mut buf = [0u8; 5];
    let bytes_copied = cursor.copy_to_slice(&mut buf)?;
    assert_eq!(bytes_copied as usize, buf.len());
    assert_eq!(cursor.get_ref().get_ref().as_slice(), &buf[..]);
    Ok(())
  }

  #[test]
  fn peek_full_read() -> io::Result<()> {
    let mut reader = io::Cursor::new([0u8, 1, 2, 3, 4]);
    let mut buf = [0u8; 3];
    reader.set_position(1);
    assert_eq!(reader.peek(&mut buf)?, &[1, 2, 3]);
    assert_eq!(reader.position(), 1);
    Ok(())
  }

  #[test]
  fn peek_eof() -> io::Result<()> {
    let mut reader = io::Cursor::new([0u8, 1, 2, 3, 4]);
    let mut buf = [0u8; 3];
    reader.set_position(3);
    assert_eq!(reader.peek(&mut buf[..])?, &[3, 4]);
    assert_eq!(reader.position(), 3);
    Ok(())
  }
}
