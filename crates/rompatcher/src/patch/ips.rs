//! Documentation: https://zerosoft.zophar.net/ips.php

use byteorder::{BigEndian, ByteOrder, ReadBytesExt, BE};
use read_write_utils::prelude::*;
use result_result_try::try2;
use rompatcher_err::prelude::*;
use std::io;
use std::io::prelude::*;
use std::num;
use PatchingError::*;

pub const MAGIC: &[u8] = b"PAT";

const EOF_OFFSET: u32 = u32::from_be_bytes([0, b'E', b'O', b'F']);

/// Applies an IPS patch to a ROM. Returns the size of the patched file.
///
/// If this function succeeds, `patch` and `output`'s seek positions will be at
/// EOF, `rom`'s position is unspecified, and the result is the size of `output`.
///
/// # Errors
/// If the patch is invalid or can't be applied to the input file, a .
pub fn patch(
  rom: &mut (impl BufRead + Seek),
  patch: &mut impl BufRead,
  output: &mut impl BufWrite,
) -> io::Result<Result<u64, PatchingError>> {
  let mut rom = PositionTracker::from_start(rom);
  let mut patch = PositionTracker::from_start(patch);
  let mut output = PositionTracker::from_start(output);

  if &(try2!(patch.read_array::<5>().map_patch_err()?)) != b"PATCH" {
    return Ok(Err(BadPatch));
  }

  loop {
    let offset: u32 = try2!(patch.read_u24::<BE>().map_patch_err()?);
    if offset == EOF_OFFSET {
      break;
    }

    // Copy the input file as is until the next patch hunk.
    try2!(
      rom
        .copy_to_other_until(offset.into(), &mut output)
        .map_rom_err()?
    );

    let encoded_hunk_size = try2!(patch.read_u16::<BE>().map_patch_err()?);
    let hunk_size: num::NonZeroU16 = match num::NonZeroU16::new(encoded_hunk_size) {
      Some(hunk_size) => {
        // Patch contains the bytes to write verbatim.
        try2!(
          patch
            .copy_to_other_exactly(u64::from(hunk_size.get()), &mut output)
            .map_patch_err()?
        );
        hunk_size
      }
      None => {
        // The patch contains a 1 byte repeating sequence.
        let pattern_len: num::NonZeroU16 = {
          let pattern_len = try2!(patch.read_u16::<BE>().map_patch_err()?);
          try2!(num::NonZeroU16::new(pattern_len).ok_or(BadPatch))
        };
        let mut data = try2!(
          patch
            .read_u8()
            .map(|byte| io::repeat(byte).take(u64::from(pattern_len.get())))
            .map_patch_err()?
        );
        data.copy_to_inner_of(&mut output)?;
        pattern_len
      }
    };

    // Skip over the patched bytes in the input file.
    rom.seek_relative(i64::from(hunk_size.get()))?;
  }

  match try2!(
    patch
      .optionally(|patch| patch.read_array::<3>())
      .map_patch_err()?
  )
  .map(|array| u64::from(BigEndian::read_u24(&array[..])))
  {
    None => {
      if output.position() == 0 {
        // If nothing was written to the output, the patch must be bad.
        // This isn't necessarily the case in the other branch of the match;
        // a patch that only truncates the file could be valid.
        return Ok(Err(BadPatch));
      }
      // No truncation necessary; copy the rest of the input file.
      rom.copy_to_other(&mut output)?;
    }
    Some(truncated_size) => {
      // The patch specifies a truncated size for the output file.
      // The new EOF should be further than the last change in the patch,
      // and the patch must now be at EOF.
      if truncated_size < output.position() || !patch.has_reached_eof()? {
        return Ok(Err(BadPatch));
      }
      try2!(
        rom
          .copy_to_other_until(truncated_size, &mut output)
          .map_rom_err()?
      );
    }
  };

  Ok(Ok(output.position()))
}

#[derive(Debug, thiserror::Error)]
pub enum PatchingError {
  #[error("The patch file is corrupt.")]
  BadPatch,
  #[error(
    "The patch is not meant for this file, and can't be applied due to the file being too small."
  )]
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
