use byteorder::{ReadBytesExt, LE};
use checked_range::prelude::CheckedRange;
use read_write_hashers::{HashingReader, HashingWriter};
use read_write_utils::next_bytes_eq;
use read_write_utils::prelude::*;
use result_result_try::try2;
use rompatcher_err::*;
use std::borrow::Cow;
use std::hash::Hasher;
use std::io::prelude::*;
use std::io::SeekFrom;
use std::num::{NonZeroU32, NonZeroU8};
use std::ops::{Deref, DerefMut, Range};
use std::{cmp, io};
use PatchingError::*;

pub const MAGIC: &[u8] = b"PPF";

const BLOCK_CHECK_LENGTH: u16 = 1024;

/// Applies a PPF patch to a ROM.
pub fn patch(
  rom: &mut (impl BufRead + Seek),
  patch: &mut (impl BufRead + Seek),
  output: &mut impl BufWrite,
  strict: bool,
) -> io::Result<Result<(), PatchingError>> {
  let mut patch = Patch(PositionTracker::from_start(patch));
  let mut rom = PositionTracker::from_start(rom);
  let mut hasher = crc32fast::Hasher::new();

  let Format {
    block_check,
    footer_body_len_size,
    has_undo_data,
    offset_size,
  } = try2!(patch.parse_format()?);

  loop {
    let offset: u64 = try2!(patch.read_offset(offset_size)?);
    let hunk_length: u8 = try2!(patch.read_u8().map_patch_err()?);
    let hunk_length: NonZeroU8 = try2!(NonZeroU8::new(hunk_length).ok_or(BadPatch));

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
        .copy_exactly(u64::from(hunk_length.get()), output)
        .map_patch_err()?
    );

    if has_undo_data {
      patch.seek_relative(i64::from(hunk_length.get()))?;
    }

    if try2!(patch.has_reached_end(footer_body_len_size)?) {
      break;
    }
  }

  if rom.position() == 0 {
    return Ok(Err(BadPatch));
  }

  Ok(Ok(()))
}

struct Patch<T>(T);

impl<T: BufRead + Seek> Patch<PositionTracker<&mut T>> {
  /// Parses the PPF header and footer and performs block check validation.
  ///
  /// `patch`'s cursor must be at the start of the file, and `eof` must be the
  /// length of the PPF file.
  ///
  /// If this method returns `Ok`, `patch` will be positioned at the start of
  /// the patch data. No guarantees are made about its cursor position otherwise.
  pub fn parse_format(&mut self) -> io::Result<Result<Format, PatchingError>> {
    // applyppf3 parses the magic string to obtain the version number and
    // ignores the dedicated version byte. However, ROM Patcher JS checks both
    // and throws an error if they don't match. Given the latter's widespread
    // use, it's probably safe to follow its lead.
    let version_string = try2!(self.read_version_string()?);
    let version_byte = try2!(self.read_version_byte()?);
    if version_string != version_byte {
      return Ok(Err(BadPatch));
    }

    // The PPF docs don't specify the encoding of the description or the
    // contents of unused space. In practice, it always seems to be ASCII, with
    // spaces (0x20) or less commonly nul (0x00) as padding. In that case,
    // String::from_utf8_lossy will cast the byte slice without having to copy
    // and modify the string, while str::trim_end will handle trailing spaces.
    // Nul bytes aren't displayed even if they're in the middle of a string.
    let description: [u8; 50] = self.read_array()?;
    let description: Cow<str> = String::from_utf8_lossy(&description);
    let description: &str = description.trim_end();
    log::debug!("PPF patch description: {description}");

    Ok(Ok(match version_string {
      Version::V1 => Format {
        footer_body_len_size: None,
        offset_size: NonZeroU8::new(4).unwrap(),
        has_undo_data: false,
        block_check: None,
      },
      Version::V2 => {
        // File size checks were deprecated in V3 because they were unreliable,
        // but an absent file size might indicate an invalid PPF file.
        try2!(
          NonZeroU32::try_from(try2!(self.read_u32::<LE>().map_patch_err()?)).map_err(|_| BadPatch)
        );
        let crc32 = try2!(self.hash_validation_block()?);
        Format {
          footer_body_len_size: NonZeroU8::new(4),
          offset_size: NonZeroU8::new(4).unwrap(),
          has_undo_data: false,
          block_check: Some(BlockCheck { crc32, region: ImageType::BIN.block_check_range() }),
        }
      }
      Version::V3 => {
        let image_type = try2!(self.read_image_type()?);
        let has_block_check = try2!(
          try2!(self.read_u8().map_patch_err()?)
            .try_into_bool()
            .map_err(|_| BadPatch)
        );
        let has_undo_data = try2!(
          try2!(self.read_u8().map_patch_err()?)
            .try_into_bool()
            .map_err(|_| BadPatch)
        );
        self.seek_relative(1)?; // Unused in V3
        let block_check = match has_block_check {
          false => None,
          true => {
            let crc32 = try2!(self.hash_validation_block()?);
            let region = image_type.block_check_range();
            Some(BlockCheck { crc32, region })
          }
        };
        Format {
          footer_body_len_size: NonZeroU8::new(2),
          offset_size: NonZeroU8::new(8).unwrap(),
          has_undo_data,
          block_check,
        }
      }
    }))
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
  fn has_reached_end(
    &mut self,
    footer_body_len_size: Option<NonZeroU8>,
  ) -> io::Result<Result<bool, PatchingError>> {
    const BEGIN_MAGIC: &[u8] = b"@BEGIN_FILE_ID.DIZ";
    const END_MAGIC: &[u8] = b"@END_FILE_ID.DIZ";
    const MAX_BODY_LEN: u64 = 3072;

    if self.has_reached_eof()? {
      // No footer.
      return Ok(Ok(true));
    }

    let body_len_size = match footer_body_len_size {
      None => return Ok(Ok(false)), // The patch can't have a footer.
      Some(footer_body_len_size) => footer_body_len_size,
    };

    if next_bytes_eq!(self, BEGIN_MAGIC)? {
      // The patch may have a footer, but it hasn't been reached.
      return Ok(Ok(false));
    }

    // The start of the footer was found.
    // Validate the body length and the magic string terminating the footer.
    let body_start = self.position() + BEGIN_MAGIC.len() as u64;
    let body_end = self.seek(SeekFrom::End(
      -(END_MAGIC.len() as i64 + i64::from(body_len_size.get())),
    ))?;
    let body_len = try2!(u64::checked_sub(body_end, body_start).ok_or(BadPatch));
    if body_len > MAX_BODY_LEN {
      return Ok(Err(BadPatch));
    }
    let body_len = body_len as u16; // safe because of the previous check
    if !next_bytes_eq!(self, END_MAGIC)? {
      return Ok(Err(BadPatch));
    }
    let expected_body_len: u32 = {
      // Little endian order yields the same numerical value at larger sizes,
      // so a 4 byte buffer can be used for both a body_len_size of 2 and 4.
      let mut buf = [0u8; size_of::<u32>()];
      self.read_exact(&mut buf[..usize::from(body_len_size.get())])?;
      u32::from_le_bytes(buf)
    };
    if expected_body_len != u32::from(body_len) {
      return Ok(Err(BadPatch));
    }

    if !self.has_reached_eof()? {
      return Ok(Err(BadPatch));
    }

    Ok(Ok(true))
  }
}

impl<T: Read> Patch<PositionTracker<&mut T>> {
  fn hash_validation_block(&mut self) -> io::Result<Result<u32, PatchingError>> {
    let mut hashing_writer = HashingWriter::new(io::sink(), crc32fast::Hasher::new());
    try2!(
      self
        .copy_exactly(u64::from(BLOCK_CHECK_LENGTH), &mut hashing_writer)
        .map_patch_err()?
    );
    Ok(Ok(hashing_writer.into_hasher().finalize()))
  }
}

impl<T: BufRead> Patch<T> {
  fn read_offset(&mut self, size: NonZeroU8) -> io::Result<Result<u64, PatchingError>> {
    let size = usize::from(size.get());
    let mut buf = [0u8; size_of::<u64>()];
    self
      .read_exact(&mut buf[..size])
      .map(|_| buf)
      .map(u64::from_le_bytes)
      .map_patch_err()
  }

  fn read_version_string(&mut self) -> io::Result<Result<Version, PatchingError>> {
    let bytes = try2!(self.read_array::<5>().map_patch_err()?);
    match bytes.as_slice() {
      b"PPF10" => Ok(Ok(Version::V1)),
      b"PPF20" => Ok(Ok(Version::V2)),
      b"PPF30" => Ok(Ok(Version::V3)),
      _ => Ok(Err(BadPatch)),
    }
  }

  fn read_version_byte(&mut self) -> io::Result<Result<Version, PatchingError>> {
    let value = try2!(self.read_u8().map_patch_err()?);
    match value {
      0 => Ok(Ok(Version::V1)),
      1 => Ok(Ok(Version::V2)),
      2 => Ok(Ok(Version::V3)),
      _ => Ok(Err(BadPatch)),
    }
  }

  fn read_image_type(&mut self) -> io::Result<Result<ImageType, PatchingError>> {
    let value = try2!(self.read_u8().map_patch_err()?);
    match value {
      0 => Ok(Ok(ImageType::BIN)),
      1 => Ok(Ok(ImageType::GI)),
      _ => Ok(Err(BadPatch)),
    }
  }
}

impl<T> Deref for Patch<T> {
  type Target = T;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl<T> DerefMut for Patch<T> {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.0
  }
}

/// Details about the format of a PPF file.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Format {
  offset_size: NonZeroU8,
  footer_body_len_size: Option<NonZeroU8>,
  has_undo_data: bool,
  block_check: Option<BlockCheck>,
}

/// A PPF2 or PPF3 block check.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BlockCheck {
  region: CheckedRange<Range<u64>, u64>,
  crc32: u32,
}

/// The PPF format versions.
#[derive(Copy, Clone, Debug, Default, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub enum Version {
  #[default]
  V1,
  V2,
  V3,
}

/// The ROM image types used in block checks.
#[derive(Clone, Copy, Debug, Default, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub enum ImageType {
  #[default]
  BIN,
  GI,
}

impl ImageType {
  pub fn block_check_range(&self) -> CheckedRange<Range<u64>, u64> {
    match self {
      ImageType::BIN => {
        CheckedRange::new(0x9320..(0x9320 + u64::from(BLOCK_CHECK_LENGTH))).unwrap()
      }
      ImageType::GI => CheckedRange::new(0x80A0..(0x80A0 + u64::from(BLOCK_CHECK_LENGTH))).unwrap(),
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
