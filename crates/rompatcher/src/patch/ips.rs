//! Documentation: https://zerosoft.zophar.net/ips.php

use super::{patch_err, rom_err, Error};
use crate::patch::Error::BadPatch;
use byteorder::{BigEndian, ByteOrder, ReadBytesExt, BE};
use read_write_utils::prelude::*;
use std::io;
use std::io::prelude::*;
use std::num;

pub const MAGIC: &[u8] = b"PAT";

const EOF_OFFSET: u32 = u32::from_be_bytes([0, b'E', b'O', b'F']);

/// Applies an IPS patch to a ROM. Returns the size of the patched file.
///
/// If this function succeeds, `patch` and `output`'s seek positions will be at
/// EOF, `rom`'s position is unspecified, and the result is the size of `output`.
///
/// # Errors
/// If the patch is malformed or can't be applied due to not corresponding to
/// the provided ROM, this function returns [`ErrorKind::InvalidData`].
pub fn patch(
  rom: &mut (impl BufRead + Seek),
  patch: &mut impl BufRead,
  output: &mut impl BufWrite,
) -> Result<u64, Error> {
  let mut rom = PositionTracker::from_start(rom);
  let mut patch = PositionTracker::from_start(patch);
  let mut output = PositionTracker::from_start(output);

  if &patch.read_array::<5>().map_err(patch_err)? != b"PATCH" {
    return Err(BadPatch);
  }

  loop {
    let offset: u32 = patch.read_u24::<BE>().map_err(patch_err)?;
    if offset == EOF_OFFSET {
      break;
    }

    // Copy the input file as is until the next patch hunk.
    rom
      .copy_to_other_until(offset.into(), &mut output)
      .map_err(rom_err)?;

    let encoded_hunk_size = patch.read_u16::<BE>().map_err(patch_err)?;
    let hunk_size: i64 = match num::NonZeroU16::new(encoded_hunk_size) {
      Some(hunk_size) => {
        // Patch contains the bytes to write verbatim.
        let hunk_size: u16 = hunk_size.get();
        patch
          .copy_to_other_exactly(u64::from(hunk_size), &mut output)
          .map_err(patch_err)?;
        i64::from(hunk_size)
      }
      None => {
        // The patch contains a 1 byte repeating sequence.
        let pattern_len: u16 = {
          let pattern_len = patch.read_u16::<BE>().map_err(patch_err)?;
          num::NonZeroU16::new(pattern_len).ok_or(BadPatch)?.get()
        };
        let mut data = patch
          .read_u8()
          .map(|byte| io::repeat(byte).take(u64::from(pattern_len)))
          .map_err(patch_err)?;
        data.copy_to_inner_of(&mut output)?;
        i64::from(pattern_len)
      }
    };

    // Skip over the patched bytes in the input file.
    rom.seek_relative_accurate(hunk_size)?;
  }

  match patch
    .optionally(|patch| patch.read_array::<3>())
    .map_err(patch_err)?
    .map(|array| u64::from(BigEndian::read_u24(&array[..])))
  {
    None => {
      // No truncation necessary; copy the rest of the input file.
      if output.position() == 0 {
        // If nothing was written to the output, the patch must be bad.
        // This isn't necessarily the case in the other branch of the match;
        // a patch that only truncates the file could be valid.
        return Err(BadPatch);
      }
      rom.copy_to_other(&mut output)?;
    }
    Some(truncated_size) => {
      // The patch specifies a truncated size for the output file.
      // The new EOF should be further than the last change in the patch,
      // and the patch must now be at EOF.
      if truncated_size < output.position() || !patch.has_reached_eof()? {
        return Err(BadPatch);
      }
      rom
        .copy_to_other_until(truncated_size, &mut output)
        .map_err(rom_err)?;
    }
  };

  Ok(output.position())
}
