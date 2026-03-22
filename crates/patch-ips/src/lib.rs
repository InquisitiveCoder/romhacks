//! IPS format documentation: <https://zerosoft.zophar.net/ips.php>

use byteorder::{ReadBytesExt, BE};
use read_write_utils::prelude::*;
use read_write_utils::read::AmortizedRead;
use read_write_utils::seek::SeekRelative;
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
/// If this function succeeds, `patch` and `output`'s pos positions will be at
/// EOF, `rom`'s position is unspecified, and the result is the size of `output`.
///
/// # Errors
/// If the patch is invalid or can't be applied to the input file, a .
pub fn patch(
  rom: &mut (impl AmortizedRead + SeekRelative),
  patch: &mut impl BufRead,
  output: &mut impl BufWrite,
) -> io::Result<Result<u64, PatchingError>> {
  let rom = PositionTracker::from_start(rom);
  let mut patch = PositionTracker::from_start(patch);
  let output = PositionTracker::from_start(output);
  if try2!(read_array_ne!(patch, b"PATCH").map_patch_err()?) {
    return Ok(Err(BadPatch));
  }
  apply_patch(rom, patch, output)
}

fn apply_patch(
  mut rom: PositionTracker<impl AmortizedRead + SeekRelative>,
  mut patch: PositionTracker<impl BufRead>,
  mut output: PositionTracker<impl BufWrite>,
) -> io::Result<Result<u64, PatchingError>> {
  loop {
    let offset: u32 = try2!(patch.read_u24::<BE>().map_patch_err()?);
    if offset == EOF_OFFSET {
      break;
    }

    // Copy the input file as is until the next patch hunk.
    let rom_copy_result = rom
      .copy_to_inner_until(offset.into(), &mut output)
      .map_rom_err()?;
    if let Err(InputFileTooSmall) = rom_copy_result {
      // If patching is unable to continue because the ROM is too small, replace
      // the input and output with RepeatSlice and Sink, then continue.
      // Only return InputFileTooSmall if the patch appears to be valid.
      let mut rom = PositionTracker::at_position(rom.position(), io::repeat(0));
      let mut output = PositionTracker::at_position(output.position(), io::sink());
      // Finish the copy so that rom and output reach the same position as if
      // the initial copy had succeeded.
      rom
        .copy_to_inner_until(offset.into(), &mut output)
        .expect("A copy from Repeat to Sink shouldn't fail.");
      return match apply_patch(rom, patch, output) {
        Ok(Ok(_)) => Ok(Err(InputFileTooSmall)),
        result => result,
      };
    } else {
      // Propagate other PatchErrors immediately.
      try2!(rom_copy_result);
    }

    let encoded_hunk_size = try2!(patch.read_u16::<BE>().map_patch_err()?);
    let hunk_size: num::NonZeroU16 = match num::NonZeroU16::new(encoded_hunk_size) {
      Some(hunk_size) => {
        // Patch contains the bytes to write verbatim.
        try2!(
          patch
            .copy_to_inner_exactly(u64::from(hunk_size.get()), &mut output)
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
        let byte = try2!(patch.read_u8().map_patch_err()?);
        io::repeat(byte)
          .take(u64::from(pattern_len.get()))
          .copy_to_inner(&mut output)?;
        pattern_len
      }
    };

    // Skip over the patched bytes in the input file.
    rom.relative_seek(i64::from(hunk_size.get()))?;
  }

  let truncated_size = patch
    .if_not_eof(|patch| Ok(u64::from(patch.read_u24::<BE>()?)))
    .map_patch_err();
  match try2!(truncated_size?) {
    None => {
      if output.position() == 0 {
        // If nothing was written to the output, the patch must be bad.
        // This isn't necessarily the case in the other branch of the match;
        // a patch that only truncates the file could be valid.
        return Ok(Err(BadPatch));
      }
      // No truncation necessary; copy the rest of the input file.
      rom.copy_to_inner(&mut output)?;
    }
    Some(truncated_size) => {
      // The patch specifies a truncated size for the output file.
      // The new EOF should be further than the last change in the patch,
      // and the patch must now be at EOF.
      if truncated_size < output.position() || !patch.reached_eof()? {
        return Ok(Err(BadPatch));
      }
      try2!(
        rom
          .copy_to_inner_until(truncated_size, &mut output)
          .map_rom_err()?
      );
    }
  };

  Ok(Ok(output.position()))
}

#[derive(Debug)]
pub enum PatchingError {
  BadPatch,
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
