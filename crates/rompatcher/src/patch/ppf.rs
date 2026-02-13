use crate::convert::prelude::*;
use crate::patch;
use crate::patch::Error::{BadPatch, WrongInputFile};
use crate::patch::{patch_err, rom_err};
use byteorder::{ReadBytesExt, LE};
use range_utils::prelude::CheckedRange;
use read_write_utils::hash::{HashingReader, HashingWriter};
use read_write_utils::prelude::*;
use std::borrow::Cow;
use std::fmt::Formatter;
use std::hash::Hasher;
use std::io::prelude::*;
use std::num;
use std::ops::Range;
use std::{cmp, io};

pub const MAGIC: &[u8] = b"PPF";

const BLOCK_CHECK_LENGTH: u16 = 1024;
const BEGIN_MAGIC: &[u8] = b"@BEGIN_FILE_ID.DIZ";
const END_MAGIC: &[u8] = b"@END_FILE_ID.DIZ";

/// Applies a PPF patch to a ROM.
pub fn patch(
  rom: &mut (impl BufRead + Seek),
  patch: &mut (impl BufRead + Seek),
  output: &mut impl BufWrite,
  strict: bool,
) -> Result<(), patch::Error> {
  let mut patch = PositionTracker::from_start(patch);
  let Format {
    block_check,
    can_have_footer,
    has_undo_data,
    rom_offset_type,
  } = Format::parse_and_validate(&mut patch)?;
  let mut rom = PositionTracker::from_start(rom);

  let offset_size: usize = rom_offset_type.size();
  let magic_offset: u64 = {
    let mut buf = [0u8; 8];
    (&mut &BEGIN_MAGIC[..offset_size]).read(&mut buf[..])?;
    u64::from_le_bytes(buf)
  };
  let mut hasher = crc32fast::Hasher::new();

  loop {
    let offset: u64 = {
      let mut buf = [0u8; size_of::<u64>()];
      u64::from_le_bytes(
        patch
          .read_exact(&mut buf[..offset_size])
          .map(|_| buf)
          .map_err(patch_err)?,
      )
    };

    if can_have_footer && offset == magic_offset {
      break;
    }

    let hunk_length: u8 = patch.read_u8().map_err(patch_err)?;
    num::NonZeroU8::new(hunk_length).ok_or(BadPatch)?;

    // If the patch includes a block check, we need to compare a specific 1 KB
    // block of the ROM to the block that was included in the patch header.
    // Hash the ROM block when the patching loop reaches it, then compare its
    // crc32 to hash of the patch block that was calculated earlier.
    let initial_rom_position: u64 = rom.position();
    let rom_range = CheckedRange::new(initial_rom_position..offset).ok_or(BadPatch)?;
    match &block_check {
      None => {
        rom.copy_until(offset, output).map_err(rom_err)?;
      }
      Some(BlockCheck { region, crc32 }) => {
        if !rom_range.overlaps(region) {
          rom.copy_until(offset, output).map_err(rom_err)?;
        } else {
          // Copy until the start of the block check region.
          rom
            .copy_until(cmp::max(region.start, initial_rom_position), output)
            .map_err(rom_err)?;
          // Hash and copy until the patch offset or the end of the block check
          // region, whichever comes first.
          rom
            .take_from_inner_until(cmp::min(offset, region.end), |take| {
              take.exactly(|rom| {
                let mut hashing_reader = HashingReader::new(rom, &mut hasher);
                io::copy(&mut hashing_reader, output)
              })
            })
            .map_err(rom_err)?;
          // If we reached the end of the block check region first, copy from
          // the ROM until the patch offset is reached.
          rom
            .copy_until(cmp::max(offset, rom.position()), output)
            .map_err(rom_err)?;
        }

        // If we finished hashing the block check region on this iteration,
        // compare it to the block included in the patch.
        if (initial_rom_position..=rom.position()).contains(&region.end) {
          let rom_block_crc32 = hasher.finish();
          if strict && rom_block_crc32 != u64::from(*crc32) {
            return Err(WrongInputFile);
          }
        }
      }
    }

    debug_assert_eq!(rom.position(), offset);
    patch
      .copy_exactly(u64::from(hunk_length), output)
      .map_err(patch_err)?;

    if has_undo_data {
      patch.seek_relative(hunk_length.into())?;
    }

    if patch.has_reached_eof()? {
      break; // EOF
    }
  }

  if rom.position() == 0 {
    return Err(BadPatch);
  }

  Ok(())
}

/// Details about the format of a PPF file.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Format {
  rom_offset_type: RomOffsetType,
  has_undo_data: bool,
  can_have_footer: bool,
  block_check: Option<BlockCheck>,
}

impl Format {
  /// Parses the PPF header and footer and performs block check validation.
  ///
  /// `patch`'s cursor must be at the start of the file, and `eof` must be the
  /// length of the PPF file.
  ///
  /// If this method returns `Ok`, `patch` will be positioned at the start of
  /// the patch data. No guarantees are made about its cursor position otherwise.
  pub fn parse_and_validate(
    patch: &mut PositionTracker<impl BufRead + Seek>,
  ) -> Result<Format, patch::Error> {
    // applyppf3 parses the magic string to obtain the version number and
    // ignores the dedicated version byte. However, ROM Patcher JS checks both
    // and throws an error if they don't match. Given the latter's widespread
    // use, it's probably safe to follow its lead.
    let version = Version::try_from(&patch.read_array::<5>().map_err(patch_err)?)?;
    if version != Version::try_from(patch.read_u8().map_err(patch_err)?)? {
      return Err(BadPatch);
    }

    // The PPF docs don't specify the encoding of the description or the
    // contents of unused space. In practice, it always seems to be ASCII, with
    // spaces (0x20) or less commonly nul (0x00) as padding. In that case,
    // String::from_utf8_lossy will cast the byte slice without having to copy
    // and modify the string, while str::trim_end will handle trailing spaces.
    // Nul bytes aren't displayed even if they're in the middle of a string.
    let description: [u8; 50] = patch.read_array()?;
    let description: Cow<str> = String::from_utf8_lossy(&description);
    let description: &str = description.trim_end();
    log::debug!("PPF patch description: {description}");

    Ok(match version {
      Version::V1 => Format {
        can_have_footer: false,
        rom_offset_type: RomOffsetType::U32,
        has_undo_data: false,
        block_check: None,
      },
      Version::V2 => {
        // File size checks were deprecated in V3 because they were unreliable,
        // but an absent file size might indicate an invalid PPF file.
        num::NonZeroU32::try_from(patch.read_u32::<LE>()?).map_err(|_| BadPatch)?;
        let mut hashing_writer = HashingWriter::new(io::sink(), crc32fast::Hasher::new());
        patch.take_from_inner(BLOCK_CHECK_LENGTH as u64, |take| {
          take.exactly(|patch| io::copy(patch, &mut hashing_writer))
        })?;
        let block_start = u64::from(ImageType::BIN.block_check_offset());
        let block_end = block_start + u64::from(BLOCK_CHECK_LENGTH);
        Format {
          can_have_footer: true,
          rom_offset_type: RomOffsetType::U32,
          has_undo_data: false,
          block_check: Some(BlockCheck {
            region: CheckedRange::new(block_start..block_end).unwrap(),
            crc32: hashing_writer.into_hasher().finalize(),
          }),
        }
      }
      Version::V3 => {
        let image_type = ImageType::try_from(patch.read_u8()?)?;
        let has_block_check = (patch.read_u8()?)
          .try_into_bool()
          .map_err(|_| patch::Error::BadPatch)?;
        let has_undo_data = (patch.read_u8()?)
          .try_into_bool()
          .map_err(|_| patch::Error::BadPatch)?;
        patch.seek_relative(1)?; // Unused in V3
        let block_check = match has_block_check {
          false => None,
          true => {
            let mut hashing_writer = HashingWriter::new(io::sink(), crc32fast::Hasher::new());
            patch.take_from_inner(u64::from(BLOCK_CHECK_LENGTH), |take| {
              take.exactly(|patch| io::copy(patch, &mut hashing_writer))
            })?;
            let block_start = u64::from(image_type.block_check_offset());
            let block_end = block_start + u64::from(BLOCK_CHECK_LENGTH);
            Some(BlockCheck {
              region: CheckedRange::new(block_start..block_end).unwrap(),
              crc32: hashing_writer.into_hasher().finalize(),
            })
          }
        };
        Format {
          can_have_footer: true,
          rom_offset_type: RomOffsetType::U64,
          has_undo_data,
          block_check,
        }
      }
    })
  }

  /// Finds the end of the PPF2 or PPF3 patch data. PPF2 and PPF3 files may
  /// have an **optional** footer with the following structure:
  ///
  /// `"@BEGIN_FILE_ID.DIZ" BODY "@END_FILE_ID.DIZ" BODY_LENGTH`
  ///
  /// where BODY_LENGTH cannot exceed 3072. BODY_LENGTH is 4 bytes long in
  /// PPF2 files and 2 bytes long in PPF3. (It's not clear what purpose the
  /// final two BODY_LENGTH bytes served in PPF2; the PPF3 docs don't say.)
  ///
  /// The PPF3 documentation refers to the BODY as a file_id, FILE_ID.DIZ, or
  /// FILE_ID.DIZ file. Because of this lack of consistency and the potential
  /// ambiguity with the term "file_id area", this code uses the terms "footer"
  /// and "body" instead.
  fn find_end_of_patch<R: Read + Seek>(
    patch: &mut io::BufReader<R>,
    body_len_type: FooterBodyLengthType,
    range: std::ops::Range<u64>,
  ) -> Result<u64, patch::Error> {
    const MAX_BODY_LENGTH: u32 = 3072;

    let remaining: u64 = range.end - range.start;
    let body_len_size: usize = body_len_type.size();
    let footer_end_len: u64 = END_MAGIC.len() as u64 + body_len_size as u64;

    // If the end string doesn't fit, there's obviously no footer. Return EOF.
    // Note that this behavior differs from the applyppf3 program; it only
    // checks for the second ".DIZ", which can result in false positives.
    // Also note that we don't want to return early in the case where the start
    // string doesn't fit. If the file ends with the footer end string but we
    // can't find the start string, that should be an error.
    if remaining < footer_end_len {
      return Ok(range.end);
    }

    // We need to check the footer body length stored at the end of the PPF,
    // then backtrack to validate the start of the footer.
    //
    // If file is larger than the read buffer, the BufReader will need to refill
    // its buffer at some point. Instead of letting the BufReader refill the
    // buffer at an arbitrary position close to the end of the file, it's better
    // to refill it with the last patch.capacity() bytes so that we can backtrack
    // within the buffer instead of seeking backwards beyond the start of the
    // buffer and performing an additional read.
    let end_buf_pos: u64 = if range.end > patch.capacity() as u64 {
      // The buffer needs to be empty for BufReader::fill_buf to refill it;
      // BufReader::seek.rs will always discard the buffer.
      let pos: u64 = patch.seek(io::SeekFrom::End(-(patch.capacity() as i64)))?;
      patch.fill_buf()?;
      pos
    } else {
      range.start
    };

    // All of the following relative seeks (except possibly the final seek.rs back
    // to the start of the patch region) should fall within the buffer.

    // Seek to the end-of-footer magic string.
    let end_magic_pos: u64 = range.end - footer_end_len;
    patch.seek_relative((end_magic_pos - end_buf_pos) as i64)?;

    let seek_to_start = |patch: &mut io::BufReader<R>, pos: u64| -> io::Result<()> {
      if range.start >= end_buf_pos {
        // The start of the patch area falls within the read buffer.
        // Perform a relative seek.rs to keep the buffer.
        patch.seek_relative(range.start as i64 - pos as i64)
      } else {
        // The start of the patch area isn't in the buffer, so the buffer will
        // be discarded regardless of how we seek.rs. An absolute seek.rs is simpler
        // and avoids overflow issues when calculating this offset.
        patch.seek(io::SeekFrom::Start(range.start))?;
        Ok(())
      }
    };

    let buf = {
      let mut buf = [0u8; END_MAGIC.len()];
      patch.read_exact(&mut buf[..]).map(|_| buf)?
    };
    // If there's no footer, seek.rs back to the start of the patch data and return
    // EOF. This is the most common case.
    if buf != END_MAGIC {
      seek_to_start(patch, end_magic_pos)?;
      return Ok(range.end);
    }

    let body_len: u32 = {
      // Little endian order yields the same numerical value at larger sizes,
      // so a 4 byte buffer can be used for both a body_len_size of 2 and 4.
      let mut buf = [0u8; size_of::<u32>()];
      patch.read_exact(&mut buf[..body_len_size])?;
      u32::from_le_bytes(buf)
    };
    let footer_len: u64 =
      BEGIN_MAGIC.len() as u64 + body_len as u64 + END_MAGIC.len() as u64 + body_len_size as u64;
    if body_len > MAX_BODY_LENGTH || footer_len > remaining {
      // If the body length stored in the file is larger than the max defined
      // in the PPF specs, or it's larger than the non-header region of the
      // file, the file is probably corrupt.
      return Err(patch::Error::BadPatch);
    }

    patch.seek_relative(-(footer_len as i64))?;
    let buf = {
      let mut buf = [0u8; BEGIN_MAGIC.len()];
      patch.read_exact(&mut buf[..]).map(|_| buf)?
    };
    if buf != BEGIN_MAGIC {
      // If the file contains an end-of-footer string without a matching
      // start-of-footer string, the file is probably corrupt.
      return Err(patch::Error::BadPatch);
    }

    // Found the footer. Seek back to the start of the patch.
    let footer_pos = range.end - footer_len;
    let current_pos = footer_pos + BEGIN_MAGIC.len() as u64;
    seek_to_start(patch, current_pos)?;
    Ok(footer_pos)
  }
}

/// A PPF2 or PPF3 block check.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BlockCheck {
  region: CheckedRange<Range<u64>, u64>,
  crc32: u32,
}

impl BlockCheck {
  pub fn validate(
    &self,
    patch: &mut impl Read,
    file: &mut (impl Read + Seek),
  ) -> Result<(), patch::Error> {
    file.seek(io::SeekFrom::Start(self.region.start.into()))?;
    let file_block: [u8; BLOCK_CHECK_LENGTH as usize] = file.read_array()?;
    let validation_block: [u8; BLOCK_CHECK_LENGTH as usize] = patch.read_array()?;
    if file_block != validation_block {
      Err(patch::Error::BadPatch)?;
    }
    Ok(())
  }
}

/// The PPF format versions.
#[derive(Copy, Clone, Debug, Default, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub enum Version {
  #[default]
  V1,
  V2,
  V3,
}

impl std::fmt::Display for Version {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    match self {
      Version::V1 => write!(f, "PPF1.0"),
      Version::V2 => write!(f, "PPF2.0"),
      Version::V3 => write!(f, "PPF3.0"),
    }
  }
}

impl TryFrom<&[u8; 5]> for Version {
  type Error = patch::Error;

  fn try_from(value: &[u8; 5]) -> Result<Self, Self::Error> {
    match value {
      b"PPF10" => Ok(Version::V1),
      b"PPF20" => Ok(Version::V2),
      b"PPF30" => Ok(Version::V3),
      _ => Err(patch::Error::BadPatch),
    }
  }
}

impl TryFrom<u8> for Version {
  type Error = patch::Error;

  fn try_from(value: u8) -> Result<Self, Self::Error> {
    match value {
      0 => Ok(Version::V1),
      1 => Ok(Version::V2),
      2 => Ok(Version::V3),
      _ => Err(BadPatch),
    }
  }
}

/// The ROM image types used in block checks.
#[derive(Clone, Copy, Debug, Default, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub enum ImageType {
  #[default]
  BIN,
  GI,
}

impl ImageType {
  pub fn block_check_offset(&self) -> u16 {
    match self {
      ImageType::BIN => 0x9320,
      ImageType::GI => 0x80A0,
    }
  }
}

impl TryFrom<u8> for ImageType {
  type Error = patch::Error;

  fn try_from(value: u8) -> Result<Self, Self::Error> {
    match value {
      0 => Ok(Self::BIN),
      1 => Ok(Self::GI),
      _ => Err(patch::Error::BadPatch),
    }
  }
}

#[derive(Clone, Copy, Debug, Default, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub enum FooterBodyLengthType {
  #[default]
  U16,
  U32,
}

impl FooterBodyLengthType {
  const fn size(self) -> usize {
    match self {
      Self::U16 => size_of::<u16>(),
      Self::U32 => size_of::<u32>(),
    }
  }
}

#[derive(Clone, Copy, Debug, Default, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub enum RomOffsetType {
  #[default]
  U32,
  U64,
}

impl RomOffsetType {
  const fn size(self) -> usize {
    match self {
      Self::U32 => size_of::<u32>(),
      Self::U64 => size_of::<u64>(),
    }
  }
}
