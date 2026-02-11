pub use std::error::*;

pub mod prelude {
  pub use miette::Diagnostic;
  pub use thiserror::Error;
}
