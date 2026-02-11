//! Code shared by Byuu's (a.k.a. Near) two patch formats, UPS and BPS.

use crate::crc;
use crate::crc::Crc32;
use crate::patch::ups::MAGIC;
use crate::patch::{Error, HasInternalCrc32};
use byteorder::{ReadBytesExt, LE};
use read_write_utils;
use std::io;
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

impl<R: Read> Patch<R> {
  pub fn new(mut file: R) -> io::Result<Patch<R>> {
    let mut hasher = crc::CRC32Hasher::new();
    hasher.update(MAGIC);
    let (footer, bytes_read) =
      read_write_utils::copy_all_but_last::<FOOTER_LEN>(&mut file, &mut hasher)?;
    let end_of_data = MAGIC.len() as u64 + bytes_read;
    hasher.update(&footer);
    let expected_patch_crc32 = hasher.finish();
    let mut footer = io::Cursor::new(footer);
    let rom_crc32 = Crc32::new(footer.read_u32::<LE>()?);
    let result_crc32 = Crc32::new(footer.read_u32::<LE>()?);
    let crc32 = Crc32::new(footer.read_u32::<LE>()?);
    if crc32 != expected_patch_crc32 {
      return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(Self { file, crc32, end_of_data, rom_crc32, result_crc32 })
  }
}

pub fn validate_checksums(
  patch: &mut io::BufReader<&mut (impl Read + Seek + Sized + HasInternalCrc32)>,
  file_checksum: crc::Crc32,
) -> Result<(), Error> {
  let expected_file_checksum = crc::Crc32::new(patch.read_u32::<LE>()?);
  let result_checksum = crc::Crc32::new(patch.read_u32::<LE>()?);
  let expected_patch_checksum = crc::Crc32::new(patch.read_u32::<LE>()?);

  // Check if the patch is valid before anything else.
  if patch.get_ref().internal_crc32() != expected_patch_checksum {
    return Err(Error::BadPatch);
  }

  if file_checksum != expected_file_checksum {
    return Err(if file_checksum == result_checksum {
      Error::AlreadyPatched
    } else {
      Error::WrongInputFile
    });
  }

  Ok(())
}
