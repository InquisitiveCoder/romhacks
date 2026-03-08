use crate::{HashingWriter, WriteHasher};
use read_write_utils::pos::PositionTracker;
use read_write_utils::prelude::*;
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
pub struct MonotonicHashingReader<R, H> {
  inner: PositionTracker<R>,
  hasher: PositionTracker<WriteHasher<H>>,
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
  R: Read + Seek,
  H: Hasher,
{
  fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
    self.read_and_hash(|inner| {
      let amt: usize = inner.read(buf)?;
      Ok(&buf[..amt])
    })
  }
}

impl<R, H> BufRead for MonotonicHashingReader<R, H>
where
  R: BufRead + Seek,
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
        self.seek_and_hash_to(position)?;
      }
      SeekFrom::Current(offset) => {
        self.seek_relative(offset)?;
      }
      SeekFrom::End(offset) => {
        // Hash as many bytes as possible before discarding the buffer.
        // The buffer might include the end of the stream, so don't read past
        // buf.len() + offset.
        let buf = self.inner.fill_buf()?;
        if let Some(safe_read_len) = (buf.len() as u64)
          .checked_add_signed(offset)
          .map(|x| std::cmp::min(x, buf.len() as u64))
          .and_then(|x| usize::try_from(x).ok())
        {
          debug_assert!(safe_read_len <= buf.len());
          let _ = self.hasher.write_all(&buf[..safe_read_len]);
          self.inner.consume(safe_read_len);
        }
        let position = self.inner.seek(pos)?;
        self.seek_and_hash_to(position)?;
      }
    }
    Ok(self.inner.position())
  }

  fn seek_relative(&mut self, offset: i64) -> io::Result<()> {
    let position = self.inner.position();
    let new_position = position.checked_add_signed(offset).ok_or(InvalidInput)?;
    self.seek_and_hash_to(new_position)
  }
}

impl<R, H> MonotonicHashingReader<R, H>
where
  R: Read + Seek,
  H: Hasher,
{
  fn read_and_hash<'a, 'b>(
    &'a mut self,
    read_or_consume: impl FnOnce(&'a mut PositionTracker<R>) -> io::Result<&'b [u8]>,
  ) -> io::Result<usize>
  where
    'a: 'b,
  {
    let starting_position = self.inner.position();
    let data: &[u8] = read_or_consume(&mut self.inner)?;
    let already_hashed_len: u64 = self.hasher.position() - starting_position;
    // If the conversion to usize fails, the inner stream is so far behind the
    // hasher that a single read can't catch up to the hasher's position.
    let unhashed_data: &[u8] = usize::try_from(already_hashed_len)
      .ok()
      .and_then(|hashed_len| data.split_at_checked(hashed_len))
      .map(|(_hashed, unhashed)| unhashed)
      .unwrap_or(&[]);
    self.hasher.write_all(unhashed_data)?;
    Ok(data.len())
  }

  fn seek_and_hash_to(&mut self, position: u64) -> io::Result<()> {
    let hasher_position = self.hasher.position();
    if position <= hasher_position {
      // Seeking to a position that's already been hashed, nothing to do but
      // pos the inner stream.
      self.inner.seek(SeekFrom::Start(position))?;
    } else {
      // Seeking to unhashed data.
      // Seek to the furthest hashed position, then read and hash until the
      // new position is reached.
      self.inner.seek(SeekFrom::Start(hasher_position))?;
      self
        .inner
        .take_from_inner_until(position, |inner| inner.copy_to_inner_of(&mut self.hasher))?;
    }
    Ok(())
  }
}
