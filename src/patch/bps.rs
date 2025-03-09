use crate::crc;
use crate::patch::Error;
use std::{io, mem};

use crate::io::prelude::*;

pub fn patch(
  rom: &mut (impl Read + Write + Seek + Resize),
  patch: &mut (impl Read + Seek),
  file_checksum: crc::Crc32,
  patch_checksum: crc::Crc32,
  patch_eof: u64,
) -> Result<Result<(), Error>, io::Error> {
  use crate::patch::Error as E;
  Ok(Ok(()))
}
