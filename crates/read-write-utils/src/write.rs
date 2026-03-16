use crate::DEFAULT_BUF_SIZE;
use std::collections::VecDeque;
use std::io;
use std::io::prelude::*;

pub trait WriteExt: Write {
  /// Wraps `self` in a `BufWriter` with 1.5 times the default buffer size.
  ///
  /// This works around a bug in [`std::io::copy`] that flushes the `BufWriter` if its
  /// remaining capacity after a `write` falls below the default buffer size.
  fn buffer_writes(self) -> io::BufWriter<Self>
  where
    Self: Sized,
  {
    io::BufWriter::with_capacity(DEFAULT_BUF_SIZE * 3 / 2, self)
  }
}

/// Writers that have an internal buffer or don't perform I/O.
///
/// This trait indicates that a writer is suitable for small and repeated
/// writes, as explained in the documentation for [`BufWriter`][1]. By adding
/// this bound to generic code, you don't have to defensively wrap writers in a
/// possibly redundant [`BufWriter`][1], nor force a specific buffer size on
/// consumers of your code.
///
/// [1]: io::BufWriter
pub trait BufWrite: Write {}

/// [`BufWrite`] implementations that support reading from their underlying
/// stream.
pub trait AsRead: BufWrite {
  /// Returns a mutable reference to the underlying writer.
  ///
  /// This method is intended for cases where the inner writer has capabilities
  /// that `self` doesn't (e.g. [`Read`].) Writing directly to the inner writer
  /// without calling [`flush`][1] is likely to result in the data being
  /// written in an unintended order.
  ///
  /// [1]: Write::flush
  fn as_read(&mut self) -> io::Result<&mut dyn Read>;
}

impl BufWrite for &mut [u8] {}

impl BufWrite for io::Cursor<&mut [u8]> {}
impl AsRead for io::Cursor<&mut [u8]> {
  fn as_read(&mut self) -> io::Result<&mut dyn Read> {
    self.flush()?;
    Ok(self)
  }
}

impl BufWrite for io::Empty {}
impl BufWrite for io::Sink {}
impl BufWrite for io::StderrLock<'_> {}
impl BufWrite for io::StdoutLock<'_> {}

impl BufWrite for io::Cursor<&mut Vec<u8>> {}
impl AsRead for io::Cursor<&mut Vec<u8>> {
  fn as_read(&mut self) -> io::Result<&mut dyn Read> {
    self.flush()?;
    Ok(self)
  }
}

impl BufWrite for io::Cursor<Box<[u8]>> {}
impl AsRead for io::Cursor<Box<[u8]>> {
  fn as_read(&mut self) -> io::Result<&mut dyn Read> {
    self.flush()?;
    Ok(self)
  }
}

impl BufWrite for io::Cursor<Vec<u8>> {}
impl AsRead for io::Cursor<Vec<u8>> {
  fn as_read(&mut self) -> io::Result<&mut dyn Read> {
    self.flush()?;
    Ok(self)
  }
}

impl BufWrite for VecDeque<u8> {}
impl AsRead for VecDeque<u8> {
  fn as_read(&mut self) -> io::Result<&mut dyn Read> {
    self.flush()?;
    Ok(self)
  }
}

impl BufWrite for Vec<u8> {}

impl<W: Write> BufWrite for io::BufWriter<W> {}

impl<I: Read + Write> AsRead for io::BufWriter<I> {
  fn as_read(&mut self) -> io::Result<&mut dyn Read> {
    self.flush()?;
    Ok(self.get_mut())
  }
}

impl<const N: usize> BufWrite for io::Cursor<[u8; N]> {}
impl<const N: usize> AsRead for io::Cursor<[u8; N]> {
  fn as_read(&mut self) -> io::Result<&mut dyn Read> {
    self.flush()?;
    Ok(self)
  }
}

impl<W: BufWrite> BufWrite for Box<W> {}
impl<W: AsRead> AsRead for Box<W> {
  fn as_read(&mut self) -> io::Result<&mut dyn Read> {
    self.flush()?;
    self.as_mut().as_read()
  }
}

impl<W: BufWrite> BufWrite for &mut W {}
impl<W: AsRead> AsRead for &mut W {
  fn as_read(&mut self) -> io::Result<&mut dyn Read> {
    self.flush()?;
    <W as AsRead>::as_read(self)
  }
}
