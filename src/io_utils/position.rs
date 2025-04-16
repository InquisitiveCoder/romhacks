use crate::io_utils::prelude::*;
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

  pub fn map_inner<R>(self, f: impl FnOnce(T) -> R) -> PositionTracker<R> {
    PositionTracker::<R>::with_known_position(self.position, f(self.inner))
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
    self.position = self.position.checked_add(num_copied).unwrap();
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
    writer.position = writer.position.checked_add(num_copied).unwrap();
    Ok(num_copied)
  }

  /// Equivalent to `(&mut self).take(pos - self.position())`.
  ///
  /// # Errors
  /// This function returns [`InvalidInput`] if `pos < self.position()`.
  pub fn until_position(&mut self, pos: u64) -> Result<Take<&mut Self>> {
    match pos.checked_sub(self.position) {
      Some(amt) => Ok(self.take(amt)),
      None => Err(Error::from(InvalidInput)),
    }
  }
}

impl<S: BufRead + Seek> Seek for PositionTracker<S> {
  /// Seeks to the given position in the underlying stream and updates
  /// [`Self::position`][1] if it succeeds.
  ///
  /// This function will call [`Self::seek_relative`] whenever possible in order
  /// to make it easier to take advantage of its performance benefits.
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
    let offset_from_current_position = match pos {
      Start(position) => i64::try_from(position.wrapping_sub(self.position)).ok(),
      Current(offset) => Some(offset),
      _ => None,
    };
    if let Some(offset) = offset_from_current_position {
      self.seek_relative(offset)?;
    } else {
      self.position = self.inner.seek(pos)?;
    }
    Ok(self.position)
  }

  /// Returns [`PositionTracker::position()`].
  fn stream_position(&mut self) -> Result<u64> {
    Ok(self.position())
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
    // For backward seeks, we can track the position reliably since negative
    // overflow is forbidden.
    if offset <= 0 {
      self.inner.seek_relative(offset)?;
      // The inner reader will check for negative overflow
      self.position = self.position.wrapping_add_signed(offset);
      return Ok(());
    }

    // For forward seeks, the inner reader may accept a seek past EOF and
    // desync our calculated position from the reader.
    // However, we can check if the seek falls within the buffer;
    // if it doesn't, there's no harm in calling inner.seek() instead of
    // inner.seek_relative(), and we can use the new position reported by inner.
    // This does run the risk of occasionally triggering an unnecessary buffer
    // refill, but there's no other way to reliably track the position.
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

impl<R: Read> Read for PositionTracker<R> {
  fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
    let amount = self.inner.read(buf)?;
    self.position += amount as u64;
    Ok(amount)
  }
}

impl<B: BufRead> BufRead for PositionTracker<B> {
  fn fill_buf(&mut self) -> Result<&[u8]> {
    self.inner.fill_buf()
  }

  /// Calls [`BufRead::consume`] on the underlying reader.
  ///
  /// # Panics
  /// This function panics if `self.position() + amt` overflows.
  fn consume(&mut self, amt: usize) {
    self.position = self.position.checked_add(amt as u64).unwrap();
    self.inner.consume(amt);
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

impl<T> Deref for PositionTracker<T> {
  type Target = T;

  fn deref(&self) -> &Self::Target {
    &self.inner
  }
}
