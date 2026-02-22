//! Format documentation: https://www.romhacking.net/documents/392/

use crate::crc::{CRC32Hasher, Crc32};
use crate::patch::byuu::varint::{DecodingError, ReadNumber};
use crate::patch::byuu::*;
use aligned_vec::{avec, AVec, CACHELINE_ALIGN};
use byteorder::{ReadBytesExt, LE};
use ::rayon::prelude::*;
use read_write_utils::hash::{HashingReader, HashingWriter};
use read_write_utils::prelude::*;
use result_result_try::try2;
use rompatcher_err::prelude::*;
use std::cmp::Ordering;
use std::io::prelude::*;
use std::io::ErrorKind::Interrupted;
use std::{io, iter};
use wide::u8x16;
use PatchingError as E;
use PatchingError::*;

pub const MAGIC: &[u8] = b"UPS";

const SIMD_SIZE: usize = u8x16::LANES as usize;

pub fn patch(
  rom: &mut impl BufRead,
  patch: &mut (impl BufRead + Seek),
  output: &mut impl BufWrite,
  strict: bool,
) -> io::Result<Result<PatchReport, PatchingError>> {
  let start_of_footer: u64 = try2!(
    patch
      .seek(io::SeekFrom::End(-(FOOTER_LEN as i64)))
      .map_patch_err::<E>()?
  );
  patch.seek(io::SeekFrom::Start(0))?;

  let mut rom = PositionTracker::from_start(HashingReader::new(rom, CRC32Hasher::new()));
  let mut patch = PositionTracker::from_start(HashingReader::new(patch, CRC32Hasher::new()));
  let mut output = PositionTracker::from_start(HashingWriter::new(output, CRC32Hasher::new()));

  if &(try2!(patch.read_array::<4>().map_patch_err::<PatchingError>()?)) != b"UPS1" {
    return Ok(Err(BadPatch));
  }

  let expected_source_size: u64 = try2!(patch.read_number()?);
  let expected_target_size: u64 = try2!(patch.read_number()?);

  let patch_result = apply_patch(
    &mut rom,
    &mut patch,
    &mut output,
    &start_of_footer,
    expected_target_size,
  );

  // Check if the patch is valid before returning any errors from apply_patch.
  // An InputFileTooSmall error is a false positive if the patch is corrupt.
  try2!(
    patch
      .copy_until(start_of_footer, &mut io::sink())
      .map_patch_err::<E>()?
  );
  let expected_source_crc32 = Crc32::new(try2!(patch.read_u32::<LE>().map_patch_err::<E>()?));
  let expected_target_crc32 = Crc32::new(try2!(patch.read_u32::<LE>().map_patch_err::<E>()?));
  let patch_internal_crc32 = patch.hasher().finish();
  let expected_patch_crc32 = Crc32::new(try2!(patch.read_u32::<LE>().map_patch_err::<E>()?));
  let patch_whole_file_crc32 = patch.hasher().finish();
  if patch_internal_crc32 != expected_patch_crc32 {
    return Ok(Err(BadPatch));
  }
  try2!(patch_result?);

  // Patching succeeded.

  let actual_target_crc32 = output.hasher().finish();
  let actual_target_size = output.position();
  if actual_target_size != expected_target_size {
    return Ok(Err(BadPatch));
  }

  // Read and hash the rest of the file.
  rom.copy_to(&mut io::sink())?;
  let actual_source_crc32 = rom.hasher().finish();
  let actual_source_size = rom.position();

  if strict {
    if actual_source_crc32 != expected_source_crc32 || actual_source_size != expected_source_size {
      return if actual_source_crc32 == expected_target_crc32 {
        Ok(Err(AlreadyPatched))
      } else {
        Ok(Err(WrongInputFile))
      };
    }

    if actual_target_crc32 != expected_target_crc32 {
      // If the source checksum matches but the output checksum doesn't, assume
      // the input file is wrong but its checksum collided with the correct file
      // by chance. That's more likely than a corrupted patch having a checksum
      // collision AND passing every single validation check up to this point.
      return Ok(Err(WrongInputFile));
    }
  }

  Ok(Ok(PatchReport {
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
  }))
}

fn apply_patch(
  mut rom: &mut PositionTracker<HashingReader<&mut impl BufRead, CRC32Hasher>>,
  mut patch: &mut PositionTracker<HashingReader<&mut (impl BufRead + Seek), CRC32Hasher>>,
  mut output: &mut PositionTracker<HashingWriter<&mut impl BufWrite, CRC32Hasher>>,
  start_of_footer: &u64,
  expected_target_size: u64,
) -> io::Result<Result<(), PatchingError>> {
  let mut output_buf: AVec<u8> = avec![];
  let mut is_subsequent_iteration = false;
  loop {
    let relative_offset: u64 = try2!(patch.read_number()?);
    try2!(
      rom
        // As a minor optimization, apply_patch_block doesn't XOR the 0x00
        // delimiter with the corresponding ROM byte. Therefore, one extra byte
        // needs to be copied on subsequent iterations.
        .copy_to_other_exactly(
          u64::from(is_subsequent_iteration) + relative_offset,
          &mut output,
        )
        .map_rom_err::<E>()?
    );
    try2!(apply_patch_block(
      &mut rom,
      &mut patch,
      &mut output,
      &mut output_buf
    )?);
    match patch.position().cmp(&start_of_footer) {
      Ordering::Less => is_subsequent_iteration = true,
      Ordering::Equal => break, // reached the footer
      Ordering::Greater => return Ok(Err(BadPatch)),
    }
  }

  try2!(
    rom
      .copy_to_other_until(expected_target_size, &mut output)
      .map_rom_err::<E>()?
  );

  Ok(Ok(()))
}

/// Applies a single patch block (offset + XOR bytes) to the output.
fn apply_patch_block(
  rom: &mut impl BufRead,
  patch: &mut impl BufRead,
  output: &mut impl BufWrite,
  output_buf: &mut AVec<u8>,
) -> io::Result<Result<(), PatchingError>> {
  // Keep XORing the patch's read buffer with the  corresponding ROM bytes until
  // the end-of-block delimiter (0x00) is found.
  loop {
    let patch_read_buf: &[u8] = match patch.fill_buf() {
      Ok(buf) if buf.is_empty() => return Ok(Err(BadPatch)), // EOF
      Ok(buf) => buf,
      Err(e) if e.kind() == Interrupted => continue,
      Err(e) => return Err(e),
    };

    // memchr uses SIMD to efficiently find the 1st occurrence of 0x00.
    let (patch_bytes, reached_end_of_block) = ::memchr::memchr(0x00, patch_read_buf)
      .map(|i| (&patch_read_buf[..i], true))
      .unwrap_or_else(|| (patch_read_buf, false));

    let patch_xor_rom_bytes: &[u8] = {
      output_buf.resize(patch_bytes.len(), 0x00);
      let rom_bytes = &mut output_buf[..];
      rom.chain(io::repeat(0x00)).copy_to_slice(rom_bytes)?;
      xor_slices(patch_bytes, rom_bytes);
      output_buf.as_slice()
    };

    output.write_all(patch_xor_rom_bytes)?;

    // If the delimiter was found, add 1 to consume it as well.
    let consume_amt = patch_bytes.len() + usize::from(reached_end_of_block);
    patch.consume(consume_amt);

    if reached_end_of_block {
      break;
    }
  }

  Ok(Ok(()))
}

/// Uses [`rayon`] to XOR `patch_hunk` and `rom_hunk` in parallel.
/// `rom_hunk` should be aligned to the cache line size to prevent false sharing.
fn xor_slices(patch_bytes: &[u8], rom_bytes: &mut [u8]) {
  debug_assert_eq!(patch_bytes.len(), rom_bytes.len());
  patch_bytes
    .par_chunks(CACHELINE_ALIGN)
    .zip(rom_bytes.par_chunks_mut(CACHELINE_ALIGN))
    .for_each(xor_cache_line);
}

/// Uses SIMD to XOR two cache line-sized slices together.
fn xor_cache_line((patch_cache_line, output_cache_line): (&[u8], &mut [u8])) {
  debug_assert_eq!(patch_cache_line.len(), output_cache_line.len());
  iter::zip(
    patch_cache_line.chunks(SIMD_SIZE),
    output_cache_line.chunks_mut(SIMD_SIZE),
  )
  .for_each(xor_simd);
}

fn xor_simd((patch_chunk, output_chunk): (&[u8], &mut [u8])) {
  debug_assert_eq!(patch_chunk.len(), output_chunk.len());
  fn to_simd(chunk: &[u8]) -> u8x16 {
    let mut buffer = [0u8; SIMD_SIZE];
    buffer[..chunk.len()].copy_from_slice(chunk);
    u8x16::new(buffer)
  }
  let result: [u8; 16] = (to_simd(patch_chunk) ^ to_simd(output_chunk)).to_array();
  let result: &[u8] = &result[..output_chunk.len()];
  output_chunk.copy_from_slice(result);
}

#[derive(Debug, thiserror::Error)]
pub enum PatchingError {
  #[error("The patch file is corrupt.")]
  BadPatch,
  #[error("The patch is not meant for this file.")]
  WrongInputFile,
  #[error(
    "The patch is not meant for this file, and can't be applied due to the file being too small."
  )]
  InputFileTooSmall,
  #[error("This patch has already been applied to the input file.")]
  AlreadyPatched,
}

impl From<DecodingError> for PatchingError {
  fn from(_: DecodingError) -> Self {
    BadPatch
  }
}

impl PatchingIOErrors for PatchingError {
  fn bad_patch() -> Self {
    BadPatch
  }

  fn input_file_too_small() -> Self {
    InputFileTooSmall
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  pub fn test_xor_simd() {
    let patch_chunk = &[0xEBu8, 0x17];
    let rom_chunk = &mut [0x38u8, 0xAB];
    xor_simd((patch_chunk, rom_chunk));
    assert_eq!(rom_chunk, &[0xEBu8 ^ 0x38u8, 0x17 ^ 0xAB]);
  }
}
