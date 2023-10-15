use crate::error::prelude::*;
use crate::kdl::prelude::*;
use crate::{crc, fs, hack, io, kdl, mem, patch, path};
use std::str::FromStr;

pub const SCHEMA: &str = include_str!("romhacks.schema.kdl");

// nodes
const ROMHACKS_MANIFEST: &str = "romhacks-manifest";
const FILE: &str = "file";
const PATCH: &str = "patch";
const RESULT: &str = "result";
const HACK: &str = "hack";

// props
const URL: &str = "url";
const CRC_32: &str = "crc32";
const VERSION: &str = "version";

pub fn get_or_create(
  manifest_path: &path::FilePath,
  rom_path: &path::FilePathBuf,
  rom_digest: crc::Crc32,
  patch_digest: crc::Crc32,
) -> Result<kdl::KdlDocument, Error> {
  let str = match fs::read_to_string(&manifest_path) {
    Ok(str) => str,
    Err(err) => {
      if err.kind() != io::ErrorKind::NotFound {
        Err(fs::Error::file(err, manifest_path))?;
      }
      log::info!("Didn't find manifest file \"{manifest_path}\". Creating a new manifest.");
      return Ok(create());
    }
  };

  kdl::Schema::parse(SCHEMA)
    .unwrap()
    .check_text_matches(manifest_path.as_str(), &str)?;

  let manifest = mem::init(kdl::KdlDocument::from_str(&str).unwrap(), |doc| {
    doc.nodes_mut().sort_by(|a, b| {
      fn ord(node: &kdl::KdlNode) -> i32 {
        (node.name().value() != ROMHACKS_MANIFEST) as i32
      }
      ord(a).cmp(&ord(b))
    })
  });

  let existing_file_node = &manifest.nodes()[1..]
    .iter()
    .find(|node| kdl::NodeId::new(FILE, (0, rom_path.file_name())) == **node);
  let existing_file_node = match existing_file_node {
    Some(node) => node,
    None => return Ok(manifest),
  };

  validate_file(existing_file_node, rom_digest, patch_digest)?;

  Ok(manifest)
}

#[rustfmt::skip]
fn create() -> kdl::KdlDocument {
  mem::init(kdl::KdlDocument::new(), |doc| {
    doc.nodes_mut().push(mem::init(kdl::KdlNode::new(ROMHACKS_MANIFEST), |node| {
      node.insert(VERSION, "1.0");
    }));
  })
}

fn validate_file(
  file_node: &kdl::KdlNode,
  file_crc32: crc::Crc32,
  patch_crc32: crc::Crc32,
) -> Result<(), Error> {
  let patches: &[kdl::KdlNode] = kdl::unwrap_children(file_node);
  let patch_id = kdl::NodeId::new(PATCH, (CRC_32, patch_crc32));
  if patches.iter().find(|patch| patch_id == **patch).is_some() {
    Err(Error::AlreadyPatched)?;
  }
  let last_patch: &kdl::KdlNode = patches.last().unwrap();
  let last_result_crc32 = kdl::unwrap_children(last_patch)
    .iter()
    .find(|node| node.name().value() == RESULT)
    .and_then(|node| node.get(CRC_32))
    .and_then(|entry| entry.value().as_i64().map(|x| x as u32))
    .map(crc::Crc32::new)
    .unwrap();
  if file_crc32 != last_result_crc32 {
    Err(Error::ManifestOutdated)?;
  }
  Ok(())
}

pub fn update(
  doc: &mut kdl::KdlDocument,
  rom: path::FilePathBuf,
  patch: patch::Patch,
  hack: hack::RomHack,
  file_digest: crc::Crc32,
  patch_digest: crc::Crc32,
  patched_digest: crc::Crc32,
) {
  let file_nodes = doc.nodes_mut();
  kdl::NodeId::new(FILE, (0, rom.file_name()))
    .get_or_insert(file_nodes, |node| {
      node.insert(CRC_32, file_digest);
    })
    .ensure_children()
    .nodes_mut()
    .push(mem::init(kdl::KdlNode::new(PATCH), |node| {
      node.insert(0, patch.path.file_name());
      node.insert(CRC_32, patch_digest);
      let children = node.ensure_children().nodes_mut();
      children.push(mem::init(kdl::KdlNode::new(HACK), |node| {
        node.insert(URL, hack.url.as_str());
        node.insert(VERSION, hack.version.as_str());
      }));
      children.push(mem::init(kdl::KdlNode::new(RESULT), |node| {
        node.insert(CRC_32, patched_digest);
      }));
    }));
}

#[non_exhaustive]
#[derive(Debug, Error, Diagnostic)]
pub enum Error {
  #[error(transparent)]
  #[diagnostic(transparent)]
  IO(#[from] fs::Error),
  #[error(transparent)]
  #[diagnostic(transparent)]
  Kdl(#[from] kdl::CheckFailure),
  #[error("According to the manifest file, this patch has already been applied.")]
  AlreadyPatched,
  #[error("The file doesn't match the last patch result in the manifest.")]
  ManifestOutdated,
}
