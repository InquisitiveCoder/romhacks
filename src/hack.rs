#[derive(Clone, Debug, clap::Args)]
pub struct RomHack {
  #[arg(short, long = "hack-url")]
  pub url: url::Url,
  #[arg(short, long = "hack-version")]
  pub version: String,
}
