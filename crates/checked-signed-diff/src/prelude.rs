macro_rules! checked_signed_diff {
  ($a:ident, $b:ident, $int:ty) => {{
    let res = $a.wrapping_sub($b) as $int;
    let overflow = ($a >= $b) == (res < 0);
    if !overflow { Some(res) } else { None }
  }};
}

/// Equivalent to [`u8::checked_signed_diff`].
#[inline]
pub const fn checked_signed_u8_diff(a: u8, b: u8) -> Option<i8> {
  checked_signed_diff!(a, b, i8)
}

/// Equivalent to [`u16::checked_signed_diff`].
#[inline]
pub const fn checked_signed_u16_diff(a: u16, b: u16) -> Option<i16> {
  checked_signed_diff!(a, b, i16)
}

/// Equivalent to [`u32::checked_signed_diff`].
#[inline]
pub const fn checked_signed_u32_diff(a: u32, b: u32) -> Option<i32> {
  checked_signed_diff!(a, b, i32)
}

/// Equivalent to [`u64::checked_signed_diff`].
#[inline]
pub const fn checked_signed_u64_diff(a: u64, b: u64) -> Option<i64> {
  checked_signed_diff!(a, b, i64)
}

/// Equivalent to [`u128::checked_signed_diff`].
#[inline]
pub const fn checked_signed_u128_diff(a: u128, b: u128) -> Option<i128> {
  checked_signed_diff!(a, b, i128)
}

pub trait CheckedSignedDiff {
  /// The signed integer type that corresponds to `Self`.
  type Signed;

  /// Checked integer subtraction.
  ///
  /// Computes `self - rhs` and checks if the result fits into [`Self::Signed`],
  /// returning `None` if overflow occurred.
  fn checked_signed_difference(self, rhs: Self) -> Option<Self::Signed>;
}

impl CheckedSignedDiff for u8 {
  type Signed = i8;

  #[inline]
  fn checked_signed_difference(self, rhs: Self) -> Option<Self::Signed> {
    checked_signed_u8_diff(self, rhs)
  }
}

impl CheckedSignedDiff for u16 {
  type Signed = i16;

  #[inline]
  fn checked_signed_difference(self, rhs: Self) -> Option<Self::Signed> {
    checked_signed_u16_diff(self, rhs)
  }
}

impl CheckedSignedDiff for u32 {
  type Signed = i32;

  #[inline]
  fn checked_signed_difference(self, rhs: Self) -> Option<Self::Signed> {
    checked_signed_u32_diff(self, rhs)
  }
}

impl CheckedSignedDiff for u64 {
  type Signed = i64;

  #[inline]
  fn checked_signed_difference(self, rhs: Self) -> Option<Self::Signed> {
    checked_signed_u64_diff(self, rhs)
  }
}

impl CheckedSignedDiff for u128 {
  type Signed = i128;

  #[inline]
  fn checked_signed_difference(self, rhs: Self) -> Option<Self::Signed> {
    checked_signed_u128_diff(self, rhs)
  }
}
