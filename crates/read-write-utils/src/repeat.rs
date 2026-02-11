use std::io;
use std::io::prelude::*;

/// A [reader] that yields the bytes in a slice infinitely, as [`Repeat`] does
/// for a single byte.
///
///  # Examples
/// ```
/// use std::io::prelude::*;
/// use read_write_utils::RepeatSlice;
///
/// let mut repeat = RepeatSlice::new(&[1, 2, 3]);
/// let buf = &mut [0u8; 2][..];
/// let _ = repeat.read(buf);
/// assert_eq!(buf, &[1, 2]);
/// let _ = repeat.read(buf);
/// assert_eq!(buf, &[3, 1]);
/// let _ = repeat.read(buf);
/// assert_eq!(buf, &[2, 3]);
/// ```
///
/// [reader]: Read
/// [Repeat]: [std::read-write-utils::Repeat]
pub struct RepeatSlice<'a> {
  cursor: io::Cursor<&'a [u8]>,
}

impl<'a> RepeatSlice<'a> {
  /// Returns a new reader.
  ///
  /// # Panics
  /// This function panics if `slice.len() == 0`.
  pub fn new(slice: &'a [u8]) -> Self {
    assert!(slice.len() > 1);
    Self { cursor: io::Cursor::new(slice) }
  }

  pub fn slice(&self) -> &[u8] {
    self.cursor.get_ref()
  }
}

impl Read for RepeatSlice<'_> {
  fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
    if self.slice().len() == 1 {
      return io::repeat(self.slice()[0]).read(buf);
    }

    loop {
      self.cursor.read(buf)?;
      if self.cursor.position() == self.slice().len() as u64 {
        self.cursor.set_position(0);
      }
      if buf.is_empty() {
        return Ok(buf.len());
      }
    }
  }
}

impl BufRead for RepeatSlice<'_> {
  fn fill_buf(&mut self) -> io::Result<&[u8]> {
    self.cursor.fill_buf()
  }

  fn consume(&mut self, amt: usize) {
    self.cursor.consume(amt)
  }
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  pub fn test_repeat_slice() -> io::Result<()> {
    let mut repeat = RepeatSlice::new(&[1, 2, 3]);
    let buf = &mut [0u8; 2][..];
    repeat.read(buf)?;
    assert_eq!(buf, &[1, 2]);
    repeat.read(buf)?;
    assert_eq!(buf, &[3, 1]);
    repeat.read(buf)?;
    assert_eq!(buf, &[2, 3]);
    Ok(())
  }
}
