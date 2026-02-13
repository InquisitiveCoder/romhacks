//! Code shared by Byuu's (a.k.a. Near) two patch formats, UPS and BPS.

use crate::crc::Crc32;
use crate::patch::HasInternalCrc32;
use byteorder::ReadBytesExt;
use std::io::{Read, Seek};

pub mod varint;

pub const FOOTER_LEN: usize = 3 * size_of::<u32>();

pub struct Checksums {
  pub rom_crc32: u32,
  pub result_crc32: u32,
  pub patch_crc32: u32,
}

#[derive(Clone, Debug)]
pub struct Patch<R> {
  file: R,
  crc32: Crc32,
  end_of_data: u64,
  rom_crc32: Crc32,
  result_crc32: Crc32,
}

pub struct PatchReport {
  pub expected_source_crc32: Crc32,
  pub actual_source_crc32: Crc32,
  pub expected_source_size: u64,
  pub actual_source_size: u64,
  pub expected_target_crc32: Crc32,
  pub actual_target_crc32: Crc32,
  pub expected_target_size: u64,
  pub actual_target_size: u64,
  pub patch_internal_crc32: Crc32,
  pub patch_whole_file_crc32: Crc32,
}
