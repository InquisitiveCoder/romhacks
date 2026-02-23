use crate::error::prelude::*;

pub mod prelude {
  pub use super::TryIntoBool;
}

pub trait TryIntoBool {
  fn try_into_bool(self) -> Result<bool, TryIntoBoolError>;
}

impl TryIntoBool for u8 {
  fn try_into_bool(self) -> Result<bool, TryIntoBoolError> {
    match self {
      0 => Ok(false),
      1 => Ok(true),
      _ => Err(TryIntoBoolError(())),
    }
  }
}

#[derive(Clone, Debug)]
pub struct TryIntoBoolError(pub(crate) ());
