use regex_lite::Regex;

pub fn game_name<P: AsRef<crate::path::FilePath> + ?Sized>(file_path: &P) -> &str {
  let file_path = file_path.as_ref();
  let mut game_name = file_path.file_stem();
  if (file_path.extension()).is_some_and(|ext| ext.eq_ignore_ascii_case(".bin")) {
    let regex = Regex::new(r#" \(Track [0-9]+\)$"#).unwrap();
    if let Some(m) = regex.find(game_name) {
      game_name = &game_name[0..m.range().start]
    }
  }
  game_name
}
