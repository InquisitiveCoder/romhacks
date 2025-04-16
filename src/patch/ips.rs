use super::map_io_err;
use crate::io_utils::prelude::*;
use byteorder::{BigEndian, ByteOrder};
use std::io;
use std::io::ErrorKind::*;
use std::io::prelude::*;
use std::num;

// Documentation: https://zerosoft.zophar.net/ips.php

pub const MAGIC: &[u8] = b"PAT";

const EOF_OFFSET: u32 = u32::from_be_bytes([0, b'E', b'O', b'F']);

/// Applies an IPS patch to a ROM. Returns the size of the patched file.
///
/// If this function succeeds, `patch` and `output`'s seek positions will be at
/// EOF; `rom`'s position will either be at EOF or the truncated output size
/// specified in the patch.
///
/// # Errors
/// If the patch is malformed or can't be applied due to not corresponding to
/// the provided ROM, this function returns [`ErrorKind::InvalidData`].
pub fn patch(
  rom: &mut impl BufRead,
  patch: &mut impl BufRead,
  output: &mut impl CopyWriteBuf,
) -> io::Result<u64> {
  _patch(rom, patch, output).map_err(map_io_err)
}

fn _patch(
  rom: &mut impl BufRead,
  patch: &mut impl BufRead,
  output: &mut impl CopyWriteBuf,
) -> io::Result<u64> {
  let mut rom = PositionTracker::from_start(rom);
  let mut patch = PositionTracker::from_start(patch);

  if &patch.read_array::<5>()? != b"PATCH" {
    return Err(io::Error::from(InvalidData));
  }

  loop {
    let offset: u32 = patch.read_u24::<BE>()?;
    if offset == EOF_OFFSET {
      break;
    }
    rom
      .until_position(offset as u64)?
      .exact(|rom| rom.copy_using_writer_buf(output))?;
    match num::NonZeroU16::new(patch.read_u16::<BE>()?) {
      Some(hunk_size) => {
        (&mut patch)
          .take(hunk_size.get().into())
          .exact(|patch| patch.copy_using_writer_buf(output))?;
      }
      None => {
        let size = num::NonZeroU16::new(patch.read_u16::<BE>()?).ok_or(InvalidData)?;
        io::repeat(patch.read_u8()?)
          .take(size.get().into())
          .exact(|repeat| repeat.copy_using_writer_buf(output))?;
      }
    }
  }

  match patch
    .optionally(|patch| patch.read_array::<3>())?
    .map(|arr| BigEndian::read_u24(&arr[..]) as u64)
  {
    Some(new_size) => {
      rom
        .until_position(new_size)?
        .exact(|rom| rom.copy_using_writer_buf(output))?;
    }
    None => {
      rom.copy_using_writer_buf(output)?;
    }
  };

  if patch.fill_buf()?.is_empty() {
    Ok(rom.position())
  } else {
    Err(io::Error::from(InvalidData))
  }
}
