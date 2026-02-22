/// Implements the `?` operator for `Result<T, E>` values in functions that
/// return `Result<Result<T, E>, _>`. Errors are wrapped in `Ok` before being
/// returned.
///
/// In other words, `try2!(result)` is equivalent to:
/// ```ignore
/// match result {
///   Ok(x) => x,
///   Err(e) => return Ok(Err(From::from(e)))
/// }
/// ```
///
/// The macro takes its name from the `try` macro which was superseded by the
/// `?` operator. The `2` alludes to the fact that it operates on nested
/// `Result`s while keeping the macro name short.
///
/// # Examples
/// ```
/// use std::io;
/// use result_result_try::try2;
///
/// struct ApplicationError;
///
/// fn can_fail() -> io::Result<Result<(), ApplicationError>> {
///   let simple_result: Result<u8, ApplicationError> = Ok(1);
///   let x: u8 = try2!(simple_result);
///   assert_eq!(x, 1);
///
///   // `try2` can also be used with nested results by combining it with `?`
///   let nested_result: io::Result<Result<u8, ApplicationError>> = Ok(Ok(2));
///   let y: u8 = try2!(nested_result?);
///   assert_eq!(y, 2);
///
///   Ok(Ok(()))
/// }
/// ```
#[macro_export]
macro_rules! try2 {
  ($e:expr) => {
    match $e {
      ::core::result::Result::Ok(x) => x,
      ::core::result::Result::Err(e) => {
        return ::core::result::Result::Ok(::core::result::Result::Err(::core::convert::From::from(
          e,
        )))
      }
    }
  };
}
