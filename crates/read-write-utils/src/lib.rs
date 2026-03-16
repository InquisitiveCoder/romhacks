//! # Read/Write Utils
//!
//! This crate fills gaps in `std::io`, with a focus on avoiding unnecessary
//! system calls and taking full advantage of buffered I/O.
//!
//! The cornerstone of this crate is the [`PositionTracker`][1] adapter, which
//! tracks a reader or writer's stream position as you perform I/O operations.
//! Among other things, this makes it much easier to use [`seek_relative`][2]
//! to avoid pre-maturely discarding a [`BufReader`][3]'s internal buffer.
//!
//! Other common tasks that this crate addresses:
//! * [`exactly`][4] asserts that the expected number of bytes were read.
//! * [`peek`][5] and [`peek_len`][6] provide efficient ways to look ahead into
//!   any buffered reader.
//! * [`read_n`][7] is useful for one-off reads.
//! * [`copy_to_slice`][8] fills up a slice as much as possible
//! * [`reached_eof`][9] expresses intent more clearly than
//!   `fill_buf()?.is_empty()`.
//! * [`if_not_eof`][10] provides a way to handle optional data at the end of a
//!   reader.
//! * The [`BufWrite`][11] trait can be used to ensure consumers of your code
//!   provide a writer that's suitable for small, frequent writes.
//! * [`RepeatSlice`][12] provides a more general version of [`io::repeat`][13].
//!
//! Simply `use read_write_utils::prelude::*` to import all traits, macros, and
//! [`PositionTracker`][1].
//!
//! [1]: prelude::PositionTracker
//! [2]: ::std::io::Seek::seek_relative
//! [3]: ::std::io::BufReader
//! [4]: prelude::TakeExt::exactly
//! [5]: prelude::BufReadExt::peek
//! [6]: prelude::BufReadExt::peek_len
//! [7]: prelude::ReadExt::read_n
//! [8]: prelude::ReadExt::copy_to_slice
//! [9]: prelude::BufReadExt::reached_eof
//! [10]: prelude::BufReadExt::if_not_eof
//! [11]: prelude::BufWrite
//! [12]: repeat::RepeatSlice
//! [13]: ::std::io::repeat

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
/// See also: [`peek_ne!`].
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
/// See also: [`read_array_ne!`].
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
