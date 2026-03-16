use std::io;
use std::io::prelude::*;

/// A [reader] that yields the bytes in a slice infinitely, just as
/// [`io::repeat`] does for a single byte.
///
/// If the slice is 1 byte long, all reads delegate to [`io::repeat`].
///
///  # Examples
/// ```
/// # use std::io::prelude::*;
/// # use read_write_utils::repeat::RepeatSlice;
/// #
/// let mut repeat = RepeatSlice::new(&[1, 2, 3]);
/// let buf = &mut [0u8; 2][..];
/// let _ = repeat.read_exact(buf);
/// assert_eq!(buf, &[1, 2]);
/// let _ = repeat.read_exact(buf);
/// assert_eq!(buf, &[3, 1]);
/// let _ = repeat.read_exact(buf);
/// assert_eq!(buf, &[2, 3]);
/// ```
///
/// [reader]: Read
pub struct RepeatSlice<T> {
  cursor: io::Cursor<T>,
}

impl<T: AsRef<[u8]>> RepeatSlice<T> {
  /// Creates a new `RepeatSlice`.
  ///
  /// # Panics
  /// This function panics if the slice is empty.
  pub fn new(slice: T) -> Self {
    assert!(!slice.as_ref().is_empty());
    Self { cursor: io::Cursor::new(slice) }
  }

  /// Returns a reference to the slice.
  pub fn slice(&self) -> &[u8] {
    self.cursor.get_ref().as_ref()
  }

  fn wrap_position(&mut self) {
    self
      .cursor
      .set_position(self.cursor.position() % self.slice().len() as u64);
  }
}

impl<T: AsRef<[u8]>> Read for RepeatSlice<T> {
  fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
    if self.slice().len() == 1 {
      return io::repeat(self.slice()[0]).read(buf);
    }
    let read_amt = self.cursor.read(buf)?;
    self.wrap_position();
    Ok(read_amt)
  }
}

impl<T: AsRef<[u8]>> BufRead for RepeatSlice<T> {
  fn fill_buf(&mut self) -> io::Result<&[u8]> {
    self.cursor.fill_buf()
  }

  fn consume(&mut self, amt: usize) {
    self.cursor.consume(amt);
    self.wrap_position();
  }
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  #[should_panic]
  pub fn empty_slice_panics() {
    RepeatSlice::new(&[]);
  }

  #[test]
  pub fn test_single_byte_slice() -> io::Result<()> {
    let mut repeat = RepeatSlice::new(&[7]);
    let mut buf = [0u8; 8];
    // Reading from a single byte slice delegates to io::repeat(),
    // which fills the buffer in a single read.
    assert_eq!(repeat.read(&mut buf)?, 8);
    assert_eq!(&buf, &[7u8; 8]);
    Ok(())
  }

  #[test]
  pub fn test_slice_reads() -> io::Result<()> {
    let mut repeat = RepeatSlice::new(&[1, 2, 3]);
    let buf = &mut [0u8; 2][..];
    repeat.read_exact(buf)?;
    assert_eq!(buf, &[1, 2]);
    repeat.read_exact(buf)?;
    assert_eq!(buf, &[3, 1]);
    repeat.read_exact(buf)?;
    assert_eq!(buf, &[2, 3]);
    Ok(())
  }

  #[test]
  pub fn test_consume() -> io::Result<()> {
    let mut repeat = RepeatSlice::new(&[1, 2, 3]);
    let buf = &mut [0u8; 2][..];

    // Consumed bytes should get skipped.
    repeat.fill_buf()?;
    repeat.consume(2);
    repeat.read_exact(buf)?;
    assert_eq!(buf, &[3, 1]);

    // Consuming until EOF should reset the cursor.
    let remaining_amt = repeat.fill_buf()?.len();
    repeat.consume(remaining_amt);
    assert_ne!(repeat.fill_buf()?.len(), 0);
    repeat.read_exact(buf)?;
    assert_eq!(buf, &[1, 2]);
    Ok(())
  }
}
