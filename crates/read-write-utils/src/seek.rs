use crate::prelude::ReadExt;
use polonius_the_crab::prelude::*;
use std::cmp::Ordering;
use std::io;
use std::io::prelude::*;
use std::io::ErrorKind::InvalidInput;

/// Readers that support relative seeks, but may not support seeks relative to
/// the start or end of the stream (e.g. [io::Repeat]).
///
/// Prefer using [`SeekRelative`] as a trait bound if you don't need access to
/// [`Seek::seek`].
pub trait SeekRelative {
  /// Seeks relative to the current position.
  ///
  /// If the implementing type also implements [`Seek`], this method must behave
  /// identically to [seek_relative][1]. Otherwise, this method is allowed to
  /// seek before the start of the stream if the stream is capable of performing
  /// a [`read`][2] or [`write`][3] from that position; for example, when a
  /// reader repeats the same data infinitely, or a writer discards all data.
  ///
  /// This method deliberately uses a different name to avoid conflicts when a
  /// type implements both traits.
  ///
  /// [1]: Seek::seek_relative
  /// [2]: Read::read
  /// [3]: Write::write
  fn relative_seek(&mut self, offset: i64) -> io::Result<()>;
}

/// Utility methods for buffered readers that support relative seeks.
pub trait Peek: BufRead + SeekRelative {
  /// Peeks at the next `buf.len()` bytes in the reader. The returned slice's
  /// length can be smaller than `buf` if EOF is reached.
  ///
  /// This method first attempts to return `buf.len()` bytes from
  /// [`self.fill_buf()`][1]. Otherwise, [`self.copy_to_slice(buf)`][2] will
  /// be called, followed by a backwards [`relative_seek`][3] to return `self`
  /// to its former position.
  ///
  /// For peeks much smaller than the size of the reader's internal buffer,
  /// there's a high probability that the copy and seek are avoided.
  ///
  /// The [`peek_eq!`][4] macro provides a convenient way to call `peek`
  /// and compare the result to a `const` slice or slice literal.
  ///
  /// The [`peek`] function provides an alternative for types that implement
  /// [`Seek`] and can't implement [`SeekRelative`].
  ///
  /// # Errors
  /// This method can return any error from [`fill_buf`][1], [`io::copy`] and
  /// [`relative_seek`][3].
  ///
  /// # Examples
  /// ```
  /// # use std::io;
  /// # use std::io::{Cursor, SeekFrom};
  /// # use std::io::prelude::*;
  /// # use read_write_utils::prelude::*;
  /// #
  /// let mut reader = Cursor::new([0u8, 1, 2, 3, 4, 5, 6, 7]);
  /// let mut buf = [0u8; 3];
  /// assert_eq!(reader.peek(&mut buf[..])?, &[0, 1, 2]);
  /// reader.set_position(6);
  /// assert_eq!(reader.peek(&mut buf[..])?, &[6, 7]);
  /// # Ok::<(), io::Error>(())
  /// ```
  ///
  /// [1]: BufRead::fill_buf
  /// [2]: ReadExt::copy_to_slice
  /// [3]: SeekRelative::relative_seek
  /// [4]: crate::peek_eq!
  fn peek<'a>(&'a mut self, buf: &'a mut [u8]) -> io::Result<&'a [u8]> {
    peek(self, buf, SeekRelative::relative_seek)
  }

  /// [Compares][1] the remaining number of bytes in the reader to `amt`.
  ///
  /// This method first compares the length of the slice returned by
  /// [`fill_buf`][2]; if it's greater than `amt`, that result is returned.
  /// Otherwise, up to `amt + 1` bytes are read from `self`, followed by a
  /// backwards [`relative_seek`][6] to return `self` to its former position.
  /// The number of bytes read is then compared to `amt`.
  ///
  /// If `amt` is much smaller than the size of the reader's internal buffer,
  /// there's a high probability that the copy and seek are avoided.
  ///
  /// The [`peek_len`] function provides an alternative for types that implement
  /// [`Seek`] and can't implement [`SeekRelative`].
  ///
  /// # Errors
  /// If `amt + 1` can't be converted to an `i64`, this method returns
  /// [`InvalidInput`]. Otherwise, see [`std::io::copy`] and
  /// [`relative_seek`][6].
  ///
  /// [1]: Ord::cmp
  /// [2]: BufRead::fill_buf
  /// [3]: io::Take::take
  /// [4]: io::copy
  /// [5]: io::sink
  /// [6]: SeekRelative::relative_seek
  fn peek_len(&mut self, amt: usize) -> io::Result<Ordering> {
    peek_len(self, amt, SeekRelative::relative_seek)
  }
}

impl<R: BufRead + SeekRelative> Peek for R {}

macro_rules! impl_with_seek {
  () => {
    fn relative_seek(&mut self, offset: i64) -> ::std::io::Result<()> {
      ::std::io::Seek::seek_relative(self, offset)
    }
  };
}

impl SeekRelative for std::fs::File {
  impl_with_seek!();
}

impl SeekRelative for std::sync::Arc<std::fs::File> {
  impl_with_seek!();
}

impl SeekRelative for io::Empty {
  impl_with_seek!();
}

impl<R> SeekRelative for io::BufReader<R>
where
  R: Seek + ?Sized,
{
  impl_with_seek!();
}

impl<S> SeekRelative for Box<S>
where
  S: Seek + ?Sized,
{
  impl_with_seek!();
}

impl<S: AsRef<[u8]>> SeekRelative for io::Cursor<S> {
  impl_with_seek!();
}

impl<S: Seek> SeekRelative for io::Take<S> {
  impl_with_seek!();
}

impl<S> SeekRelative for io::BufWriter<S>
where
  S: Write + Seek + ?Sized,
{
  impl_with_seek!();
}

impl SeekRelative for io::Repeat {
  fn relative_seek(&mut self, _offset: i64) -> io::Result<()> {
    Ok(())
  }
}

impl SeekRelative for io::Sink {
  fn relative_seek(&mut self, _offset: i64) -> io::Result<()> {
    Ok(())
  }
}

impl<S> SeekRelative for &mut S
where
  S: SeekRelative + ?Sized,
{
  fn relative_seek(&mut self, offset: i64) -> io::Result<()> {
    (*self).relative_seek(offset)
  }
}

/// Implementation of [`Peek::peek`] that accepts a closure for its
/// `seek_relative` implementation.
///
/// This provides a workaround when a type from an external crate implements
/// [`Seek`], but not [`SeekRelative`].
fn peek<'a, S, F>(
  mut reader: &'a mut S,
  buf: &'a mut [u8],
  seek_relative: F,
) -> io::Result<&'a [u8]>
where
  S: BufRead + ?Sized,
  F: FnOnce(&mut S, i64) -> io::Result<()>,
{
  // the polonius macro seems to misbehave when its pseudo-parameter is self.
  polonius!(|reader| -> Result<&'polonius [u8], io::Error> {
    let inner_buf = polonius_try!(reader.fill_buf());
    if let Some(slice) = inner_buf.get(0..buf.len()) {
      polonius_return!(Ok(slice));
    }
  });
  let copy_amt = reader.copy_to_slice(buf)?;
  // The only way to overflow this cast is with a 9 exabyte slice.
  seek_relative(reader, -(copy_amt as i64))?;
  Ok(&buf[0..copy_amt])
}

/// Implementation of [`Peek::peek_len`] that accepts a closure for its
/// `seek_relative` implementation.
///
/// This provides a workaround when a type from an external crate implements
/// [`Seek`], but not [`SeekRelative`].
fn peek_len<S, F>(reader: &mut S, amt: usize, seek_relative: F) -> io::Result<Ordering>
where
  S: BufRead + ?Sized,
  F: FnOnce(&mut S, i64) -> io::Result<()>,
{
  use Ordering::Greater;
  i64::try_from(amt).map_err(|_| InvalidInput)?;
  if let Greater = reader.fill_buf()?.len().cmp(&amt) {
    return Ok(Greater);
  }
  // All casts and arithmetic below this line are safe since amt <= i64::MAX.
  let copy_amt = reader.take(amt as u64 + 1).copy_to(&mut io::sink())?;
  seek_relative(reader, -(copy_amt as i64))?;
  Ok(copy_amt.cmp(&(amt as u64)))
}
