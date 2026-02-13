use crate::prelude::{BufWrite, TakeExt};
use checked_signed_diff::prelude::*;
use std::io::ErrorKind::*;
use std::io::*;
use std::ops::Deref;

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
  pub fn new(mut inner: S) -> Result<Self> {
    let position = inner.stream_position()?;
    Ok(Self::with_known_position(position, inner))
  }
}

impl<T> PositionTracker<T> {
  /// Equivalent to [`PositionTracker::from_known_position(0, inner)`][1].
  ///
  /// [1]: Self::with_known_position
  pub fn from_start(inner: T) -> Self {
    Self { inner, position: 0 }
  }

  /// Creates a `PositionTracker` initialized to the given seek position.
  /// It's the caller's responsibility to ensure `position` matches `inner`'s
  /// seek position.
  ///
  /// If the stream's position isn't known, use [`from_unknown_position`][1].
  ///
  /// [1]: Self::new
  pub fn with_known_position(position: u64, inner: T) -> Self {
    Self { inner, position }
  }

  /// The underlying stream's seek position.
  pub fn position(&self) -> u64 {
    self.position
  }

  /// Gets a reference to the underlying reader.
  pub fn get_ref(&self) -> &T {
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

  pub fn map_inner<R>(self, f: impl FnOnce(T) -> R) -> PositionTracker<R> {
    PositionTracker::<R>::with_known_position(self.position, f(self.inner))
  }

  pub fn with_inner<F, G, R, O>(&mut self, f: F, g: G) -> Result<O>
  where
    R: ?Sized,
    F: FnOnce(&mut T) -> &mut R,
    G: FnOnce(&mut PositionTracker<&mut R>) -> Result<O>,
  {
    let mut inner_tracker = PositionTracker::with_known_position(self.position, f(&mut self.inner));
    g(&mut inner_tracker)
  }

  fn increment_position(&mut self, amt: u64) {
    self.position = self.position.checked_add(amt).unwrap();
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

impl<S: BufRead + Seek> PositionTracker<S> {
  /// Calls either [`seek_relative`][1] or [`seek`][2] on the underlying reader
  /// and updates [`Self::position`][3].
  ///
  /// Specifically, `seek` will be used iff `offset` is positive and greater
  /// than the length of the underlying reader's buffer. This strategy avoids
  /// discarding the buffer unnecessarily, while allowing the stream position to
  /// be tracked reliably even when seeking past EOF. However, it should be
  /// noted that it causes unnecessary refills when this function is called with
  /// a large positive offset while the buffer is empty; the call to
  /// [`BufRead::fill_buf`] will refill the buffer, but the out-of-bounds seek
  /// will cause it to be discarded.
  ///
  /// [1]: Seek::seek_relative
  /// [2]: Seek::seek
  /// [3]: Self::position
  fn seek_relative_accurate(&mut self, offset: i64) -> Result<()> {
    // For backward seeks, we can track the position reliably since negative
    // overflow is forbidden.
    if offset <= 0 {
      self.inner.seek_relative(offset)?;
      // The inner reader will check for negative overflow
      self.position = self.position.wrapping_add_signed(offset);
      return Ok(());
    }

    // For forward seeks, the inner reader may accept a seek past EOF and
    // desynchronize our calculated position from the reader. However, if the
    // seek doesn't fall within the buffer, calling inner.seek_relative()
    // would've discarded the buffer, so we can call inner.seek() and use the
    // new position reported by inner. This does run the risk of occasionally
    // triggering an unnecessary buffer refill, but there's no other way to
    // reliably track the position.
    let offset_is_in_buffer = usize::try_from(offset)
      .ok()
      .map(|offset| {
        self
          .inner
          .fill_buf()
          .map(|buf| buf.split_at_checked(offset).is_some())
      })
      .transpose()?
      .unwrap_or(false);
    if offset_is_in_buffer {
      self.inner.seek_relative(offset)?;
      self.position = self.position.wrapping_add_signed(offset);
    } else {
      self.position = self.inner.seek(SeekFrom::Current(offset))?;
    }
    Ok(())
  }
}

impl<S: Seek> Seek for PositionTracker<S> {
  /// Seeks to the given position in the underlying stream and updates
  /// [`Self::position`][1] if it succeeds.
  ///
  /// This function will call [`Self::seek_relative`] whenever possible to make
  /// it easier to take advantage of its performance benefits.
  /// Specifically:
  /// * If the seek is relative to the start of the stream, [`Self::position`]
  /// will be used to calculate the offset from the current position. If the
  /// offset fits in an `i64`, `Self::seek_relative` will be used.
  /// * If the seek is relative to the current position, `Self::seek_relative`
  /// is always used.
  /// * If the seek is relative to the end of the stream, `Self::seek_relative`
  /// will never be used, since the offset can't be calculated from
  /// `Self::position` alone.
  ///
  /// [1]: Self::position
  /// [2]: Seek::seek_relative
  /// [3]: Seek::seek
  fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
    use SeekFrom::*;
    if let Some(offset) = match pos {
      Start(position) => position.checked_signed_difference(self.position),
      Current(offset) => Some(offset),
      _ => None,
    } {
      self.seek_relative(offset)?;
    } else {
      self.position = self.inner.seek(pos)?;
    }
    Ok(self.position)
  }

  /// Returns [`PositionTracker::position()`].
  fn stream_position(&mut self) -> Result<u64> {
    self.position = self.inner.stream_position()?;
    Ok(self.position)
  }

  /// Calls either [`seek_relative`][1] or [`seek`][2] on the underlying reader
  /// and updates [`Self::position`][3].
  ///
  /// Specifically, `seek` will be used iff `offset` is positive and greater
  /// than the length of the underlying reader's buffer. This strategy avoids
  /// discarding the buffer unnecessarily, while allowing the stream position to
  /// be tracked reliably even when seeking past EOF. However, it should be
  /// noted that it causes unnecessary refills when this function is called with
  /// a large positive offset while the buffer is empty; the call to
  /// [`BufRead::fill_buf`] will refill the buffer, but the out-of-bounds seek
  /// will cause it to be discarded.
  ///
  /// [1]: Seek::seek_relative
  /// [2]: Seek::seek
  /// [3]: Self::position
  fn seek_relative(&mut self, offset: i64) -> Result<()> {
    // The inner reader will check for overflow.
    self.inner.seek_relative(offset)?;
    self.position = self.position.wrapping_add_signed(offset);
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
  pub fn with_bufwrite_inner<F, R>(&mut self, f: F) -> Result<R>
  where
    F: FnOnce(&mut PositionTracker<&mut W::Inner>) -> Result<R>,
  {
    self.flush()?;
    let mut inner_tracker =
      PositionTracker::with_known_position(self.position, self.inner.get_mut());
    let result = f(&mut inner_tracker)?;
    self.position = inner_tracker.position();
    Ok(result)
  }
}

impl<R: Read> Read for PositionTracker<R> {
  fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
    let amount = self.inner.read(buf)?;
    self.position += amount as u64;
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
    self.position += amt as u64;
  }
}

impl<W: Write> Write for PositionTracker<W> {
  fn write(&mut self, buf: &[u8]) -> Result<usize> {
    let amount = self.inner.write(buf)?;
    self.position += amount as u64;
    Ok(amount)
  }

  fn flush(&mut self) -> Result<()> {
    self.inner.flush()
  }
}

impl<W: BufWrite> BufWrite for PositionTracker<W> {
  type Inner = W::Inner;

  fn get_ref(&self) -> &Self::Inner {
    self.inner.get_ref()
  }

  fn get_mut(&mut self) -> &mut Self::Inner {
    self.inner.get_mut()
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
  fn copy_to_inner_of(&mut self, writer: &mut PositionTracker<impl Write>) -> Result<u64> {
    writer.copy_to_inner_from(self)
  }
}

impl<R: Read> PositionTrackerReadExt for R {}
