use crate::prelude::*;

#[test]
pub fn fn_is_const() {
  const _: Option<i8> = checked_signed_u8_diff(0, 0);
  const _: Option<i16> = checked_signed_u16_diff(0, 0);
  const _: Option<i32> = checked_signed_u32_diff(0, 0);
  const _: Option<i64> = checked_signed_u64_diff(0, 0);
  const _: Option<i128> = checked_signed_u128_diff(0, 0);
}

#[test]
pub fn trait_is_implemented() {
  assert_eq!(0u8.checked_signed_difference(1), Some(-1));
  assert_eq!(0u16.checked_signed_difference(1), Some(-1));
  assert_eq!(0u32.checked_signed_difference(1), Some(-1));
  assert_eq!(0u64.checked_signed_difference(1), Some(-1));
  assert_eq!(0u128.checked_signed_difference(1), Some(-1));
}

#[test]
pub fn max_positive_diff() {
  assert_eq!(
    (i32::MAX as u32).checked_signed_difference(0),
    Some(i32::MAX)
  );
}

#[test]
pub fn overflow() {
  assert_eq!((i32::MAX as u32 + 1).checked_signed_difference(0), None);
}

#[test]
pub fn max_negative_diff() {
  assert_eq!(
    0u32.checked_signed_difference(i32::MAX as u32 + 1),
    Some(i32::MIN)
  );
}

#[test]
pub fn underflow() {
  assert_eq!(0u32.checked_signed_difference(i32::MAX as u32 + 2), None);
}
