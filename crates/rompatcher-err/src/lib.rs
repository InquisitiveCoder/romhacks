use std::io;

pub mod prelude {
  pub use super::*;
}

pub trait PatchingIOErrors {
  fn bad_patch() -> Self;
  fn input_file_too_small() -> Self;
}

pub trait IOResultExt<T> {
  fn map_patch_err<E: PatchingIOErrors>(self) -> io::Result<Result<T, E>>;

  fn map_rom_err<E: PatchingIOErrors>(self) -> io::Result<Result<T, E>>;
}

impl<T> IOResultExt<T> for io::Result<T> {
  fn map_patch_err<E: PatchingIOErrors>(self) -> io::Result<Result<T, E>> {
    match self {
      Ok(x) => Ok(Ok(x)),
      Err(e) => match e.kind() {
        io::ErrorKind::InvalidInput => Ok(Err(E::bad_patch())),
        io::ErrorKind::InvalidData => Ok(Err(E::bad_patch())),
        io::ErrorKind::UnexpectedEof => Ok(Err(E::bad_patch())),
        _ => Err(e),
      },
    }
  }

  fn map_rom_err<E: PatchingIOErrors>(self) -> io::Result<Result<T, E>> {
    match self {
      Ok(x) => Ok(Ok(x)),
      Err(e) => match e.kind() {
        io::ErrorKind::InvalidInput => Ok(Err(E::bad_patch())),
        io::ErrorKind::UnexpectedEof => Ok(Err(E::input_file_too_small())),
        _ => Err(e),
      },
    }
  }
}
