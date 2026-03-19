use std::io;
use std::io::prelude::*;

/// Readers that support relative seeks, but may not support seeks relative to
/// the start or end of the stream (e.g. [io::Repeat]).
///
/// Prefer using [`SeekRelative`] as a trait bound if you don't need access to
/// [`Seek::seek`].
pub trait SeekRelative {
  /// Seeks relative to the current position. If a type also implements [`Seek`],
  /// this method must behave identically to [Seek::seek_relative].
  fn seek_relative(&mut self, offset: i64) -> io::Result<()>;
}

macro_rules! impl_with_seek {
  () => {
    fn seek_relative(&mut self, offset: i64) -> ::std::io::Result<()> {
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
  fn seek_relative(&mut self, _offset: i64) -> io::Result<()> {
    Ok(())
  }
}

impl SeekRelative for io::Sink {
  fn seek_relative(&mut self, _offset: i64) -> io::Result<()> {
    Ok(())
  }
}

impl<S> SeekRelative for &mut S
where
  S: SeekRelative + ?Sized,
{
  fn seek_relative(&mut self, offset: i64) -> io::Result<()> {
    (*self).seek_relative(offset)
  }
}
