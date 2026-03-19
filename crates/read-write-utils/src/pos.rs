use crate::prelude::*;
use crate::repeat::RepeatSlice;
use checked_signed_diff::prelude::*;
use std::io;
use std::io::prelude::*;
use std::io::ErrorKind::*;
use std::ops::Deref;

const ERR_MSG: &str = "PositionTracker position overflowed.";

/// An I/O adapter which tracks the cursor position of its underlying stream.
///
/// There are various use cases that require knowing a stream's cursor position.
/// This is trivial, but tedious, to track manually. Since a variable  must be
/// updated each time any I/O is done, this approach tends to clutter up the
/// implementation of I/O-heavy code. Alternatively, if the stream implements
/// [`Seek`], the position can be obtained by calling [`stream_position`][1],
/// but this usually entails a system call.
///
/// `PositionTracker` addresses this problem by wrapping every I/O operation and
/// automatically updating an internal position variable accordingly. This
/// facilitates common tasks such as:
/// * Calculating relative offsets for [`seek_relative`][2] to avoid prematurely
///   discarding a reader's internal buffer. `PositionTracker` converts calls to
///   [`seek`][2] into [`seek_relative`][3] whenever possible.
/// * [Copying bytes until a specific position is reached][4].
/// * Writing methods that decode variable-length data; there's no longer a need
///   to clutter up the return value by including the number of bytes read.
/// * Exiting a loop when the reader reaches a specific position.
///
/// # Copy Optimizations
/// [`io::copy`] attempts to optimize cases where the reader or writer are
/// `std` types. For instance, it can use the internal buffers of a
/// [`BufReader`][5] or [`BufWriter`][6] instead of allocating an additional
/// buffer and performing redundant copies. Additionally, it can delegate
/// [`File`][7]-to-[`File`][7] copies to the Linux kernel, provided they're
/// wrapped only in `std` adapters.
///
/// Passing a `PositionTracker` to `copy` can disable these optimizations. For
/// this reason, `PositionTracker` provides methods that copy from or to the
/// inner stream (e.g. [`copy_to`][8]). These methods should be preferred in
/// generic code and when wrapping `std` types.
///
/// [1]: Seek::stream_position
/// [2]: Seek::seek
/// [3]: Seek::seek_relative
/// [4]: PositionTracker::take_until
/// [5]: io::BufReader
/// [6]: io::BufWriter
/// [7]: std::fs::File
/// [8]: PositionTracker::copy_to
pub struct PositionTracker<T> {
  inner: T,
  position: u64,
}

impl<S: Seek> PositionTracker<S> {
  /// Returns a `PositionTracker` initialized to [`inner.stream_position()`][1].
  ///
  /// If the stream's position is known, use [`from_start`][2] or
  /// [`with_known_position`][3]
  ///
  /// [1]: Seek::stream_position
  /// [2]: Self::from_start
  /// [3]: Self::at_position
  pub fn with_unknown_position(mut inner: S) -> io::Result<Self> {
    let position = inner.stream_position()?;
    Ok(Self::at_position(position, inner))
  }
}

impl<T> PositionTracker<T> {
  /// Equivalent to [`PositionTracker::with_known_position(0, inner)`][1].
  ///
  /// [1]: Self::at_position
  pub fn from_start(inner: T) -> Self {
    Self { inner, position: 0 }
  }

  /// Creates a `PositionTracker` initialized to the given position.
  ///
  /// The provided position must match `inner`'s cursor position in order for
  /// `PositionTracker` to behave correctly.
  ///
  /// If the stream's position isn't known, use [`with_unknown_position`][1].
  ///
  /// # Example
  /// This example demonstrates what happens if an **incorrect** position is
  /// provided.
  /// ```
  /// # use std::io::prelude::*;
  /// # use std::io::Cursor;
  /// # use read_write_utils::pos::PositionTracker;
  /// #
  /// let mut inner = Cursor::new(vec![0u8, 1, 2, 3]);
  /// let mut tracker = PositionTracker::at_position(1, inner);
  /// assert_eq!(tracker.position(), 1);
  /// let mut buf: [u8; 1] = [0];
  /// tracker.read(&mut buf[..]);
  /// // Even though the tracker thinks it's at position == 1,
  /// // it ends up reading the first byte.
  /// assert_eq!(0, buf[0]);
  /// // The tracker's position has been incremented by 1, but is still wrong.
  /// assert_eq!(tracker.position(), 2);
  /// ```
  ///
  /// [1]: Self::with_unknown_position
  pub fn at_position(position: u64, inner: T) -> Self {
    Self { inner, position }
  }

  /// The calculated position of the inner stream.
  ///
  /// The accuracy of this value depends on the [`PositionTracker`] being
  /// created with a correct initial position. For instance, if
  /// [`PositionTracker::from_start`] is used to create a tracker, the inner
  /// stream _must_ be at position 0 when the tracker is created.
  ///
  /// Finally, be aware that many [`Seek`] implementations allow seeking past
  /// EOF and return such positions from [`Seek::stream_position()`]. It's
  /// likewise not an error for this method to positions beyond EOF.
  ///
  /// [1]: Read::read
  /// [2]: Write::write
  /// [3]: Seek::seek
  pub fn position(&self) -> u64 {
    self.position
  }

  /// Gets a reference to the underlying stream.
  pub fn inner(&self) -> &T {
    &self.inner
  }

  /// Unwraps this `PositionTracker`, returning the underlying stream.
  pub fn into_inner(self) -> T {
    self.inner
  }

  /// Unwraps this `PositionTracker`, returning the underlying stream and
  /// [`Self::position`].
  pub fn into_parts(self) -> (T, u64) {
    (self.inner, self.position)
  }

  fn increment_position(&mut self, amt: impl TryInto<u64>) {
    let amount: u64 = amt.try_into().ok().expect(ERR_MSG);
    self.position = self.position.checked_add(amount).expect(ERR_MSG);
  }

  fn increment_position_signed(&mut self, amt: i64) {
    self.position = self.position.checked_add_signed(amt).expect(ERR_MSG);
  }
}

impl<R: Read> PositionTracker<R> {
  /// Calls [`io::copy`] with the inner reader and updates [`position`][1].
  ///
  /// See [`Copy Optimizations`](PositionTracker#copy-optimizations) for more
  /// details.
  ///
  /// [1]: Self::position
  pub fn copy_to<W>(&mut self, writer: &mut W) -> io::Result<u64>
  where
    W: Write + ?Sized,
  {
    let num_copied = self.inner.copy_to(writer)?;
    self.increment_position(num_copied);
    Ok(num_copied)
  }

  /// Calls [`io::copy`] with the inner reader and writer of both parameters and
  /// updates the [positions](Self::position`) of both.
  ///
  /// This function has a few benefits over calling `copy` directly:
  /// * It potentially enables `copy` to delegate file-to-file copies to the
  ///   Linux kernel, which it can't do if the reader is a `PositionTracker`.
  /// * It allows the position to be updated only once.
  /// * It provides a fluent interface, which some people may find preferable.
  pub fn copy_to_inner<W>(&mut self, writer: &mut PositionTracker<W>) -> io::Result<u64>
  where
    W: Write,
  {
    let num_copied = self.copy_to(&mut writer.inner)?;
    writer.increment_position(num_copied);
    Ok(num_copied)
  }

  /// [`Copies`][1] exactly `amount` bytes from the inner reader of this
  /// [`PositionTracker`] to `writer`.
  ///
  /// Equivalent to using [`self.take_from_inner`][2], [`TakeExt::exactly`] and
  /// [`io::copy`].
  ///
  /// [1]: io::copy
  /// [2]: Self::take
  pub fn copy_exactly(&mut self, amount: u64, writer: &mut impl Write) -> io::Result<u64> {
    self.take_exactly(amount, |reader| reader.copy_to(writer))
  }

  /// [`Copies`][1] bytes from the inner reader of this [`PositionTracker`] to
  /// `writer` until the reader reaches `SeekFrom::Start(pos)`.
  ///
  /// Equivalent to using [`self.take_until`][2],
  /// [`TakeExt::exactly`] and [`io::copy`].
  ///
  /// # Errors
  /// * [`take_from_inner_until`][2] can return [`InvalidData`].
  /// * [`TakeExt::exactly`] can return [`UnexpectedEof`].
  /// * Any error returned by [`io::copy`].
  ///
  /// [1]: io::copy
  /// [2]: Self::take_until
  pub fn copy_until(&mut self, pos: u64, writer: &mut impl Write) -> io::Result<u64> {
    self.take_until(pos, |reader| reader.copy_to(writer))
  }

  /// [`Copies`][1] exactly `amount` bytes from the inner reader and writer of
  /// both [`PositionTracker`]s.
  ///
  /// Equivalent to using [`self.take_from_inner`][2], [`TakeExt::exactly`] and
  /// [`writer.copy_from`][3].
  ///
  /// # Errors
  /// * [`TakeExt::exactly`] can return [`UnexpectedEof`].
  /// * Any error returned by [`io::copy`].
  ///
  /// [1]: io::copy
  /// [2]: Self::take
  /// [3]: Self::copy_from
  pub fn copy_to_inner_exactly(
    &mut self,
    amount: u64,
    writer: &mut PositionTracker<impl Write>,
  ) -> io::Result<u64> {
    self.take_exactly(amount, |reader| reader.copy_to_inner(writer))
  }

  /// [`Copies`][1] bytes from the inner reader and writer of both
  /// [`PositionTracker`]s until the reader reaches `SeekFrom::Start(offset)`.
  ///
  /// Equivalent to using [`self.take_from_inner_until`][2],
  /// [`TakeExt::exactly`] and [`reader.copy_to_inner_of(writer)`][3].
  ///
  /// [1]: io::copy
  /// [2]: Self::take
  /// [3]: PositionTrackerReadExt::copy_to_inner
  pub fn copy_to_inner_until(
    &mut self,
    offset: u64,
    writer: &mut PositionTracker<impl Write>,
  ) -> io::Result<u64> {
    self.take_until(offset, |reader| reader.copy_to_inner(writer))
  }

  /// Calls [`Read::take`] on the inner reader and applies it to `f`.
  ///
  /// Generic code should prefer using this function over `self.take()`, since
  /// passing a `Take<PositionTracker<_>>` (or any other non-`std` reader or
  /// writer) to [`io::copy`] will prevent it from offloading file-to-file
  /// copies to the Linux kernel.
  ///
  /// # Examples
  /// ```
  /// use std::io::prelude::*;
  /// use std::io::{copy, sink, Cursor};
  /// use read_write_utils::pos::PositionTracker;
  ///
  /// let mut reader = PositionTracker::from_start(Cursor::new(vec![0u8, 1, 2, 3, 4]));
  ///
  /// let bytes_read = reader.take(4, |take| copy(take, &mut sink()));
  /// assert_eq!(bytes_read.unwrap(), 4);
  /// assert_eq!(reader.position(), 4);
  ///
  /// let bytes_read = reader.take(4, |take| copy(take, &mut sink()));
  /// assert_eq!(bytes_read.unwrap(), 1);
  /// assert_eq!(reader.position(), 5);
  /// ```
  pub fn take<T, F>(&mut self, amt: u64, f: F) -> io::Result<T>
  where
    F: FnOnce(&mut io::Take<&mut R>) -> io::Result<T>,
  {
    let mut inner = (&mut self.inner).take(amt);
    let result = f(&mut inner);
    let num_read = amt - inner.limit();
    self.increment_position(num_read);
    result
  }

  pub fn take_exactly<T, F>(&mut self, amt: u64, f: F) -> io::Result<T>
  where
    F: FnOnce(&mut io::Take<&mut R>) -> io::Result<T>,
  {
    self.take(amt, |take| take.exactly(f))
  }

  /// Equivalent to [`self.take_exactly(pos - self.position())`][1].
  ///
  /// # Errors
  /// * [`InvalidInput`] if `pos < self.position()`.
  /// * [`UnexpectedEof`] if EOF is reached before `pos`.
  ///
  /// [1]: Self::take_exactly
  ///
  /// # Examples
  /// ```
  /// # use std::io;
  /// # use std::io::prelude::*;
  /// # use std::io::Cursor;
  /// # use read_write_utils::prelude::*;
  ///
  /// let mut reader = PositionTracker::from_start(Cursor::new(vec![0u8, 1, 2, 3, 4]));
  /// reader.seek_relative(1).unwrap();
  /// let mut output = Vec::<u8>::new();
  /// let bytes_copied = reader.take_until(3, |take| {
  ///    io::copy(take, &mut output)
  /// }).unwrap();
  /// assert_eq!(bytes_copied, 2);
  /// assert_eq!(&output[..], &[1, 2]);
  ///
  /// let error = reader.take_until(0, |take| Ok(()));
  /// assert!(error.is_err_and(|err| err.kind() == io::ErrorKind::InvalidInput));
  /// ```
  pub fn take_until<T, F>(&mut self, pos: u64, f: F) -> io::Result<T>
  where
    F: FnOnce(&mut io::Take<&mut R>) -> io::Result<T>,
  {
    match pos.checked_sub(self.position) {
      Some(amt) => self.take_exactly(amt, f),
      None => Err(io::Error::from(InvalidInput)),
    }
  }
}

impl<S: Seek> Seek for PositionTracker<S> {
  /// Seeks to the given position in the underlying stream and updates
  /// [`self.position()`][1] if it succeeds.
  ///
  /// This function will call [`seek_relative`][2] on the inner stream whenever
  /// possible to take advantage of its performance benefits. Specifically:
  /// * When seeking from the start of the stream, [`seek_relative`][2] will be
  ///   used if the offset from the current position fits in an `i64`.
  /// * If the pos is relative to the current position, [`seek_relative`][2]
  ///   is always used.
  ///
  /// [1]: Self::position
  /// [2]: Seek::seek_relative
  fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
    use io::SeekFrom::*;
    let relative_offset = match pos {
      Start(position) => position.checked_signed_difference(self.position),
      Current(offset) => Some(offset),
      _ => None,
    };
    match relative_offset {
      Some(offset) => Seek::seek_relative(self, offset)?,
      None => {
        self.position = self.inner.seek(pos)?;
      }
    }
    Ok(self.position)
  }

  /// Returns [`Ok(self.position())`](PositionTracker::position).
  fn stream_position(&mut self) -> io::Result<u64> {
    Ok(self.position())
  }

  /// Calls [`seek_relative`][1] on the inner stream and updates
  /// [`self.position()`][2].
  ///
  /// [1]: Seek::seek_relative
  /// [2]: Self::position
  fn seek_relative(&mut self, offset: i64) -> io::Result<()> {
    if offset != 0 {
      self.inner.seek_relative(offset)?;
      self.increment_position_signed(offset);
    }
    Ok(())
  }
}

impl<S: SeekRelative> SeekRelative for PositionTracker<S> {
  /// Calls [`seek_relative`][1] on the inner stream and updates
  /// [`self.position()`][2].
  ///
  /// [1]: SeekRelative::seek_relative
  /// [2]: Self::position
  fn seek_relative(&mut self, offset: i64) -> io::Result<()> {
    SeekRelative::seek_relative(&mut self.inner, offset)?;
    self.increment_position_signed(offset);
    Ok(())
  }
}

impl<W: Write> PositionTracker<W> {
  /// [Copies][1] from `reader` to this [`PositionTracker`]'s underlying writer.
  ///
  /// See [Copy Optimizations][2] for additional information.
  ///
  /// [1]: io::copy
  /// [2]: PositionTracker#copy-optimizations
  pub fn copy_from(&mut self, reader: &mut (impl Read + ?Sized)) -> io::Result<u64> {
    let amount_copied = io::copy(reader, &mut self.inner)?;
    self.increment_position(amount_copied);
    Ok(amount_copied)
  }
}

impl<I: AsRead> PositionTracker<I> {
  /// Performs tracked I/O operations on the inner stream of this [`BufWrite`].
  ///
  /// The writer will be [flushed](Write::flush) prior to calling `f`.
  /// After `f` returns, [`self.position`](Self::position) will be updated
  /// to match the `PositionTracker` of the inner stream.
  ///
  /// A noteworthy use case this function facilitates is reading from a file
  /// wrapped by a [`BufWriter`][1].
  ///
  /// [1]: io::BufWriter
  pub fn read_from_inner<F, R>(&mut self, f: F) -> io::Result<R>
  where
    F: FnOnce(&mut PositionTracker<&mut I::Reader>) -> io::Result<R>,
  {
    let mut inner_tracker = PositionTracker::at_position(self.position, self.inner.as_reader()?);
    let result = f(&mut inner_tracker)?;
    self.position = inner_tracker.position();
    Ok(result)
  }
}

impl<R: Read> Read for PositionTracker<R> {
  fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
    let amount: usize = self.inner.read(buf)?;
    self.increment_position(amount);
    Ok(amount)
  }
}

impl<R: BufRead> BufRead for PositionTracker<R> {
  /// Calls [`fill_buf`](BufRead::fill_buf) on the inner reader.
  fn fill_buf(&mut self) -> io::Result<&[u8]> {
    self.inner.fill_buf()
  }

  /// Calls [`BufRead::consume`] on the inner reader and updates
  /// [`Self::position`].
  ///
  /// The `amt` must be `<=` the number of bytes in the buffer returned by
  /// [`fill_buf`](BufRead::fill_buf).
  fn consume(&mut self, amt: usize) {
    self.inner.consume(amt);
    self.increment_position(amt);
  }
}

impl<W: Write> Write for PositionTracker<W> {
  fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
    let amount = self.inner.write(buf)?;
    self.increment_position(amount);
    Ok(amount)
  }

  fn flush(&mut self) -> io::Result<()> {
    self.inner.flush()
  }
}

impl<W: BufWrite> BufWrite for PositionTracker<W> {}

impl<T> Deref for PositionTracker<T> {
  type Target = T;

  fn deref(&self) -> &Self::Target {
    &self.inner
  }
}

pub trait PositionTrackerReadExt: Read {
  /// Equivalent to [`PositionTracker::copy_from`].
  ///
  /// The only advantage of this method is that the order the reader and writer
  /// is consistent with [`io::copy`].
  fn copy_to_inner(&mut self, writer: &mut PositionTracker<impl Write>) -> io::Result<u64> {
    writer.copy_from(self)
  }
}

impl<R: Read> PositionTrackerReadExt for R {}

impl<T> Seek for PositionTracker<RepeatSlice<T>> {
  fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
    match pos {
      io::SeekFrom::Start(pos) => self.position = pos,
      io::SeekFrom::Current(offset) => self.increment_position(offset),
      io::SeekFrom::End(_) => self.position = u64::MAX,
    }
    Ok(self.position)
  }
}
