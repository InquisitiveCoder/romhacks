//! This crate provides a stable alternative to the nightly-only
//! [`checked_signed_diff`][1] functions.
//!
//! # Examples
//! ```
//! use checked_signed_diff::prelude::*;
//!
//! // Note the different function name to avoid clashing with std.
//! assert_eq!(2u32.checked_signed_difference(5), Some(-3));
//!
//! // const functions are provided too since const traits are unstable
//! const _: i32 = checked_signed_u32_diff(2, 5).unwrap();
//! ```
//!
//! [1]: u32::checked_signed_diff
//! [2]: prelude::CheckedSignedDiff

pub mod prelude;

#[cfg(test)]
mod tests;
