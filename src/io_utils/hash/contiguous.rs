use super::*;
use std::arch::x86_64::_mm256_undefined_si256;
use std::io::ErrorKind::InvalidInput;

/// A [`Read`] adapter that hashes every byte up to its underlying reader's
/// current position once and only once.
///
/// Specifically, if the reader's cursor is moved forward, the bytes between
/// the old and new positions will be hashed; if the cursor is moved backwards,
/// calling `read` won't hash any bytes that occur prior to the previous
/// position, since they've already been hashed.
///
/// This allows a byte source to be hashed while doing work that requires
/// occasional seeks back and forth. If the stream is only read strictly
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

  pub fn hash_remainder(&mut self) -> io::Result<u64> {
    io::copy(&mut self.inner, &mut self.hasher)
  }
}

impl<R, H> MonotonicHashingReader<R, H> {
  pub fn inner(&self) -> &PositionTracker<R> {
    &self.inner
  }

  pub fn hasher(&self) -> &PositionTracker<HashingWriter<io::Sink, H>> {
    &self.hasher
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

impl<R, H> MonotonicHashingReader<R, H>
where
  R: BufRead + Seek,
  H: Hasher,
{
  fn read_or_consume<'a>(
    &'a mut self,
    action: impl FnOnce(&mut PositionTracker<R>) -> io::Result<&'a [u8]>,
  ) -> io::Result<usize> {
    let stream_position: u64 = self.inner.position();
    let hasher_position: u64 = self.hasher.position();
    let abs_diff: u64 = stream_position.abs_diff(hasher_position);
    let is_hasher_behind_stream = hasher_position < stream_position;
    if is_hasher_behind_stream {
      // Seek back to the hasher position and hash the gap
      self.inner.seek(io::SeekFrom::Start(hasher_position))?;
      (&mut self.inner)
        .take(abs_diff)
        .consuming_copy(&mut self.hasher)?;
    }
    let data: &[u8] = action(&mut self.inner)?;
    let is_stream_behind_hasher = !is_hasher_behind_stream;
    let already_hashed_len: u64 = abs_diff * is_stream_behind_hasher as u64;
    // If the conversion to usize fails, the stream is so far behind the hasher
    // that a single read can't exceed the hasher's position.
    let unhashed_data: &[u8] = usize::try_from(already_hashed_len)
      .ok()
      .and_then(|overlap| data.split_at_checked(overlap))
      .map(|(_hashed, unhashed)| unhashed)
      .unwrap_or(&[]);
    // Writing to the hasher is infallible.
    let _ = self.hasher.write(unhashed_data);
    Ok(data.len())
  }

  fn consume_and_hash(&mut self, amount: u64) -> io::Result<u64> {
    (&mut self.inner)
      .take(amount)
      .consuming_copy(&mut self.hasher)
  }
}

impl<R, H> Read for MonotonicHashingReader<R, H>
where
  R: BufRead + Seek,
  H: Hasher,
{
  fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
    self.read_or_consume(|inner| {
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
    let amt = self.read_or_consume(|inner| inner.fill_buf());
    (&mut self.inner).consume(amt)
  }
}

impl<R: BufRead + Seek, H: Hasher> Seek for MonotonicHashingReader<R, H> {
  fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
    match pos {
      io::SeekFrom::Start(offset) => {
        if offset <= self.hasher.position() {
          self.inner.seek(io::SeekFrom::Start(offset))?;
        } else {
          (&mut self.inner).copy_until_position(offset, &mut self.hasher)?;
        }
      }
      io::SeekFrom::Current(offset) => {
        self.seek_relative(offset)?;
      }
      io::SeekFrom::End(_) => {
        let starting_position = self.inner.position();
        let new_position = self.inner.seek(pos)?;
      }
    }
    Ok(self.inner.position())
  }

  fn seek_relative(&mut self, offset: i64) -> io::Result<()> {
    let new_position = self
      .inner
      .position()
      .checked_add_signed(offset)
      .ok_or(InvalidInput)?;
    if new_position <= self.hasher.position() {
      return self.inner.seek_relative(offset);
    }
    let not_hashed_len = new_position - self.hasher.position();
    (&mut self.inner)
      .take(not_hashed_len)
      .consuming_copy(&mut self.hasher)?;
    Ok(())
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

impl<W: BufWrite, H> BufWrite for MonotonicHashingReader<W, H> {}
