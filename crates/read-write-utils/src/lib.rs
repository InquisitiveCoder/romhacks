pub mod prelude;

pub mod pos;

pub mod read;
pub mod repeat;
mod write;

/// The buffer size constant used internally by `std::io` since Rust 1.9.0,
/// copied verbatim.
pub const DEFAULT_BUF_SIZE: usize = if cfg!(target_os = "espidf") { 512 } else { 8 * 1024 };

/// Calls [`peek`][1] on a reader and compares the result to a `const` slice.
/// The buffer argument for `peek` is an array of matching size.
///
/// # Examples
/// ```
/// use std::io::Cursor;
/// use std::io;
/// use std::io::ErrorKind::UnexpectedEof;
/// use read_write_utils::prelude::*;
///
/// let mut reader = Cursor::new([0u8, 1, 2, 3, 4]);
///
/// const MAGIC_STRING: &[u8] = &[0, 1, 2];
/// assert!(peek_eq!(reader, MAGIC_STRING)?);
/// assert_eq!(reader.position(), 0);
///
/// // slice literals work too
/// assert!(!peek_eq!(reader, &[4, 5, 6])?);
/// assert_eq!(reader.position(), 0);
///
/// // reader.peek() doesn't fail with UnexpectedEof
/// reader.set_position(3);
/// assert!(!peek_eq!(reader, &[4, 5, 6])?);
/// # Ok::<(), std::io::Error>(())
/// ```
/// [1]: prelude::BufReadExt::peek
#[macro_export]
macro_rules! peek_eq {
  ($reader:expr, $const_slice:expr) => {{
    use read_write_utils::prelude::*;
    let mut buf = [0u8; $const_slice.len()];
    $reader.peek(&mut buf).map(|slice| slice == $const_slice)
  }};
}

/// Negates the result of [`peek_eq!`](crate::prelude::peek_eq).
#[macro_export]
macro_rules! peek_ne {
  ($reader:expr, $const_slice:expr) => {
    peek_eq!($reader, $const_slice).map(|x| !x)
  };
}

/// Calls [`read_n`][1] on a reader and compares its bytes to a `const`
/// slice. The array size is obtained from the slice's length.
///
/// # Examples
/// ```
/// use std::io::Cursor;
/// use std::io;
/// use std::io::ErrorKind::UnexpectedEof;
/// use read_write_utils::prelude::*;
///
/// let mut reader = Cursor::new([0u8, 1, 2, 3, 4]);
///
/// const MAGIC_STRING: &[u8] = &[0, 1, 2];
/// assert!(read_array_eq!(reader, MAGIC_STRING)?);
/// assert_eq!(reader.position(), 3);
///
/// // slice literals work too
/// assert_eq!(
///   read_array_eq!(reader, &[4, 5, 6]).map_err(|err| err.kind()),
///   Err(UnexpectedEof)
/// );
/// assert_eq!(reader.position(), 5);
/// # Ok::<(), std::io::Error>(())
/// ```
///
/// [1]: prelude::ReadExt::read_n
#[macro_export]
macro_rules! read_array_eq {
  ($reader:expr, $const_slice:expr) => {{
    use read_write_utils::prelude::*;
    $reader
      .read_n::<{ $const_slice.len() }>()
      .map(|array| &array[..] == $const_slice)
  }};
}

/// Negates the result of [`read_array_eq!`].
#[macro_export]
macro_rules! read_array_ne {
  ($reader:expr, $const_slice:expr) => {
    read_array_eq!($reader, $const_slice).map(|x| !x)
  };
}
