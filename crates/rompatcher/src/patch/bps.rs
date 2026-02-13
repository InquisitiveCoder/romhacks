use crate::crc::{CRC32Hasher, Crc32};
use crate::patch;
use crate::patch::byuu::varint::ReadNumber;
use crate::patch::byuu::PatchReport;
use crate::patch::Error::{AlreadyPatched, BadPatch, WrongInputFile};
use crate::patch::{patch_err, rom_err, Error};
use byteorder::{LittleEndian, ReadBytesExt};
use read_write_utils::hash::{HashingReader, HashingWriter, MonotonicHashingReader};
use read_write_utils::prelude::*;
use read_write_utils::repeat::RepeatSlice;
use std::cmp::Ordering;
use std::io;
use std::io::prelude::*;
use std::io::SeekFrom;
use std::num::NonZeroU64;

pub const MAGIC: &[u8] = b"BPS";

pub fn patch<O>(
  rom: &mut (impl BufRead + Seek),
  patch: &mut (impl BufRead + Seek),
  output_file: &mut O,
  strict: bool,
) -> Result<PatchReport, patch::Error>
where
  O: BufWrite + Seek,
  for<'a> &'a mut O::Inner: Read + Write + Seek,
{
  let start_of_footer: u64 = patch
    .seek(SeekFrom::End(-(patch::byuu::FOOTER_LEN as i64)))
    .map_err(patch_err)?;
  patch.seek(SeekFrom::Start(0))?;

  let mut rom = PositionTracker::from_start(MonotonicHashingReader::new(rom, CRC32Hasher::new()));
  let mut patch = PositionTracker::from_start(HashingReader::new(patch, CRC32Hasher::new()));
  let mut output = PositionTracker::from_start(HashingWriter::new(output_file, CRC32Hasher::new()));

  if &patch.read_array::<4>().map_err(patch_err)? != b"BPS1" {
    return Err(BadPatch);
  }

  let expected_source_size: u64 = patch.read_number().map_err(patch_err)?;
  let expected_target_size: u64 = patch.read_number().map_err(patch_err)?;
  let metadata_size: u64 = patch.read_number().map_err(patch_err)?;
  // Skip over the metadata, but still hash its contents.
  patch.copy_exactly(metadata_size, &mut io::sink())?;

  let patch_result = apply_patch(
    &mut rom,
    &mut patch,
    &mut output,
    &start_of_footer,
    expected_source_size,
  );

  // Check if the patch is valid before returning any errors from apply_patch.
  // An InputFileTooSmall error is a false positive if the patch is corrupt.
  patch.copy_until(start_of_footer, &mut io::sink())?;
  let expected_source_crc32 = Crc32::new(patch.read_u32::<LittleEndian>().map_err(patch_err)?);
  let expected_target_crc32 = Crc32::new(patch.read_u32::<LittleEndian>().map_err(patch_err)?);
  let patch_internal_crc32 = patch.hasher().finish();
  let expected_patch_crc32 = Crc32::new(patch.read_u32::<LittleEndian>().map_err(patch_err)?);
  let patch_whole_file_crc32 = patch.hasher().finish();
  if patch_internal_crc32 != expected_patch_crc32 {
    return Err(BadPatch);
  }
  patch_result?;

  // Patching succeeded.

  let actual_target_crc32 = output.hasher().finish();
  let actual_target_size = output.position();
  if actual_target_size != expected_target_size {
    return Err(BadPatch);
  }

  // Read and hash the rest of the file.
  rom.copy_to(&mut io::sink())?;
  let actual_source_crc32 = rom.hasher().finish();
  let actual_source_size = rom.position();

  if strict {
    if actual_source_crc32 != expected_source_crc32 || actual_source_size != expected_source_size {
      return if actual_source_crc32 == expected_target_crc32 {
        Err(AlreadyPatched)
      } else {
        Err(WrongInputFile)
      };
    }

    if actual_target_crc32 != expected_target_crc32 {
      // If the source checksum matches but the output checksum doesn't, assume
      // the input file is wrong but its checksum collided with the correct file
      // by chance. That's more likely than a corrupted patch having a checksum
      // collision AND passing every single validation check up to this point.
      return Err(WrongInputFile);
    }
  }

  Ok(PatchReport {
    expected_source_crc32,
    actual_source_crc32,
    expected_target_crc32,
    actual_target_crc32,
    patch_internal_crc32,
    patch_whole_file_crc32,
    expected_source_size,
    actual_source_size,
    expected_target_size,
    actual_target_size,
  })
}

fn apply_patch<O>(
  rom: &mut PositionTracker<MonotonicHashingReader<&mut (impl BufRead + Seek), CRC32Hasher>>,
  patch: &mut PositionTracker<HashingReader<&mut (impl BufRead + Seek), CRC32Hasher>>,
  mut output: &mut PositionTracker<HashingWriter<&mut O, CRC32Hasher>>,
  start_of_footer: &u64,
  expected_source_size: u64,
) -> Result<(), patch::Error>
where
  O: BufWrite + Seek,
  for<'a> &'a mut O::Inner: Read + Write + Seek,
{
  let mut source_relative_offset: u64 = 0;
  let mut target_relative_offset: u64 = 0;
  let mut target_copy_buffer: Vec<u8> = Vec::new();

  loop {
    match patch.decode_command()? {
      Command::SourceRead { length } => {
        eprintln!("source read");
        if output.position() >= expected_source_size {
          return Err(BadPatch);
        }
        rom.seek(SeekFrom::Start(output.position()))?;
        rom
          .copy_to_other_exactly(length.get(), &mut output)
          .map_err(rom_err)?;
      }
      Command::TargetRead { length } => {
        eprintln!("target read");
        patch
          .copy_to_other_exactly(length.get(), &mut output)
          .map_err(patch_err)?;
      }
      Command::SourceCopy { length, offset } => {
        eprintln!("source copy, length: {length}, offset: {offset}");
        source_relative_offset = source_relative_offset
          .checked_add_signed(offset)
          .ok_or(BadPatch)?;
        if source_relative_offset >= expected_source_size {
          return Err(BadPatch);
        }
        rom.seek(SeekFrom::Start(source_relative_offset))?;
        rom
          .copy_to_other_exactly(length.get(), &mut output)
          .map_err(rom_err)?;
        source_relative_offset = source_relative_offset
          .checked_add(length.get())
          .ok_or(BadPatch)?;
      }
      Command::TargetCopy { length, offset } => {
        eprintln!("target copy");
        target_relative_offset = target_relative_offset
          .checked_add_signed(offset)
          .ok_or(BadPatch)?;
        let output_offset = output.position();
        let sequence_period_len: NonZeroU64 = output_offset
          .checked_sub(target_relative_offset)
          .map(|offset_diff| u64::min(offset_diff, length.get()))
          .and_then(NonZeroU64::new)
          .ok_or(BadPatch)?;
        output.seek(SeekFrom::Start(target_relative_offset))?;
        // BufWriters don't support reading, so use the inner writer instead.
        output
          .with_inner(
            |hasher: &mut HashingWriter<_, _>| hasher.inner_mut().get_mut(),
            |output: &mut PositionTracker<&mut O::Inner>| {
              target_copy_buffer
                .reserve(usize::try_from(sequence_period_len.get()).unwrap_or(usize::MAX));
              output.copy_exactly(sequence_period_len.get(), &mut target_copy_buffer)?;
              output.seek(SeekFrom::Start(output_offset))
            },
          )
          .map_err(patch_err)?;

        RepeatSlice::new(&target_copy_buffer[..])
          .take(length.get())
          .copy_to_inner_of(&mut output)?;
        target_copy_buffer.clear();
        target_relative_offset = target_relative_offset
          .checked_add(length.get())
          .ok_or(BadPatch)?;
      }
    }

    match patch.position().cmp(&start_of_footer) {
      Ordering::Less => continue,
      Ordering::Equal => break, // reached the footer
      Ordering::Greater => return Err(BadPatch),
    }
  }

  Ok(())
}

trait ReadBPS: Read + ReadNumber {
  fn decode_command(&mut self) -> Result<Command, Error> {
    let encoded: u64 = self.read_number()?;
    let length = NonZeroU64::new((encoded >> 2) + 1).ok_or(BadPatch)?;
    Ok(match encoded {
      0 => Command::SourceRead { length },
      1 => Command::TargetRead { length },
      2 => Command::SourceCopy { length, offset: self.decode_signed_number()? },
      3 => Command::TargetCopy { length, offset: self.decode_signed_number()? },
      _ => return Err(BadPatch),
    })
  }

  fn decode_signed_number(&mut self) -> io::Result<i64> {
    let encoded: u64 = self.read_number()?;
    // 63 bits always fit in an i64.
    Ok(((encoded >> 1) as i64) * (if encoded & 1 == 1 { -1 } else { 1 }))
  }
}

impl<R: Read> ReadBPS for R {}

enum Command {
  SourceRead { length: NonZeroU64 },
  TargetRead { length: NonZeroU64 },
  SourceCopy { length: NonZeroU64, offset: i64 },
  TargetCopy { length: NonZeroU64, offset: i64 },
}
