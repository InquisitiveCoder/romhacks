use polonius_the_crab::prelude::*;
use std::cmp::Ordering;
use std::io;
use std::io::prelude::*;
use std::io::ErrorKind::{Interrupted, UnexpectedEof};

pub trait ReadExt: Read {
  /// Equivalent to `io::copy(self, writer)`. This can be useful if you need to
  /// call [`io::copy`] at the end of a method chain.
  fn copy_to(&mut self, writer: &mut impl Write) -> io::Result<u64> {
    io::copy(self, writer)
  }

  /// Calls [`read`][1] until `slice` is full or EOF is reached.
  ///
  /// This is equivalent to using [`take`][2] and [`io::copy`], but it won't
  /// allocate a redundant buffer and copy the data twice, which is still the
  /// case as of the time of this writing (Rust 1.94). Additionally, it's _much_
  /// simpler syntactically. Compare to:
  /// ```no_run
  /// io::copy(&mut (&mut reader).take(buf.len() as u64), &mut buf)
  /// ```
  ///
  /// If you want to ensure that the slice was filled, use [`read_exact`][3]
  /// instead.
  ///
  /// # Errors
  /// Like [`io::copy`], if [`read`][1] fails due to an [`Interrupted`] error,
  /// this function will retry the operation. If [`read`][1] returns any other
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
  fn copy_to_slice(&mut self, mut buf: &mut [u8]) -> io::Result<u64> {
    let mut total: u64 = 0;
    loop {
      match self.read(buf) {
        Ok(0) => return Ok(total),
        Ok(read_amount) => {
          total += read_amount as u64;
          buf = &mut buf[read_amount..];
        }
        Err(e) if e.kind() == Interrupted => {}
        Err(e) => return Err(e),
      }
    }
  }

  /// Uses [`read_exact`][1] to fill and return an array of length `N`.
  ///
  /// This method provides a stable alternative to [`read_array`][2], which is
  /// still nightly-only as of Rust 1.94.0.
  ///
  /// If you intend to compare the result of this method to a `const` slice or
  /// slice literal, see the [`read_array_eq!`][3] macro.
  ///
  /// # Errors
  /// This function returns any error from [`read_exact`][1].
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
  /// [1]: Read::read_exact
  /// [2]: Read::read_array
  /// [3]: crate::read_array_eq!
  fn read_n<const N: usize>(&mut self) -> io::Result<[u8; N]> {
    let mut arr = [0u8; N];
    self.read_exact(&mut arr)?;
    Ok(arr)
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
  /// The [`peek_eq!`][4] macro provides a convenient way to call `peek`
  /// and compare the result to a `const` slice or slice literal.
  ///
  /// # Errors
  /// This method can return any error from [`fill_buf`][1], [`io::copy`] and
  /// [`seek_relative`][3].
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
  /// [4]: crate::peek_eq!
  fn peek<'a>(&'a mut self, buf: &'a mut [u8]) -> io::Result<&'a [u8]>
  where
    Self: Seek,
  {
    // the polonius macro seems to misbehave when its pseudo-parameter is self.
    let mut reader = self;
    polonius!(|reader| -> Result<&'polonius [u8], io::Error> {
      let inner_buf = polonius_try!(reader.fill_buf());
      if let Some(slice) = inner_buf.get(0..buf.len()) {
        polonius_return!(Ok(slice));
      }
    });
    let copy_amt = reader.copy_to_slice(buf)?;
    reader.seek_relative(-(copy_amt as i64))?;
    Ok(&buf[0..copy_amt as usize])
  }

  /// [Compares][1] the remaining number of bytes in the reader to `amt`.
  ///
  /// This method first compares the length of the slice returned by
  /// [`fill_buf`][2]; if it's greater than `amt`, that result is returned.
  /// Otherwise, [up to][3] `amt + 1` bytes are [copied][4] to a [sink][5],
  /// followed by a backwards [`seek_relative`][6] to return `self` to its
  /// former position. The number of bytes copied is then compared to `amt`.
  ///
  /// If `amt` is much smaller than the size of the reader's internal buffer,
  /// there's a high probability that the copy and seek are avoided.
  ///
  /// # Errors
  /// If `amt + 1` can't be converted to an `i64`, this method returns
  /// [`InvalidInput`]. Otherwise, see [`std::io::copy`] and
  /// [`seek_relative`][6].
  ///
  /// [1]: Ord::cmp
  /// [2]: BufRead::fill_buf
  /// [3]: io::Take::take
  /// [4]: io::copy
  /// [5]: io::sink
  /// [6]: Seek::seek_relative
  fn peek_len(&mut self, amt: usize) -> io::Result<Ordering>
  where
    Self: Seek,
  {
    use Ordering::Greater;
    if let Greater = self.fill_buf()?.len().cmp(&amt) {
      return Ok(Greater);
    }
    let copy_amt = self.take(amt as u64 + 1).copy_to(&mut io::sink())?;
    self.seek_relative(-(copy_amt as i64))?;
    Ok(copy_amt.cmp(&(amt as u64)))
  }
}
impl<R: BufRead> BufReadExt for R {}

pub trait TakeExt {
  /// Executes an I/O operation and asserts that it read exactly
  /// [`self.limit()`][1] bytes.
  ///
  /// In other words, this method allows the "exact" aspect of
  /// [`read_exact`][2] to be applied to arbitrary I/O functions.
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
  /// let mut file_a = io::Cursor::new([0xFF; 3]);
  /// let mut file_b: Vec<u8> = vec![];
  ///
  /// // This will fail since io::copy will reach EOF before it can copy 5 bytes.
  /// // The type annotation for the closure parameter isn't necessary, and has
  /// // been included for the sake of clarity.
  /// let result = file_a.take(5).exactly(|file_a: &mut io::Take<_>| {
  ///   io::copy(file_a, &mut file_b)
  /// });
  /// assert!(result.is_err_and(|err| err.kind() == UnexpectedEof));
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// [1]: io::Take::limit
  /// [2]: Read::read_exact
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
