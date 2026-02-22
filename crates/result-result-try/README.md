# Result\<Result\> Try

This crate provides the `try2!` macro, which implements the functionality of the
`?` operator in functions that return a `Result<Result, _>`.

The motivation behind using nested `Result`s is explained in
["Error Handling in a Correctness-Critical Rust Project"][1].
In short, it allows fatal or unhandleable errors to be separated from
errors that the caller should handle. This arises frequently in code that
performs I/O, since there are myriad reasons why an I/O operation can fail
that are outside the control of the caller and the application.

However, implementing functions that return nested `Result`s is awkward since
errors that occur inside the function can't be propagated with the `?` operator.
This crate aims to address that problem.

## Examples

```rust
use std::io;
use result_result_try::try2;

struct ApplicationError;

fn can_fail() -> io::Result<Result<(), ApplicationError>> {
  let simple_result: Result<u8, ApplicationError> = Ok(1);
  let x: u8 = try2!(simple_result);
  assert_eq!(x, 1);

  // `try2` can also be used with nested results by combining it with `?`
  let nested_result: io::Result<Result<u8, ApplicationError>> = Ok(Ok(2));
  let y: u8 = try2!(nested_result?);
  assert_eq!(y, 2);
  
  Ok(Ok(()))
}
```

[1]: https://sled.rs/errors.html