pub fn init() {
  use std::io::Write;
  pretty_env_logger::formatted_builder()
    .format(|buf, record| writeln!(buf, "{}: {}", record.level(), record.args()))
    .filter_level(log::LevelFilter::Trace)
    .init();
}
