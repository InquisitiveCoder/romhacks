use std::io;
use std::io::prelude::*;
use std::io::ErrorKind::{Interrupted, UnexpectedEof};

/// Utility methods for readers.
pub trait ReadExt: Read {
  /// Equivalent to `io::copy(self, writer)`. This can be useful if you need to
  /// call [`io::copy`] at the end of a method chain.
  fn copy_to<W>(&mut self, writer: &mut W) -> io::Result<u64>
  where
    W: Write + ?Sized,
  {
    io::copy(self, writer)
  }

  /// Calls [`read`][1] until `buf` is full or EOF is reached.
  ///
  /// This is equivalent to using [`take`][2] and [`io::copy`], but it won't
  /// allocate a redundant buffer and copy the data twice, which is still the
  /// case as of Rust 1.94. It's also considerably shorter and simpler than:
  /// ```
  /// # use std::io;
  /// # use std::io::prelude::*;
  /// # use read_write_utils::prelude::*;
  /// #
  /// # fn compare_copies(mut reader: impl Read, mut buf: &mut [u8]) -> () {
  /// io::copy(&mut (&mut reader).take(buf.len() as u64), &mut buf);
  /// # }
  /// ```
  ///
  /// If you need to ensure that the slice was filled, use [`read_exact`][3]
  /// instead.
  ///
  /// # Errors
  /// Like [`io::copy`], this method will retry any [`read`][1] that fails due
  /// to an [`Interrupted`] error. Any other kind of error will be returned
  /// immediately.
  ///
  /// # Examples
  /// The code below demonstrates the method's behavior when there aren't
  /// enough bytes left in the reader to fill the buffer.
  /// ```
  /// # use std::io::Cursor;
  /// # use std::io::prelude::*;
  /// # use read_write_utils::prelude::*;
  /// #
  /// let mut reader = Cursor::new(vec![1, 2, 3]);
  /// let mut buffer = [0u8; 5];
  ///
  /// // All bytes were copied.
  /// assert_eq!(
  ///    reader.copy_to_slice(&mut buffer[..])?,
  ///    reader.get_ref().len()
  /// );
  /// assert!(reader.reached_eof()?);
  ///
  /// // The first 3 indexes of the buffer have been overwritten.
  /// assert_eq!(&buffer[..], &[1, 2, 3, 0, 0]);
  /// # Ok::<(), std::io::Error>(())
  /// ```
  ///
  /// The code below demonstrates filling a buffer.
  /// ```
  /// # use std::io::prelude::*;
  /// # use std::io::{BufReader, Cursor};
  /// # use read_write_utils::prelude::*;
  /// #
  /// let mut reader = Cursor::new(vec![1, 2, 3u8]);
  /// let mut buffer = [0u8; 2];
  /// // The number of bytes copied is the size of the buffer.
  /// assert_eq!(
  ///   reader.copy_to_slice(&mut buffer[..])?,
  ///   buffer.len()
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
  fn copy_to_slice(&mut self, mut buf: &mut [u8]) -> io::Result<usize> {
    let mut total: usize = 0;
    loop {
      match self.read(buf) {
        Ok(0) => return Ok(total),
        Ok(read_amount) => {
          total = usize::checked_add(total, read_amount)
            .expect("number of bytes copied to a slice shouldn't overflow usize");
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
  /// # use std::io::Cursor;
  /// # use std::io::ErrorKind::UnexpectedEof;
  /// # use std::io::prelude::*;
  /// # use read_write_utils::prelude::*;
  /// #
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

/// Utility methods for buffered readers.
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
  /// # use std::io::prelude::*;
  /// # use std::io::Cursor;
  /// # use read_write_utils::prelude::*;
  /// let mut cursor = Cursor::new(vec![0u8; 3]);
  /// assert!(!cursor.reached_eof()?);
  ///
  /// cursor.set_position(3);
  /// assert!(cursor.reached_eof()?);
  /// # Ok::<(), std::io::Error>(())
  /// ```
  ///
  /// [1]: BufRead::fill_buf
  fn reached_eof(&mut self) -> io::Result<bool> {
    Ok(self.fill_buf()?.is_empty())
  }

  /// Performs an I/O operation if and only if `self` hasn't reached EOF. This
  /// is useful for handling optional data at the end of a reader.
  ///
  /// If `self` is at EOF, returns `Ok(None)`;
  /// otherwise, returns `Ok(Some(f(self)?))`.
  ///
  /// # Errors
  /// This function returns any errors from [`fill_buf`][1] or `f`.
  ///
  /// # Examples
  /// ```
  /// # use std::io;
  /// # use std::io::prelude::*;
  /// # use read_write_utils::prelude::*;
  /// #
  /// fn read_u8(reader: &mut impl Read) -> io::Result<u8> {
  ///   let mut buf = [0u8; 1];
  ///   reader.read_exact(&mut buf[..])?;
  ///   Ok(buf[0])
  /// }
  ///
  /// let mut reader = io::Cursor::new([0xFF; 1]);
  /// let result = reader.if_not_eof(read_u8);
  /// // Read 1 byte, as expected.
  /// assert!(result.is_ok_and(|option| option == Some(0xFF)));
  ///
  /// let mut empty = io::empty(); // always at EOF
  /// let result = empty.if_not_eof(read_u8);
  /// // No error; read_u8 wasn't called.
  /// assert!(result.is_ok_and(|option| option == None));
  ///
  /// # Ok::<(), io::Error>(())
  /// ```
  ///
  /// [1]: BufRead::fill_buf
  fn if_not_eof<R>(&mut self, f: impl FnOnce(&mut Self) -> io::Result<R>) -> io::Result<Option<R>> {
    if self.reached_eof()? {
      return Ok(None);
    }
    Ok(Some(f(self)?))
  }
}
impl<R: BufRead> BufReadExt for R {}

/// Utility methods for [`Read::take()`] adapters.
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

/// Readers that support small, frequent reads like [`BufRead`] but don't
/// necessarily support buffer operations (e.g. [io::Repeat]).
///
/// You should prefer using [`AmortizedRead`] as a trait bound if you don't
/// need access to [`BufRead::fill_buf`].
pub trait AmortizedRead: Read {}
impl AmortizedRead for &[u8] {}
impl AmortizedRead for io::Empty {}
impl AmortizedRead for io::StdinLock<'_> {}
impl<R: AmortizedRead + ?Sized> AmortizedRead for &mut R {}
impl<R: AmortizedRead + ?Sized> AmortizedRead for Box<R> {}
impl<R> AmortizedRead for io::BufReader<R> where io::BufReader<R>: BufRead {}
impl<T> AmortizedRead for io::Cursor<T> where io::Cursor<T>: BufRead {}
impl<R> AmortizedRead for io::Take<R> where io::Take<R>: BufRead {}
impl<T, U> AmortizedRead for io::Chain<T, U> where io::Chain<T, U>: BufRead {}
impl AmortizedRead for io::Repeat {}
impl<T> AmortizedRead for crate::repeat::RepeatSlice<T> where crate::repeat::RepeatSlice<T>: Read {}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn copy_to_slice_multiple_reads() -> io::Result<()> {
    // Use a BufReader with limited capacity to force the read loop to iterate
    // more than once.
    let reader = io::Cursor::new(vec![1u8, 2, 3, 4, 5]);
    let mut reader = io::BufReader::with_capacity(2, reader);
    let mut buf = [0u8; 5];
    let bytes_copied = reader.copy_to_slice(&mut buf)?;
    assert_eq!(bytes_copied, buf.len());
    assert_eq!(reader.get_ref().get_ref().as_slice(), &buf[..]);
    Ok(())
  }
}
