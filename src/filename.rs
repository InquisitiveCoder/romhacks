use regex_lite::Regex;
use std::cell::LazyCell;
use std::ffi::OsStr;
use std::path::Path;

/// The regex state required to match game names in game file names.
pub struct GameNameMatcher {
  regex: LazyCell<Regex>,
}

/// Equivalent to [`GameNameMatcher::new().infer_game_name(file_name)`][infer_game_name].
/// For repeated matches, create a [`GameNameMatcher`].
///
/// [infer_game_name]: GameNameMatcher::infer_game_name
pub fn infer_game_name(file_name: &Path) -> &OsStr {
  GameNameMatcher::new().infer_game_name(file_name)
}

impl GameNameMatcher {
  pub fn new() -> Self {
    Self {
      // Use LazyCell so the regex is only compiled the first time it's used.
      regex: LazyCell::new(|| Regex::new(r#" \(Track \d\d?\)$"#).unwrap()),
    }
  }

  /// Returns a substring of [`file_path.file_stem()`][file_stem] that's representative
  /// of the game's name.
  ///
  /// [file_stem]: Path::file_stem
  ///
  /// # Panics
  /// This method panics if `file_path.file_stem() == None`.
  pub fn infer_game_name<'a>(&self, file_path: &'a Path) -> &'a OsStr {
    let file_stem = file_path.file_stem().unwrap();
    if (file_path.extension()).is_some_and(|ext| ext.eq_ignore_ascii_case(".bin")) {
      let game_name_lossy = file_path.file_stem().unwrap().to_string_lossy();
      if let Some(m) = self.regex.find(game_name_lossy.as_ref()) {
        let bytes = file_stem.as_encoded_bytes();
        let match_len = m.range().len();
        // Per the function's documentation, it's safe to split an OsStr's bytes immediately before
        // a non-empty UTF-8 substring.
        return unsafe { OsStr::from_encoded_bytes_unchecked(&bytes[..bytes.len() - match_len]) };
      }
    }
    file_stem
  }
}

pub trait FileName {
  fn file_name(&self) -> &OsStr;
}

impl FileName for fs_err::File {
  fn file_name(&self) -> &OsStr {
    self.path().file_name().unwrap()
  }
}
