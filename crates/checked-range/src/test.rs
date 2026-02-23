use crate::prelude::*;
use std::ops::RangeFull;

#[test]
fn empty_range_is_well_formed() {
  assert!(CheckedRange::new(0..0).is_some());
}

#[test]
fn unbounded_up_to_min_value_is_well_formed() {
  assert!(CheckedRange::new(..i32::MIN).is_some());
}

#[test]
fn max_value_up_to_unbounded_is_well_formed() {
  assert!(CheckedRange::new(i32::MAX..).is_some());
}

#[test]
fn full_range_is_well_formed() {
  assert!(CheckedRange::<RangeFull, i32>::new(..).is_some());
}

#[test]
fn adjacent_ranges_dont_overlap() {
  let left = CheckedRange::new(0..1).unwrap();
  let right = CheckedRange::new(1..2).unwrap();
  assert!(!left.overlaps(&right));
  assert!(!right.overlaps(&left));
}

#[test]
fn shared_bounds_overlap() {
  let left = CheckedRange::new(0..=1).unwrap();
  let right = CheckedRange::new(1..2).unwrap();
  assert!(left.overlaps(&right));
  assert!(right.overlaps(&left));
}
