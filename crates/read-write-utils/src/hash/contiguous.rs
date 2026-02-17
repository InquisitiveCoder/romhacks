use super::*;
use crate::seek::PositionTracker;
use std::io::ErrorKind::InvalidInput;
use std::io::SeekFrom;

/// A [`Read`] adapter that hashes every byte up to its underlying reader's
/// current position once and only once.
///
/// Specifically, if the reader's cursor is moved forward, the bytes between
/// the old and new positions will be hashed; if the cursor is moved backwards,
/// calling `read` or `consume` won't hash any bytes that occur prior to the
/// furthest hashed position.
///
/// This allows a byte stream to be hashed while doing other tasks, even if they
/// require occasional backward seeks. If the stream is only read strictly
/// sequentially, a [`HashingReader`] will accomplish the same thing with less
/// overhead.
pub struct MonotonicHashingReader<R, H> {
  inner: PositionTracker<R>,
  hasher: PositionTracker<HashingWriter<io::Sink, H>>,
}

impl<R, H> MonotonicHashingReader<R, H>
where
  R: Read + Seek,
  H: Hasher,
{
  pub fn new(inner: R, hasher: H) -> Self {
    let inner = PositionTracker::from_start(inner);
    let hasher = PositionTracker::from_start(HashingWriter::new(io::sink(), hasher));
    Self { inner, hasher }
  }

  pub fn from_parts(
    inner: PositionTracker<R>,
    hasher: PositionTracker<HashingWriter<io::Sink, H>>,
  ) -> Self {
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

  pub fn position(&self) -> u64 {
    self.hasher.position()
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
  R: BufRead + Seek,
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
  fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
    match pos {
      io::SeekFrom::Start(position) => {
        self.seek_and_hash_to(position)?;
      }
      io::SeekFrom::Current(offset) => {
        self.seek_relative(offset)?;
      }
      io::SeekFrom::End(offset) => {
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

impl<W: Write, H> Write for MonotonicHashingReader<W, H> {
  fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
    self.inner.write(buf)
  }

  fn flush(&mut self) -> io::Result<()> {
    self.inner.flush()
  }
}

impl<W: BufWrite, H> BufWrite for MonotonicHashingReader<W, H> {
  type Inner = PositionTracker<W>;

  fn inner(&self) -> &Self::Inner {
    &self.inner
  }

  fn inner_mut(&mut self) -> &mut Self::Inner {
    &mut self.inner
  }
}

impl<R, H> MonotonicHashingReader<R, H>
where
  R: BufRead + Seek,
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
    self.hasher.write(unhashed_data)?;
    Ok(data.len())
  }

  fn seek_and_hash_to(&mut self, position: u64) -> io::Result<()> {
    let hasher_position = self.hasher.position();
    if position <= hasher_position {
      // Seeking to a position that's already been hashed, nothing to do but
      // seek the inner stream.
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
