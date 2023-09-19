use crate::kdl::CheckExt;
use crate::val::init;
use crate::{fs, hack, kdl, patch, paths, sha};
use const_format::{concatcp, str_repeat};
use miette::Diagnostic;
use std::io;
use std::str::FromStr;
use thiserror::Error;
use Error as E;

pub const SCHEMA: &str = include_str!("romhacks.schema.kdl");

// nodes
const ROMHACKS_MANIFEST: &'static str = "romhacks-manifest";
const FILE: &'static str = "file";
const PATCH: &'static str = "patch";
const RESULT: &'static str = "result";
const HACK: &'static str = "hack";

// props
const URL: &'static str = "url";
const SHA_1: &'static str = "sha1";
const SHA_256: &'static str = "sha256";
const VERSION: &'static str = "version";

pub fn get_or_create(
  manifest_path: &paths::FilePath,
  rom_path: &paths::FilePathBuf,
  file_digests: &sha::Digests,
  patch_digests: &sha::Digests,
) -> Result<kdl::KdlDocument, Error> {
  let str = match fs::read_to_string(&manifest_path) {
    Ok(str) => str,
    Err(err) => {
      if err.kind() != io::ErrorKind::NotFound {
        return Err(fs::Error::ReadError(err, manifest_path.into()).into());
      }
      log::info!("Didn't find manifest file \"{manifest_path}\". Creating a new manifest.");
      return Ok(create());
    }
  };

  kdl::Schema::parse(SCHEMA)
    .unwrap()
    .check_text_matches(manifest_path.as_str(), &str)?;

  let manifest = init(kdl::KdlDocument::from_str(&str).unwrap(), |doc| {
    doc.nodes_mut().sort_by(|a, b| {
      fn ord(node: &kdl::KdlNode) -> i32 {
        (if node.name().value() == ROMHACKS_MANIFEST { 0 } else { 1 })
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

  validate_file(
    existing_file_node,
    file_digests.sha256(),
    patch_digests.sha256(),
  )?;

  Ok(manifest)
}

#[rustfmt::skip]
fn create() -> kdl::KdlDocument {
  init(kdl::KdlDocument::new(), |doc| {
    doc.nodes_mut().push(init(kdl::KdlNode::new(ROMHACKS_MANIFEST), |node| {
      node.insert(VERSION, "1.0");
    }));
  })
}

fn validate_file(
  file_node: &kdl::KdlNode,
  file_sha256: &str,
  patch_sha256: &str,
) -> Result<(), Error> {
  let patches: &[kdl::KdlNode] = kdl::unwrap_children(file_node);
  let patch_id = kdl::NodeId::new(PATCH, (SHA_256, patch_sha256));
  if patches.iter().find(|patch| patch_id == **patch).is_some() {
    return Err(E::AlreadyPatched);
  }
  let last_patch: &kdl::KdlNode = patches.last().unwrap();
  let last_result_sha256 = kdl::unwrap_children(last_patch)
    .iter()
    .find(|node| node.name().value() == RESULT)
    .and_then(|node| node.get(SHA_256))
    .and_then(|entry| entry.value().as_string())
    .unwrap();
  if file_sha256 != last_result_sha256 {
    return Err(E::ManifestOutdated);
  }
  Ok(())
}

pub fn update(
  doc: &mut kdl::KdlDocument,
  rom: paths::FilePathBuf,
  patch: patch::Patch,
  hack: hack::RomHack,
  file_digests: sha::Digests,
  patch_digests: sha::Digests,
  patched_digests: sha::Digests,
) {
  fn line_wrap(v: impl Into<kdl::KdlEntry>, leading: &str) -> kdl::KdlEntry {
    init(v.into(), |e| e.set_leading(leading))
  }
  let file_nodes = doc.nodes_mut();
  kdl::NodeId::new(FILE, (0, rom.file_name()))
    .get_or_insert(file_nodes, |node| {
      const INDENT: &str = concatcp!(" \\\n", str_repeat!(" ", FILE.len() + 1));
      node.insert(SHA_1, line_wrap(file_digests.sha1(), INDENT));
      node.insert(SHA_256, line_wrap(file_digests.sha256(), INDENT));
    })
    .ensure_children()
    .nodes_mut()
    .push({
      init(kdl::KdlNode::new(PATCH), |node| {
        const INDENT: &str = concatcp!(" \\\n", str_repeat!(" ", 4 + PATCH.len() + 1));
        node.insert(0, patch.path.file_name());
        node.insert(SHA_1, line_wrap(patch_digests.sha1(), INDENT));
        node.insert(SHA_256, line_wrap(patch_digests.sha256(), INDENT));
        let children = node.ensure_children().nodes_mut();
        children.push({
          init(kdl::KdlNode::new(HACK), |node| {
            const INDENT: &str = concatcp!(" \\\n", str_repeat!(" ", 8 + HACK.len() + 1));
            node.insert(URL, hack.url.as_str());
            node.insert(VERSION, line_wrap(hack.version.as_str(), INDENT));
          })
        });
        children.push({
          init(kdl::KdlNode::new(RESULT), |result| {
            const INDENT: &str = concatcp!(" \\\n", str_repeat!(" ", 8 + RESULT.len() + 1));
            result.insert(SHA_1, patched_digests.sha1());
            result.insert(SHA_256, line_wrap(patched_digests.sha256(), INDENT));
          })
        });
      })
    });
}

#[non_exhaustive]
#[derive(Debug, Error, Diagnostic)]
pub enum Error {
  #[error(transparent)]
  #[diagnostic(transparent)]
  IOError(#[from] fs::Error),
  #[error(transparent)]
  #[diagnostic(transparent)]
  KdlError(#[from] kdl::CheckFailure),
  #[error("According to the manifest file, this patch has already been applied.")]
  AlreadyPatched,
  #[error("The file doesn't match the last patch result in the manifest.")]
  ManifestOutdated,
}
