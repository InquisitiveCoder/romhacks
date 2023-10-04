use crate::convert::prelude::*;
use crate::error::prelude::*;
use crate::io::prelude::*;
use crate::{io, mem};
use std::fmt::Formatter;
use std::num;

const BLOCK_CHECK_LENGTH: usize = 1024;

/// Applies a PPF patch to a ROM.
pub fn patch(
  rom: &mut (impl Read + Write + Seek),
  ppf: &mut (impl Read + Seek),
) -> Result<(), Error> {
  // This value isn't needed yet, but it's better to obtain it now since doing
  // so later might discard the internal buffer of the BufReader.
  let eof: u64 = ppf.seek(io::SeekFrom::End(0))?;
  ppf.seek(io::SeekFrom::Start(0))?;
  let mut ppf = io::BufReader::new(ppf);

  let format = Format::parse_and_validate(&mut ppf, rom, eof)?;
  format.apply_patch(&mut ppf, rom)?;
  Ok(())
}

/// Details about the format of a PPF file.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Format {
  patch_range: std::ops::Range<u64>,
  rom_offset_type: RomOffsetType,
  has_undo_data: bool,
}

impl Format {
  /// Parses the PPF header and footer and performs block check validation.
  ///
  /// `ppf`'s cursor must be at the start of the file, and `eof` must be the
  /// length of the PPF file.
  ///
  /// If this method returns `Ok`, `ppf` will be positioned at the start of the
  /// patch data. No guarantees are made about its cursor position otherwise.
  pub fn parse_and_validate(
    ppf: &mut io::BufReader<impl Read + Seek>,
    rom: &mut (impl Read + Seek),
    eof: u64,
  ) -> Result<Format, Error> {
    // applyppf3 parses the magic string to obtain the version number and
    // ignores the dedicated version byte. However, ROM Patcher JS checks both
    // and throws an error if they don't match. Given the latter's widespread
    // use, it's probably safe to follow its lead.
    let version = Version::try_from(&ppf.read_array::<5>()?)?;
    if version != Version::try_from(ppf.read_u8()?)? {
      return Err(Error::Version);
    }

    const DESC_LEN: usize = 50;
    // The PPF docs don't specify the encoding of the description or the
    // contents of unused space. In practice it always seems to be ASCII, with
    // spaces (0x20) or (less commonly) nul (0x00) as padding. In that case,
    // String::from_utf8_lossy will cast the byte slice without having to copy
    // and modify the string, while str::trim_end will handle trailing spaces.
    // Nul bytes aren't displayed even if they're in the middle of a string.
    log::debug!(
      "PPF patch description: {}",
      String::from_utf8_lossy(&ppf.buffer()[..DESC_LEN]).trim_end()
    );
    // Because the description was read directly from the byte buffer, the
    // BufReader's position still needs to be updated.
    ppf.seek_relative(DESC_LEN as i64)?;

    Ok(match version {
      Version::V1 => Format {
        patch_range: 56..eof,
        rom_offset_type: RomOffsetType::U32,
        has_undo_data: false,
      },
      Version::V2 => {
        // File size checks were deprecated in V3 because they were unreliable,
        // but an absent file size might indicate an invalid PPF file.
        num::NonZeroU32::try_from(ppf.read_u32::<LE>()?).map_err(|_| Error::ExpectedFileSize)?;
        BlockCheck(ImageType::BIN).validate(ppf, rom)?;
        let pos: u64 = 60 + BLOCK_CHECK_LENGTH as u64;
        let end_of_patch = Self::find_end_of_patch(ppf, FooterBodyLengthType::U32, pos..eof)?;
        Format {
          patch_range: pos..end_of_patch,
          rom_offset_type: RomOffsetType::U32,
          has_undo_data: false,
        }
      }
      Version::V3 => {
        let image_type = ImageType::try_from(ppf.read_u8()?)?;
        let has_block_check = (ppf.read_u8()?)
          .try_into_bool()
          .map_err(|_| Error::BlockCheckFlag)?;
        let has_undo_data = (ppf.read_u8()?)
          .try_into_bool()
          .map_err(|_| Error::UndoDataFlag)?;
        ppf.seek_relative(1)?; // Unused in V3
        let pos: u64 = 60 + (has_block_check as u64 * BLOCK_CHECK_LENGTH as u64);
        if has_block_check {
          BlockCheck(image_type).validate(ppf, rom)?;
        }
        let end_of_patch = Self::find_end_of_patch(ppf, FooterBodyLengthType::U16, pos..eof)?;
        Format {
          patch_range: pos..end_of_patch,
          rom_offset_type: RomOffsetType::U64,
          has_undo_data,
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
    ppf: &mut io::BufReader<R>,
    body_len_type: FooterBodyLengthType,
    range: std::ops::Range<u64>,
  ) -> Result<u64, Error> {
    const BEGIN_MAGIC: &[u8] = b"@BEGIN_FILE_ID.DIZ";
    const END_MAGIC: &[u8] = b"@END_FILE_ID.DIZ";
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
    // to refill it with the last ppf.capacity() bytes so that we can backtrack
    // within the buffer instead of seeking backwards beyond the start of the
    // buffer and performing an additional read.
    let end_buf_pos: u64 = if range.end > ppf.capacity() as u64 {
      // The buffer needs to be empty for BufReader::fill_buf to refill it;
      // BufReader::seek will always discard the buffer.
      let pos: u64 = ppf.seek(io::SeekFrom::End(-(ppf.capacity() as i64)))?;
      ppf.fill_buf()?;
      pos
    } else {
      range.start
    };

    // All of the following relative seeks (except possibly the final seek back
    // to the start of the patch region) should fall within the buffer.

    // Seek to the end-of-footer magic string.
    let end_magic_pos: u64 = range.end - footer_end_len;
    ppf.seek_relative((end_magic_pos - end_buf_pos) as i64)?;

    let seek_to_start = |ppf: &mut io::BufReader<R>, pos: u64| -> io::Result<()> {
      if range.start >= end_buf_pos {
        // The start of the patch area falls within the read buffer.
        // Perform a relative seek to keep the buffer.
        ppf.seek_relative(range.start as i64 - pos as i64)
      } else {
        // The start of the patch area isn't in the buffer, so the buffer will
        // be discarded regardless of how we seek. An absolute seek is simpler
        // and avoids overflow issues when calculating this offset.
        ppf.seek(io::SeekFrom::Start(range.start))?;
        Ok(())
      }
    };

    let buf = mem::try_init([0u8; END_MAGIC.len()], |buf| ppf.read_exact(&mut buf[..]))?;
    // If there's no footer, seek back to the start of the patch data and return
    // EOF. This is the most common case.
    if buf != END_MAGIC {
      seek_to_start(ppf, end_magic_pos)?;
      return Ok(range.end);
    }

    let body_len: u32 = {
      // Little endian order yields the same numerical value at larger sizes,
      // so a 4 byte buffer can be used for both a body_len_size of 2 and 4.
      let mut buf = [0u8; mem::size_of::<u32>()];
      ppf.read_exact(&mut buf[..body_len_size])?;
      u32::from_le_bytes(buf)
    };
    let footer_len: u64 =
      BEGIN_MAGIC.len() as u64 + body_len as u64 + END_MAGIC.len() as u64 + body_len_size as u64;
    if body_len > MAX_BODY_LENGTH || footer_len > remaining {
      // If the body length stored in the file is larger than the max defined
      // in the PPF specs, or it's larger than the non-header region of the
      // file, the file is probably corrupt.
      return Err(Error::FooterLength);
    }

    ppf.seek_relative(-(footer_len as i64))?;
    let buf = mem::try_init([0u8; BEGIN_MAGIC.len()], |buf| ppf.read_exact(&mut buf[..]))?;
    if buf != BEGIN_MAGIC {
      // If the file contains an end-of-footer string without a matching
      // start-of-footer string, the file is probably corrupt.
      return Err(Error::FooterLength);
    }

    // Found the footer. Seek back to the start of the patch.
    let footer_pos = range.end - footer_len;
    let current_pos = footer_pos + BEGIN_MAGIC.len() as u64;
    seek_to_start(ppf, current_pos)?;
    Ok(footer_pos)
  }

  pub fn apply_patch(
    self: Format,
    ppf: &mut io::BufReader<impl Read + Seek>,
    rom: &mut (impl Read + Write + Seek),
  ) -> Result<(), Error> {
    let Format { patch_range, rom_offset_type, has_undo_data } = self;
    let mut ppf = ppf.take(patch_range.end - patch_range.start);
    let mut rom = io::BufWriter::new(rom);
    let mut rom_offset: u64 = 0;

    loop {
      let offset = u64::from_le_bytes(mem::try_init([0u8; mem::size_of::<u64>()], |buf| {
        ppf.read_exact(&mut buf[..rom_offset_type.size()])
      })?);

      let hunk_length: u64 = match num::NonZeroU8::new(ppf.read_u8()?) {
        Some(x) => x.get() as u64,
        None => Err(Error::EmptyHunk)?,
      };

      // Seeking will flush the buffer so we don't want to do it if we're
      // already at the correct position. This can happen if the patch needs to
      // modify more than 255 bytes in a row.
      if rom_offset != offset {
        rom.seek(io::SeekFrom::Start(offset.into()))?;
        rom_offset = offset;
      }

      io::copy(&mut ((&mut ppf).take(hunk_length)), &mut rom)?;
      rom_offset += hunk_length;

      if has_undo_data {
        // The Take adapter doesn't implement Seek, so discard the bytes into Sink.
        io::copy(&mut (&mut ppf).take(hunk_length), &mut io::sink())?;
      }

      if ppf.limit() == 0 {
        break;
      }
    }

    rom.flush()?;
    Ok(())
  }
}

/// A PPF2 or PPF3 block check.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct BlockCheck(ImageType);

impl BlockCheck {
  pub fn validate(&self, ppf: &mut impl Read, file: &mut (impl Read + Seek)) -> Result<(), Error> {
    file.seek(io::SeekFrom::Start(
      self.0.block_check_offset().get().into(),
    ))?;
    let file_block: [u8; BLOCK_CHECK_LENGTH] = file.read_array()?;
    let validation_block: [u8; BLOCK_CHECK_LENGTH] = ppf.read_array()?;
    if file_block != validation_block {
      Err(Error::BlockCheckFailed)?;
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
  type Error = Error;

  fn try_from(value: &[u8; 5]) -> Result<Self, Self::Error> {
    match value {
      b"PPF10" => Ok(Version::V1),
      b"PPF20" => Ok(Version::V2),
      b"PPF30" => Ok(Version::V3),
      _ => Err(Error::Version),
    }
  }
}

impl TryFrom<u8> for Version {
  type Error = Error;

  fn try_from(value: u8) -> Result<Self, Self::Error> {
    match value {
      0 => Ok(Version::V1),
      1 => Ok(Version::V2),
      2 => Ok(Version::V3),
      _ => Err(Error::Version),
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
  pub fn block_check_offset(&self) -> num::NonZeroU16 {
    match self {
      ImageType::BIN => num::NonZeroU16::new(0x9320).unwrap(),
      ImageType::GI => num::NonZeroU16::new(0x80A0).unwrap(),
    }
  }
}

impl TryFrom<u8> for ImageType {
  type Error = Error;

  fn try_from(value: u8) -> Result<Self, Self::Error> {
    match value {
      0 => Ok(Self::BIN),
      1 => Ok(Self::GI),
      _ => Err(Error::ImageType),
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
      Self::U16 => mem::size_of::<u16>(),
      Self::U32 => mem::size_of::<u32>(),
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
      Self::U32 => mem::size_of::<u32>(),
      Self::U64 => mem::size_of::<u64>(),
    }
  }
}

#[derive(Debug, Error)]
pub enum Error {
  #[error("The PPF header has an invalid version number.")]
  Version,
  #[error("PPF 3.0 header has an invalid image type flag.")]
  ImageType,
  #[error("PPF 3.0 header has an invalid block check flag.")]
  BlockCheckFlag,
  #[error("PPF 3.0 header has an invalid undo data flag.")]
  UndoDataFlag,
  #[error("PPF 2.0 header has an expected file size of 0 bytes. The PPF file may be corrupt.")]
  ExpectedFileSize,
  #[error("The length field of the file_id area is invalid. The PPF file may be corrupt.")]
  FooterLength,
  #[error("The ROM failed block check validation. This PPF file is not intended for this ROM.")]
  BlockCheckFailed,
  #[error("Encountered a hunk with a length of 0 bytes. The PPF file may be corrupt.")]
  EmptyHunk,
  #[error(transparent)]
  IO(#[from] io::Error),
}
