/// Exports all traits and [`PositionTracker`].
pub mod prelude;

pub mod hash;
pub mod repeat;
pub mod seek;

/// The buffer size constant used internally by `std::read-write-utils` since Rust 1.9.0,
/// copied verbatim.
pub const DEFAULT_BUF_SIZE: usize = if cfg!(target_os = "espidf") { 512 } else { 8 * 1024 };
