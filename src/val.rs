/// Applies a closure to a value and then returns that value.
pub fn init<T, F>(mut value: T, f: F) -> T
where
  F: FnOnce(&mut T) -> (),
{
  f(&mut value);
  value
}
