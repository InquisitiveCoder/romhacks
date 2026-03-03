use byteorder::{ReadBytesExt, LE};
use checked_range::prelude::CheckedRange;
use read_write_hashers::{HashingReader, HashingWriter};
use read_write_utils::prelude::*;
use result_result_try::try2;
use rompatcher_err::*;
use std::borrow::Cow;
use std::fmt::Formatter;
use std::hash::Hasher;
use std::io::prelude::*;
use std::io::SeekFrom;
use std::num;
use std::ops::Range;
use std::{cmp, io};
use PatchingError::*;

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
) -> io::Result<Result<(), PatchingError>> {
  let mut patch = PositionTracker::from_start(patch);
  let Format {
    block_check,
    footer_body_len_size,
    has_undo_data,
    rom_offset_type,
  } = try2!(Format::parse_and_validate(&mut patch)?);
  let mut rom = PositionTracker::from_start(rom);

  let offset_size: usize = rom_offset_type.size();
  let mut hasher = crc32fast::Hasher::new();

  loop {
    let offset: u64 = {
      let mut buf = [0u8; size_of::<u64>()];
      u64::from_le_bytes(try2!(
        patch
          .read_exact(&mut buf[..offset_size])
          .map(|_| buf)
          .map_patch_err()?
      ))
    };

    let hunk_length: u8 = try2!(patch.read_u8().map_patch_err()?);
    try2!(num::NonZeroU8::new(hunk_length).ok_or(BadPatch));

    // If the patch includes a block check, we need to compare a specific 1 KB
    // block of the ROM to the block that was included in the patch header.
    // Hash the ROM block when the patching loop reaches it, then compare its
    // crc32 to hash of the patch block that was calculated earlier.
    let initial_rom_position: u64 = rom.position();
    let rom_range = try2!(CheckedRange::new(initial_rom_position..offset).ok_or(BadPatch));
    match &block_check {
      None => {
        try2!(rom.copy_until(offset, output).map_rom_err()?);
      }
      Some(BlockCheck { region, crc32 }) => {
        if !rom_range.overlaps(region) {
          try2!(rom.copy_until(offset, output).map_rom_err()?);
        } else {
          // Copy until the start of the block check region.
          try2!(
            rom
              .copy_until(cmp::max(region.start, initial_rom_position), output)
              .map_rom_err()?
          );
          // Hash and copy until the patch offset or the end of the block check
          // region, whichever comes first.
          try2!(
            rom
              .take_from_inner_until(cmp::min(offset, region.end), |take| {
                take.exactly(|rom| {
                  let mut hashing_reader = HashingReader::new(rom, &mut hasher);
                  io::copy(&mut hashing_reader, output)
                })
              })
              .map_rom_err()?
          );
          // If we reached the end of the block check region first, copy from
          // the ROM until the patch offset is reached.
          try2!(
            rom
              .copy_until(cmp::max(offset, rom.position()), output)
              .map_rom_err()?
          );
        }

        // If we finished hashing the block check region on this iteration,
        // compare it to the block included in the patch.
        if (initial_rom_position..=rom.position()).contains(&region.end) {
          let rom_block_crc32 = hasher.finish();
          if strict && rom_block_crc32 != u64::from(*crc32) {
            return Ok(Err(WrongInputFile));
          }
        }
      }
    }

    debug_assert_eq!(rom.position(), offset);
    try2!(
      patch
        .copy_exactly(u64::from(hunk_length), output)
        .map_patch_err()?
    );

    if has_undo_data {
      patch.seek_relative(hunk_length.into())?;
    }

    if try2!(is_end_of_patch(&mut patch, footer_body_len_size)?) {
      break;
    }
  }

  if rom.position() == 0 {
    return Ok(Err(BadPatch));
  }

  Ok(Ok(()))
}

/// Details about the format of a PPF file.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Format {
  rom_offset_type: RomOffsetType,
  footer_body_len_size: Option<FooterBodyLengthType>,
  has_undo_data: bool,
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
  ) -> io::Result<Result<Format, PatchingError>> {
    // applyppf3 parses the magic string to obtain the version number and
    // ignores the dedicated version byte. However, ROM Patcher JS checks both
    // and throws an error if they don't match. Given the latter's widespread
    // use, it's probably safe to follow its lead.
    let version = try2!(Version::try_from(
      &(try2!(patch.read_array::<5>().map_patch_err()?))
    ));
    if version != try2!(Version::try_from(try2!(patch.read_u8().map_patch_err()?))) {
      return Ok(Err(BadPatch));
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

    Ok(Ok(match version {
      Version::V1 => Format {
        footer_body_len_size: None,
        rom_offset_type: RomOffsetType::U32,
        has_undo_data: false,
        block_check: None,
      },
      Version::V2 => {
        // File size checks were deprecated in V3 because they were unreliable,
        // but an absent file size might indicate an invalid PPF file.
        try2!(
          num::NonZeroU32::try_from(try2!(patch.read_u32::<LE>().map_patch_err()?))
            .map_err(|_| BadPatch)
        );
        let mut hashing_writer = HashingWriter::new(io::sink(), crc32fast::Hasher::new());
        patch.take_from_inner(BLOCK_CHECK_LENGTH as u64, |take| {
          take.exactly(|patch| io::copy(patch, &mut hashing_writer))
        })?;
        let block_start = u64::from(ImageType::BIN.block_check_offset());
        let block_end = block_start + u64::from(BLOCK_CHECK_LENGTH);
        Format {
          footer_body_len_size: Some(FooterBodyLengthType::U32),
          rom_offset_type: RomOffsetType::U32,
          has_undo_data: false,
          block_check: Some(BlockCheck {
            region: CheckedRange::new(block_start..block_end).unwrap(),
            crc32: hashing_writer.into_hasher().finalize(),
          }),
        }
      }
      Version::V3 => {
        let image_type = try2!(ImageType::try_from(try2!(patch.read_u8().map_patch_err()?)));
        let has_block_check = try2!(
          try2!(patch.read_u8().map_patch_err()?)
            .try_into_bool()
            .map_err(|_| BadPatch)
        );
        let has_undo_data = try2!(
          try2!(patch.read_u8().map_patch_err()?)
            .try_into_bool()
            .map_err(|_| BadPatch)
        );
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
          footer_body_len_size: Some(FooterBodyLengthType::U16),
          rom_offset_type: RomOffsetType::U64,
          has_undo_data,
          block_check,
        }
      }
    }))
  }
}

/// Parse the end of the PPF2 or PPF3 patch data. PPF2 and PPF3 files may
/// have an **optional** footer with the following structure:
///
/// `"@BEGIN_FILE_ID.DIZ" BODY "@END_FILE_ID.DIZ" BODY_LENGTH`
///
/// where BODY_LENGTH cannot exceed 3072. BODY_LENGTH is 4 bytes long in
/// PPF2 files and 2 bytes long in PPF3. (It's not clear why 4 bytes were
/// reserved for this purpose when 2 would suffice; the PPF3 docs don't say.)
///
/// The PPF3 documentation refers to the BODY as a file_id, FILE_ID.DIZ, or
/// FILE_ID.DIZ file. Because of this lack of consistency and the potential
/// ambiguity with the term "file_id area", this code uses the terms "footer"
/// and "body" instead.
fn is_end_of_patch(
  patch: &mut PositionTracker<&mut (impl BufRead + Seek)>,
  footer_body_len_size: Option<FooterBodyLengthType>,
) -> io::Result<Result<bool, PatchingError>> {
  const MAX_BODY_LEN: u64 = 3072;

  if patch.has_reached_eof()? {
    // No footer.
    return Ok(Ok(true));
  }

  let body_len_size = match footer_body_len_size.map(FooterBodyLengthType::size) {
    None => return Ok(Ok(false)), // The patch can't have a footer.
    Some(footer_body_len_size) => footer_body_len_size,
  };

  if !patch.next_bytes_eq::<{ BEGIN_MAGIC.len() }>(BEGIN_MAGIC)? {
    // The patch may have a footer, but it hasn't been reached.
    return Ok(Ok(false));
  }

  // The start of the footer was found.
  // Validate the body length and the magic string terminating the footer.
  let body_start = patch.position() + BEGIN_MAGIC.len() as u64;
  let body_end = patch.seek(SeekFrom::End(
    -(END_MAGIC.len() as i64 + body_len_size as i64),
  ))?;
  let body_len = try2!(u64::checked_sub(body_end, body_start).ok_or(BadPatch));
  if body_len > MAX_BODY_LEN {
    return Ok(Err(BadPatch));
  }
  if &patch.read_array::<{ END_MAGIC.len() }>()?[..] != END_MAGIC {
    return Ok(Err(BadPatch));
  }
  let expected_body_len: u32 = {
    // Little endian order yields the same numerical value at larger sizes,
    // so a 4 byte buffer can be used for both a body_len_size of 2 and 4.
    let mut buf = [0u8; size_of::<u32>()];
    patch.read_exact(&mut buf[..body_len_size])?;
    u32::from_le_bytes(buf)
  };
  if expected_body_len != (body_len as u32) {
    return Ok(Err(BadPatch));
  }
  if !patch.has_reached_eof()? {
    return Ok(Err(BadPatch));
  }

  Ok(Ok(true))
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
  ) -> io::Result<Result<(), PatchingError>> {
    file.seek(io::SeekFrom::Start(self.region.start.into()))?;
    let file_block: [u8; BLOCK_CHECK_LENGTH as usize] = try2!(file.read_array().map_rom_err()?);
    let validation_block: [u8; BLOCK_CHECK_LENGTH as usize] =
      try2!(patch.read_array().map_patch_err()?);
    if file_block != validation_block {
      return Ok(Err(BadPatch));
    }
    Ok(Ok(()))
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
  type Error = PatchingError;

  fn try_from(value: &[u8; 5]) -> Result<Self, Self::Error> {
    match value {
      b"PPF10" => Ok(Version::V1),
      b"PPF20" => Ok(Version::V2),
      b"PPF30" => Ok(Version::V3),
      _ => Err(BadPatch),
    }
  }
}

impl TryFrom<u8> for Version {
  type Error = PatchingError;

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
  type Error = PatchingError;

  fn try_from(value: u8) -> Result<Self, Self::Error> {
    match value {
      0 => Ok(Self::BIN),
      1 => Ok(Self::GI),
      _ => Err(BadPatch),
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

#[derive(Debug)]
pub enum PatchingError {
  BadPatch,
  WrongInputFile,
  InputFileTooSmall,
}

impl PatchingIOErrors for PatchingError {
  fn bad_patch() -> Self {
    BadPatch
  }

  fn input_file_too_small() -> Self {
    InputFileTooSmall
  }
}

pub trait TryIntoBool {
  fn try_into_bool(self) -> Result<bool, TryIntoBoolError>;
}

impl TryIntoBool for u8 {
  fn try_into_bool(self) -> Result<bool, TryIntoBoolError> {
    match self {
      0 => Ok(false),
      1 => Ok(true),
      _ => Err(TryIntoBoolError(())),
    }
  }
}

#[derive(Clone, Debug)]
pub struct TryIntoBoolError(pub(crate) ());
