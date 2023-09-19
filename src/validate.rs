use crate::{kdl, manifest, paths};
use kdl_schema_check::{CheckExt, CheckFailure};

#[derive(Clone, Debug, clap::Args)]
pub struct Args {
  pub manifest_path: paths::FilePathBuf,
}

impl Args {
  pub fn call(self) -> Result<(), CheckFailure> {
    kdl::Schema::parse(manifest::SCHEMA)
      .unwrap()
      .check_file_matches(self.manifest_path)?;
    log::info!("File is valid.");
    Ok(())
  }
}
