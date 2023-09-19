use crate::{apply, validate};

#[derive(Clone, Debug, clap::Parser)]
#[command(author, version, about, long_about = None)]
pub struct Args {
  #[command(subcommand)]
  pub command: CommandKind,
}

#[derive(Clone, Debug, clap::Subcommand)]
#[command(about)]
pub enum CommandKind {
  Apply(apply::Args),
  Validate(validate::Args),
}
