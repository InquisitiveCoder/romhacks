use std::cmp;
use std::cmp::Ordering;
use std::ops::{
  Bound, Deref, DerefMut, Range, RangeBounds, RangeFrom, RangeFull, RangeInclusive, RangeTo,
};
use thiserror::Error;
use Bound::*;
use Ordering::*;

/// A range which is known to be well-formed; i.e., its lower bound `<=` its
/// upper bound.
///
/// More specifically, if neither of the range's bounds are [`Unbounded`],
/// the numerical value of the lower bound is less than or equal to the value
/// of the upper bound, regardless of whether they are [`Included`] or
/// [`Excluded`]. If at least one bound is [`Unbounded`], the range is always
/// well-formed.
///
/// For example, `0..`, `0..0` and `0..=0` are both well-formed,
/// but `1..0` and `1..=0` aren't.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CheckedRange<R, I>(R, std::marker::PhantomData<I>);

impl<R, I> CheckedRange<R, I>
where
  R: RangeBounds<I>,
  I: Ord,
{
  pub fn new(range: R) -> Option<CheckedRange<R, I>> {
    let is_well_formed = match (range.start_bound().limit(), range.end_bound().limit()) {
      (Some(start), Some(end)) => start <= end,
      _ => true,
    };
    Some(CheckedRange::new_unchecked(range)).filter(|_| is_well_formed)
  }

  pub fn new_unchecked(range: R) -> CheckedRange<R, I> {
    CheckedRange(range, std::marker::PhantomData)
  }

  pub fn is_empty(&self) -> bool {
    match (self.0.start_bound(), self.0.end_bound()) {
      (Included(a), Excluded(b)) => a == b,
      _ => false,
    }
  }

  /// Returns the inner [`RangeBound`].
  pub fn into_inner(self) -> R {
    self.0
  }
}

impl<I: Ord> From<RangeTo<I>> for CheckedRange<RangeTo<I>, I> {
  fn from(value: RangeTo<I>) -> Self {
    Self::new_unchecked(value)
  }
}

impl<I: Ord> From<RangeFrom<I>> for CheckedRange<RangeFrom<I>, I> {
  fn from(value: RangeFrom<I>) -> Self {
    Self::new_unchecked(value)
  }
}

impl<I: PartialOrd, R: RangeBounds<I>> CheckedRange<R, I>
where
  I: Ord,
{
  pub fn overlaps<O>(&self, other: &CheckedRange<O, I>) -> bool
  where
    O: RangeBounds<I>,
  {
    let max_start = cmp::max(
      LowerBound::new(self.start_bound()),
      LowerBound::new(other.start_bound()),
    );
    let min_end = cmp::min(
      UpperBound::new(self.end_bound()),
      UpperBound::new(other.end_bound()),
    );
    min_end.ge(&max_start)
  }
}

impl From<RangeFull> for CheckedRange<RangeFull, ()> {
  fn from(value: RangeFull) -> Self {
    Self::new_unchecked(value)
  }
}

impl<I: Ord> TryFrom<Range<I>> for CheckedRange<Range<I>, I> {
  type Error = RangeError;

  fn try_from(value: Range<I>) -> Result<Self, Self::Error> {
    CheckedRange::new(value).ok_or(RangeError::new())
  }
}

impl<I: Ord> TryFrom<RangeInclusive<I>> for CheckedRange<RangeInclusive<I>, I> {
  type Error = RangeError;

  fn try_from(value: RangeInclusive<I>) -> Result<Self, Self::Error> {
    CheckedRange::new(value).ok_or(RangeError::new())
  }
}

impl<R, I> Deref for CheckedRange<R, I> {
  type Target = R;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl<R, I> DerefMut for CheckedRange<R, I> {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.0
  }
}

trait BoundLimit {
  type Value;
  fn limit(&self) -> Option<&Self::Value>;
}

impl<T> BoundLimit for Bound<&T> {
  type Value = T;

  fn limit(&self) -> Option<&Self::Value> {
    match *self {
      Included(value) => Some(value),
      Excluded(value) => Some(value),
      Unbounded => None,
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UpperBound<I>(Bound<I>);

impl<I: Ord> UpperBound<I> {
  pub fn new(bound: Bound<I>) -> Self {
    Self(bound)
  }
}

impl<I: Ord> PartialOrd for UpperBound<I> {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

impl<I: Ord> Ord for UpperBound<I> {
  fn cmp(&self, other: &Self) -> Ordering {
    match (&self.0, &other.0) {
      (Unbounded, Unbounded) => Equal,
      (Unbounded, _) => Greater,
      (Included(a), Included(b)) => a.cmp(b),
      (Excluded(a), Excluded(b)) => a.cmp(b),
      (Included(a), Excluded(b)) => match a.cmp(b) {
        Equal => Greater,
        Less => Less,
        Greater => Greater,
      },
      _ => other.cmp(self).reverse(),
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LowerBound<I>(Bound<I>);

impl<I: Ord> LowerBound<I> {
  pub fn new(bound: Bound<I>) -> Self {
    Self(bound)
  }
}

impl<I: Ord> PartialOrd for LowerBound<I> {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

impl<I: Ord> Ord for LowerBound<I> {
  fn cmp(&self, other: &Self) -> Ordering {
    match (&self.0, &other.0) {
      (Unbounded, Unbounded) => Equal,
      (Unbounded, _) => Less,
      (Included(a), Included(b)) => a.cmp(b),
      (Excluded(a), Excluded(b)) => a.cmp(b),
      (Included(a), Excluded(b)) => match a.cmp(b) {
        Equal => Less,
        Less => Less,
        Greater => Greater,
      },
      _ => other.cmp(self).reverse(),
    }
  }
}

impl<I: Ord> PartialEq<UpperBound<I>> for LowerBound<I> {
  fn eq(&self, other: &UpperBound<I>) -> bool {
    self.partial_cmp(other) == Some(Equal)
  }
}

impl<I: Ord> PartialOrd<UpperBound<I>> for LowerBound<I> {
  fn partial_cmp(&self, other: &UpperBound<I>) -> Option<Ordering> {
    match (&self.0, &other.0) {
      (Unbounded, _) => Some(Less),
      (_, Unbounded) => Some(Less),
      (Included(a), Included(b)) => Some(a.cmp(b)),
      (Excluded(a), Excluded(b)) => Some(match a.cmp(b) {
        Equal => Greater,
        Less => Less,
        Greater => Greater,
      }),
      (Included(a), Excluded(b)) => Some(match a.cmp(b) {
        Equal => Greater,
        Less => Less,
        Greater => Greater,
      }),
      (Excluded(a), Included(b)) => Some(match a.cmp(b) {
        Equal => Greater,
        Less => Less,
        Greater => Greater,
      }),
    }
  }
}

impl<I: Ord> PartialEq<LowerBound<I>> for UpperBound<I> {
  fn eq(&self, other: &LowerBound<I>) -> bool {
    self.partial_cmp(other) == Some(Equal)
  }
}

impl<I: Ord> PartialOrd<LowerBound<I>> for UpperBound<I> {
  fn partial_cmp(&self, other: &LowerBound<I>) -> Option<Ordering> {
    LowerBound::partial_cmp(other, self).map(Ordering::reverse)
  }
}

#[derive(Debug, Error)]
#[error("Invalid range.")]
pub struct RangeError(());

impl RangeError {
  fn new() -> Self {
    Self(())
  }
}
