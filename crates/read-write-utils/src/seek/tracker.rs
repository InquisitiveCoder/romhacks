use crate::prelude::{BufWrite, TakeExt};
use checked_signed_diff::prelude::*;
use std::io::ErrorKind::*;
use std::io::*;
use std::ops::Deref;

const ERR_MSG: &'static str = "PositionTracker position overflowed.";

/// An I/O adapter which tracks the cursor position of its underlying stream.
///
/// This facilitates various usage patterns such as [`Seek::seek_relative`],
/// copying bytes until an absolute position is reached, and reading
/// variable-length data.
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
  /// [3]: Self::with_known_position
  pub fn with_unknown_position(mut inner: S) -> Result<Self> {
    let position = inner.stream_position()?;
    Ok(Self::with_known_position(position, inner))
  }
}

impl<T> PositionTracker<T> {
  /// Equivalent to [`PositionTracker::with_known_position(0, inner)`][1].
  ///
  /// [1]: Self::with_known_position
  pub fn from_start(inner: T) -> Self {
    Self { inner, position: 0 }
  }

  /// Creates a `PositionTracker` initialized to the given seek position.
  /// It's the caller's responsibility to ensure `position` matches `inner`'s
  /// seek position.
  ///
  /// If the stream's position isn't known, use [`new`][1].
  ///
  /// # Example
  /// This example demonstrates what happens if an incorrect position is
  /// provided.
  /// ```
  /// use std::io::prelude::*;
  /// use std::io::Cursor;
  /// use read_write_utils::seek::PositionTracker;
  ///
  /// let mut inner = Cursor::new(vec![0u8, 1, 2, 3]);
  /// let mut tracker = PositionTracker::with_known_position(1, inner);
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
  pub fn with_known_position(position: u64, inner: T) -> Self {
    Self { inner, position }
  }

  /// The calculated position of the inner stream.
  ///
  /// The accuracy of this value depends on the [`PositionTracker`] being
  /// created with a correct initial position. For instance, if
  /// [`PositionTracker::from_start`] is used to create a tracker, the inner
  /// stream _must_ be at position 0 when the tracker is created.
  ///
  /// Performing I/O operations directly on the inner stream (e.g. via
  /// [`BufWrite::inner_mut`]) will also desynchronize the calculated position.
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

  /// Gets a reference to the underlying reader.
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
  /// Calls [`copy`] with the inner reader and updates [`Self::position`].
  ///
  /// This function has a few benefits over calling `copy` directly:
  /// * It potentially enables `copy` to delegate file-to-file copies to the
  /// Linux kernel, which it can't do if the reader is a `PositionTracker`.
  /// * It allows the position to be updated only once.
  /// * It provides a fluent interface, which some people may find preferable.
  pub fn copy_to(&mut self, writer: &mut impl Write) -> Result<u64> {
    let num_copied = copy(&mut self.inner, writer)?;
    self.increment_position(num_copied);
    Ok(num_copied)
  }

  /// Calls [`copy`] with the inner reader and writer of both parameters and
  /// updates the [positions](Self::position`) of both.
  ///
  /// This function has a few benefits over calling `copy` directly:
  /// * It potentially enables `copy` to delegate file-to-file copies to the
  /// Linux kernel, which it can't do if the reader is a `PositionTracker`.
  /// * It allows the position to be updated only once.
  /// * It provides a fluent interface, which some people may find preferable.
  pub fn copy_to_other(&mut self, writer: &mut PositionTracker<impl Write>) -> Result<u64> {
    let num_copied = self.copy_to(&mut writer.inner)?;
    writer.increment_position(num_copied);
    Ok(num_copied)
  }

  /// [`Copies`][1] exactly `amount` bytes from the inner reader of this
  /// [`PositionTracker`] to `writer`.
  ///
  /// Equivalent to using [`self.take_from_inner`][2], [`TakeExt::exactly`] and
  /// [`copy`].
  ///
  /// [1]: copy
  /// [2]: Self::take_from_inner
  pub fn copy_exactly(&mut self, amount: u64, writer: &mut impl Write) -> Result<u64> {
    self.take_from_inner(amount, |take| take.exactly(|reader| copy(reader, writer)))
  }

  /// [`Copies`][1] bytes from the inner reader of this [`PositionTracker`] to
  /// `writer` until the reader reaches `SeekFrom::Start(offset)`.
  ///
  /// Equivalent to using [`self.take_from_inner_until`][2],
  /// [`TakeExt::exactly`] and [`copy`].
  ///
  /// [1]: copy
  /// [2]: Self::take_from_inner
  pub fn copy_until(&mut self, offset: u64, writer: &mut impl Write) -> Result<u64> {
    self.take_from_inner_until(offset, |take| take.exactly(|reader| copy(reader, writer)))
  }

  /// [`Copies`][1] exactly `amount` bytes from the inner reader and writer of
  /// both [`PositionTracker`]s.
  ///
  /// Equivalent to using [`self.take_from_inner`][2], [`TakeExt::exactly`] and
  /// [`writer.copy_from`][3].
  ///
  /// [1]: copy
  /// [2]: Self::take_from_inner
  /// [3]: Self::copy_to_inner_from
  pub fn copy_to_other_exactly(
    &mut self,
    amount: u64,
    writer: &mut PositionTracker<impl Write>,
  ) -> Result<u64> {
    self.take_from_inner(amount, |take| {
      take.exactly(|reader| reader.copy_to_inner_of(writer))
    })
  }

  /// [`Copies`][1] bytes from the inner reader and writer of both
  /// [`PositionTracker`]s until the reader reaches `SeekFrom::Start(offset)`.
  ///
  /// Equivalent to using [`self.take_from_inner_until`][2],
  /// [`TakeExt::exactly`] and [`reader.copy_to_inner_of(writer)`][3].
  ///
  /// [1]: copy
  /// [2]: Self::take_from_inner
  /// [3]: PositionTrackerReadExt::copy_to_inner_of
  pub fn copy_to_other_until(
    &mut self,
    offset: u64,
    writer: &mut PositionTracker<impl Write>,
  ) -> Result<u64> {
    self.take_from_inner_until(offset, |take| {
      take.exactly(|reader| reader.copy_to_inner_of(writer))
    })
  }

  /// Calls [`Read::take()`] on the inner reader and applies it to `f`.
  ///
  /// Generic code should prefer using this function over `self.take()`, since
  /// passing a `Take<PositionTracker<_>>` (or any other non-`std` reader or
  /// writer) to [`copy`] will prevent it from offloading file-to-file copies
  /// to the Linux kernel.
  ///
  /// # Examples
  /// ```
  /// use std::io::prelude::*;
  /// use std::io::{copy, sink, Cursor};
  /// use read_write_utils::seek::PositionTracker;
  ///
  /// let mut reader = PositionTracker::from_start(Cursor::new(vec![0u8, 1, 2, 3, 4]));
  ///
  /// let bytes_read = reader.take_from_inner(4, |take| copy(take, &mut sink()));
  /// assert_eq!(bytes_read.unwrap(), 4);
  /// assert_eq!(reader.position(), 4);
  ///
  /// let bytes_read = reader.take_from_inner(4, |take| copy(take, &mut sink()));
  /// assert_eq!(bytes_read.unwrap(), 1);
  /// assert_eq!(reader.position(), 5);
  /// ```
  pub fn take_from_inner<T, F>(&mut self, amt: u64, f: F) -> Result<T>
  where
    F: FnOnce(&mut Take<&mut R>) -> Result<T>,
  {
    let mut inner = (&mut self.inner).take(amt);
    let result = f(&mut inner);
    let num_read = amt - inner.limit();
    self.increment_position(num_read);
    result
  }

  /// Equivalent to [`self.take_from_inner(pos - self.position())`][1].
  ///
  /// # Errors
  /// This function returns [`InvalidInput`] if `pos < self.position()`.
  ///
  /// [1]: Self::take_from_inner
  ///
  /// # Examples
  /// ```
  /// use std::io::prelude::*;
  /// use std::io::{copy, Cursor};
  /// use read_write_utils::prelude::*;
  ///
  /// let mut reader = PositionTracker::from_start(Cursor::new(vec![0u8, 1, 2, 3, 4]));
  /// reader.seek_relative(1).unwrap();
  /// let mut output = Vec::<u8>::new();
  /// let bytes_copied = reader.take_from_inner_until(3, |take| {
  ///    copy(take, &mut output)
  /// }).unwrap();
  /// assert_eq!(bytes_copied, 2);
  /// assert_eq!(&output[..], &[1, 2]);
  /// ```
  pub fn take_from_inner_until<T, F>(&mut self, pos: u64, f: F) -> Result<T>
  where
    F: FnOnce(&mut Take<&mut R>) -> Result<T>,
  {
    match pos.checked_sub(self.position) {
      Some(amt) => self.take_from_inner(amt, f),
      None => Err(Error::from(InvalidInput)),
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
  /// used if the offset from the current position fits in an `i64`.
  /// * If the seek is relative to the current position, [`seek_relative`][2]
  /// is always used.
  ///
  /// [1]: Self::position
  /// [2]: Seek::seek_relative
  fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
    use SeekFrom::*;
    let relative_offset = match pos {
      Start(position) => position.checked_signed_difference(self.position),
      Current(offset) => Some(offset),
      _ => None,
    };
    match relative_offset {
      Some(offset) => self.seek_relative(offset)?,
      None => {
        self.position = self.inner.seek(pos)?;
      }
    }
    Ok(self.position)
  }

  /// Returns [`Ok(self.position())`](PositionTracker::position).
  fn stream_position(&mut self) -> Result<u64> {
    Ok(self.position())
  }

  /// Calls [`seek_relative`][1] on the inner stream and updates
  /// [`self.position()`][2].
  ///
  /// [1]: Seek::seek_relative
  /// [2]: Self::position
  fn seek_relative(&mut self, offset: i64) -> Result<()> {
    self.inner.seek_relative(offset)?;
    self.increment_position_signed(offset);
    Ok(())
  }
}

impl<W: Write> PositionTracker<W> {
  /// [Copies][1] from `reader` to this [`PositionTracker`]'s underlying writer.
  ///
  /// This function has a few benefits over calling `copy` directly:
  /// * It potentially enables `copy` to delegate file-to-file copies to the
  /// Linux kernel, which it can't do if the writer is a `PositionTracker`.
  /// * It allows the position to be updated only once.
  /// * It provides a fluent interface, which some people may find preferable.
  ///
  /// [1]: copy
  pub fn copy_to_inner_from(&mut self, reader: &mut (impl Read + ?Sized)) -> Result<u64> {
    let amount_copied = copy(reader, &mut self.inner)?;
    self.increment_position(amount_copied);
    Ok(amount_copied)
  }
}

impl<W: BufWrite> PositionTracker<W> {
  /// Performs tracked I/O operations on the inner stream of this [`BufWrite`].
  ///
  /// The writer will be [flushed](Write::flush) prior to calling `f`.
  /// After `f` returns, [`self.position`](Self::position) will be updated
  /// to match the `PositionTracker` of the inner stream.
  ///
  /// A noteworthy use case this function facilitates is reading from a file
  /// wrapped by a [`BufWriter`].
  pub fn with_bufwriter_inner<F, R>(&mut self, f: F) -> Result<R>
  where
    F: FnOnce(&mut PositionTracker<&mut W::Inner>) -> Result<R>,
  {
    self.flush()?;
    let mut inner_tracker =
      PositionTracker::with_known_position(self.position, self.inner.inner_mut());
    let result = f(&mut inner_tracker)?;
    self.position = inner_tracker.position();
    Ok(result)
  }
}

impl<R: Read> Read for PositionTracker<R> {
  fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
    let amount: usize = self.inner.read(buf)?;
    self.increment_position(amount);
    Ok(amount)
  }
}

impl<R: BufRead> BufRead for PositionTracker<R> {
  /// Calls [`fill_buf`](BufRead::fill_buf) on the inner reader.
  fn fill_buf(&mut self) -> Result<&[u8]> {
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
  fn write(&mut self, buf: &[u8]) -> Result<usize> {
    let amount = self.inner.write(buf)?;
    self.increment_position(amount);
    Ok(amount)
  }

  fn flush(&mut self) -> Result<()> {
    self.inner.flush()
  }
}

impl<W: BufWrite> BufWrite for PositionTracker<W> {
  type Inner = W::Inner;

  fn inner(&self) -> &Self::Inner {
    self.inner.inner()
  }

  /// Returns a mutable reference to [`self.inner()`][1].
  ///
  /// **Warning:** Performing I/O operations on the inner stream will cause its
  /// position to differ from the value calculated by this [`PositionTracker`].
  /// See [`PositionTracker::with_inner`] for a safer alternative.
  ///
  /// [1]: PositionTracker::inner
  /// [2]: PositionTracker::position
  fn inner_mut(&mut self) -> &mut Self::Inner {
    self.inner.inner_mut()
  }
}

impl<T> Deref for PositionTracker<T> {
  type Target = T;

  fn deref(&self) -> &Self::Target {
    &self.inner
  }
}

pub trait PositionTrackerReadExt: Read {
  /// Equivalent to [`PositionTracker::copy_to_inner_from`].
  ///
  /// The only advantage of this method is that the order the reader and writer
  /// is consistent with [`copy`].
  fn copy_to_inner_of(&mut self, writer: &mut PositionTracker<impl Write>) -> Result<u64> {
    writer.copy_to_inner_from(self)
  }
}

impl<R: Read> PositionTrackerReadExt for R {}
