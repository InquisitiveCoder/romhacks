//! Code shared by Byuu's (a.k.a. Near) two patch formats, UPS and BPS.

use crate::crc::Crc32;

pub mod varint;

pub const FOOTER_LEN: usize = 3 * size_of::<u32>();

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
