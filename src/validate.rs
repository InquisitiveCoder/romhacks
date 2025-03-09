use crate::kdl::prelude::*;
use crate::{kdl, manifest, path};

#[derive(Clone, Debug, clap::Args)]
pub struct Args {
  pub manifest_path: path::Utf8NativeFilePath,
}

impl Args {
  pub fn call(self) -> Result<(), kdl::CheckFailure> {
    kdl::Schema::parse(manifest::SCHEMA)
      .unwrap()
      .check_file_matches(self.manifest_path)?;
    log::info!("File is valid.");
    Ok(())
  }
}
