/// Exports all traits as well as [`PositionTracker`][1].
///
/// [1]: prelude::PositionTracker
pub mod prelude;

pub mod pos;

pub mod repeat;
/// The buffer size constant used internally by `std::io` since Rust 1.9.0,
/// copied verbatim.
pub const DEFAULT_BUF_SIZE: usize = if cfg!(target_os = "espidf") { 512 } else { 8 * 1024 };

/// Calls [`peek`][1] on a reader and compares the result to a `const` slice.
/// The backup buffer for `peek` is an array of matching size.
///
/// [1]: prelude::BufReadExt::peek
#[macro_export]
macro_rules! next_bytes_eq {
  ($reader:ident, $const_slice:ident) => {{
    use read_write_utils::prelude::*;
    let mut buf = [0u8; $const_slice.len()];
    $reader.peek(&mut buf).map(|slice| slice == $const_slice)
  }};
}
