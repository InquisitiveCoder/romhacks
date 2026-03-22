use crate::{HashingWriter, WriteHasher};
use read_write_utils::prelude::*;
use read_write_utils::seek::SeekRelative;
use std::hash::Hasher;
use std::io;
use std::io::prelude::*;
use std::io::ErrorKind::InvalidInput;
use std::io::SeekFrom;

/// A [`Read`] adapter that hashes every byte up to its underlying reader's
/// current position once and only once.
///
/// A `MonotonicHashingReader` remembers the furthest stream position that has
/// been reached since it was created. Whenever the adapter's position advances,
/// only the bytes that occur after that position will be hashed.  This behavior
/// facilitates hashing a reader while processing its contents in a mostly
/// sequential manner, without sacrificing the ability to seek back and forth.
///
/// If the stream is read strictly sequentially, a [`HashingReader`][1] will
/// accomplish the same thing with less overhead.
///
/// [1]: crate::HashingReader
// Invariant: inner.position() <= hasher.position()
pub struct MonotonicHashingReader<R, H> {
  inner: PositionTracker<R>,
  hasher: PositionTracker<WriteHasher<H>>,
}

impl<R, H> MonotonicHashingReader<R, H> {
  #[inline]
  fn debug_assert_invariants(&self) {
    debug_assert!(self.inner.position() <= self.hasher.position());
  }
}

impl<R: Read, H: Hasher> MonotonicHashingReader<R, H> {
  /// Creates a new `MonotonicHashingReader`. The inner reader's cursor *must*
  /// be at the start of the stream in order for the furthest hashed position
  /// to be tracked accurately.
  ///
  /// [1]: Seek::stream_position
  pub fn from_start(inner: R, hasher: H) -> Self {
    let inner = PositionTracker::from_start(inner);
    let hasher = PositionTracker::from_start(WriteHasher::from(hasher));
    Self { inner, hasher }
  }

  pub fn from_parts(inner: PositionTracker<R>, hasher: PositionTracker<WriteHasher<H>>) -> Self {
    Self { inner, hasher }
  }
}

impl<R, H> MonotonicHashingReader<R, H> {
  pub fn inner(&self) -> &PositionTracker<R> {
    &self.inner
  }

  pub fn hasher(&self) -> &H {
    self.hasher.inner().hasher()
  }

  pub fn into_parts(
    self,
  ) -> (
    PositionTracker<R>,
    PositionTracker<HashingWriter<io::Sink, H>>,
  ) {
    (self.inner, self.hasher)
  }
}

impl<R, H> Read for MonotonicHashingReader<R, H>
where
  R: Read,
  H: Hasher,
{
  fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
    self.read_and_hash(|inner| {
      let amt: usize = inner.read(buf)?;
      Ok(&buf[..amt])
    })
  }
}

impl<R, H> AmortizedRead for MonotonicHashingReader<R, H>
where
  R: Read,
  H: Hasher,
{
}

impl<R, H> BufRead for MonotonicHashingReader<R, H>
where
  R: BufRead,
  H: Hasher,
{
  fn fill_buf(&mut self) -> io::Result<&[u8]> {
    self.inner.fill_buf()
  }

  fn consume(&mut self, amt: usize) {
    let amt = self
      .read_and_hash(|inner| inner.fill_buf().map(|buf| &buf[..amt]))
      .unwrap(); // This unwrap() is safe provided the caller called fill_buf().
    self.inner.consume(amt);
  }
}

impl<R: BufRead + Seek, H: Hasher> Seek for MonotonicHashingReader<R, H> {
  fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
    match pos {
      SeekFrom::Start(position) => {
        self.seek_and_hash_to(position, Seek::seek)?;
      }
      SeekFrom::Current(offset) => {
        self.seek_relative(offset)?;
      }
      SeekFrom::End(offset) => {
        // It's not possible to determine the seek position up front in this
        // case, but the relative offset to the destination can't be smaller
        // than fill_buf().len() + offset. Therefore, it's still possible to
        // hash some of the buffered data before it's discarded by the seek.
        let buf = self.inner.fill_buf()?;
        let safe_read_len = u64::checked_add_signed(buf.len() as u64, offset)
          .map(|x| usize::try_from(x).ok().unwrap_or(usize::MAX))
          .map(|x| usize::min(x, buf.len()))
          // In the case of overflow, clamp the length to 0 or buf.len().
          .unwrap_or_else(|| usize::from(offset.is_positive()) * buf.len());
        debug_assert!(safe_read_len <= buf.len());
        let _ = self.hasher.write_all(&buf[..safe_read_len]);
        self.inner.consume(safe_read_len);
        let position = self.inner.seek(pos)?;
        self.seek_and_hash_to(position, Seek::seek)?;
      }
    }
    Ok(self.inner.position())
  }

  fn seek_relative(&mut self, offset: i64) -> io::Result<()> {
    self.seek_relative_and_hash_to(offset, Seek::seek_relative)
  }
}

impl<S, H> SeekRelative for MonotonicHashingReader<S, H>
where
  S: Read + SeekRelative,
  H: Hasher,
{
  fn relative_seek(&mut self, offset: i64) -> io::Result<()> {
    self.seek_relative_and_hash_to(offset, SeekRelative::relative_seek)
  }
}

impl<R, H> MonotonicHashingReader<R, H>
where
  R: Read,
  H: Hasher,
{
  /// [Read][1] or [consume][2] from `self.inner` and hash any bytes that occur
  /// after `self.hasher.position()`.
  ///
  /// [1]: Read::read
  /// [2]: BufRead::consume
  fn read_and_hash<'a, 'b, F>(&'a mut self, read_fn: F) -> io::Result<usize>
  where
    'a: 'b,
    F: FnOnce(&'a mut PositionTracker<R>) -> io::Result<&'b [u8]>,
  {
    self.debug_assert_invariants();
    // read_fn may or may not advance the inner reader's position, so the
    // initial position must be stored prior to calling it.
    let starting_position = self.inner.position();
    let data: &[u8] = read_fn(&mut self.inner)?;
    let hasher_offset: u64 = self.hasher.position() - starting_position;
    // If the conversion to usize fails, the inner stream is so far behind the
    // hasher that a single read can't catch up to the hasher's position.
    let unhashed_data: &[u8] = usize::try_from(hasher_offset)
      .ok()
      .and_then(|hasher_offset| data.get(hasher_offset..))
      .unwrap_or(&[]);
    self.hasher.write_all(unhashed_data)?;
    Ok(data.len())
  }
}

impl<R, H> MonotonicHashingReader<R, H>
where
  R: Read,
  H: Hasher,
{
  fn seek_and_hash_to<F>(&mut self, position: u64, seek: F) -> io::Result<()>
  where
    F: FnOnce(&mut PositionTracker<R>, SeekFrom) -> io::Result<u64>,
  {
    self.debug_assert_invariants();
    seek(
      &mut self.inner,
      SeekFrom::Start(u64::min(position, self.hasher.position())),
    )?;
    self.inner.copy_to_inner_until(position, &mut self.hasher)?;
    self.debug_assert_invariants();
    Ok(())
  }

  fn seek_relative_and_hash_to<F>(&mut self, offset: i64, seek_relative: F) -> io::Result<()>
  where
    F: FnOnce(&mut PositionTracker<R>, i64) -> io::Result<()>,
  {
    self.debug_assert_invariants();
    let new_position =
      u64::checked_add_signed(self.inner.position(), offset).ok_or(InvalidInput)?;
    let hasher_offset: i64 =
      u64::checked_signed_diff(self.hasher.position(), self.inner.position())
        .ok_or(InvalidInput)?;
    seek_relative(&mut self.inner, i64::min(offset, hasher_offset))?;
    self
      .inner
      .copy_to_inner_until(new_position, &mut self.hasher)?;
    self.debug_assert_invariants();
    Ok(())
  }
}
