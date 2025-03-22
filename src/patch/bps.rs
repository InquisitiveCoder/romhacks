use crate::crc;
use crate::patch::Error;
use std::{io, mem};

use crate::io::prelude::*;

pub const MAGIC: &[u8] = b"BPS";

pub fn patch(
  rom: &mut (impl Read + Seek),
  patch: &mut (impl Read + Seek),
  output: &mut (impl Read + Write + Seek),
  rom_checksum: crc::Crc32,
  patch_checksum: crc::Crc32,
  patch_eof: u64,
) -> Result<Result<(), Error>, io::Error> {
  use crate::patch::Error as E;
  Ok(Ok(()))
}
