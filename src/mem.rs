pub use std::mem::*;

/// Applies a closure to a value and then returns that value.
pub fn init<T, F>(mut value: T, f: F) -> T
where
  F: FnOnce(&mut T),
{
  f(&mut value);
  value
}

/// Applies a closure to a value and then returns the result.
pub fn try_init<T, F, O, E>(mut value: T, f: F) -> Result<T, E>
where
  F: FnOnce(&mut T) -> Result<O, E>,
{
  f(&mut value)?;
  Ok(value)
}

pub fn try_map<R, I, A, F>(value: A, f: F) -> Result<R, <I as TryInto<R>>::Error>
where
  A: Into<I>,
  I: TryInto<R>,
  F: FnOnce(&mut I),
{
  let mut inner: I = value.into();
  f(&mut inner);
  inner.try_into()
}
