//! Format documentation: https://www.romhacking.net/documents/392/

use crate::crc::{CRC32Hasher, Crc32};
use crate::patch::byuu::varint::ReadNumber;
use crate::patch::byuu::*;
use crate::patch::Error::{BadPatch, WrongInputFile};
use crate::patch::{patch_err, rom_err, Error};
use aligned_vec::{avec, AVec, CACHELINE_ALIGN};
use byteorder::{LittleEndian, ReadBytesExt};
use ::rayon::prelude::*;
use read_write_utils::hash::{HashingReader, HashingWriter};
use read_write_utils::prelude::*;
use read_write_utils::DEFAULT_BUF_SIZE;
use std::cmp::Ordering;
use std::io::prelude::*;
use std::io::ErrorKind::Interrupted;
use std::ops::{Deref, DerefMut};
use std::{io, iter};
use wide::u8x16;

pub const MAGIC: &[u8] = b"UPS";

const SIMD_SIZE: usize = u8x16::LANES as usize;

pub fn patch(
  rom: &mut impl BufRead,
  patch: &mut (impl BufRead + Seek),
  output: &mut impl BufWrite,
  strict: bool,
) -> Result<PatchReport, Error> {
  let start_of_footer: u64 = patch
    .seek(io::SeekFrom::End(-(FOOTER_LEN as i64)))
    .map_err(patch_err)?;
  patch.seek(io::SeekFrom::Start(0))?;

  let mut rom = PositionTracker::from_start(HashingReader::new(rom, CRC32Hasher::new()));
  let mut patch = PositionTracker::from_start(HashingReader::new(patch, CRC32Hasher::new()));
  let mut output = PositionTracker::from_start(HashingWriter::new(output, CRC32Hasher::new()));

  if &patch.read_array::<4>().map_err(patch_err)? != b"UPS1" {
    return Err(BadPatch);
  }

  let expected_source_size: u64 = patch.read_number().map_err(patch_err)?;
  let expected_target_size: u64 = patch.read_number().map_err(patch_err)?;

  let mut output_buf = CacheAlignedBuf::new();
  loop {
    let relative_offset: u64 = patch.read_number().map_err(patch_err)?;
    rom
      .copy_to_other_exactly(relative_offset, &mut output)
      .map_err(rom_err)?;
    apply_hunk(&mut rom, &mut patch, &mut output, &mut output_buf)?;
    match patch.position().cmp(&start_of_footer) {
      Ordering::Less => continue,
      Ordering::Equal => break, // reached the footer
      Ordering::Greater => return Err(BadPatch),
    }
  }

  rom
    .take_from_inner_until(expected_target_size, |take| {
      take.exactly(|rom| io::copy(rom, &mut output))
    })
    .map_err(|_| BadPatch)?;

  let actual_target_crc32 = output.hasher().finish();

  // Validation

  let actual_target_size = output.position();
  if actual_target_size != expected_target_size {
    return Err(BadPatch);
  }

  let expected_source_crc32 = Crc32::new(patch.read_u32::<LittleEndian>()?);
  let expected_target_crc32 = Crc32::new(patch.read_u32::<LittleEndian>()?);
  // The CRC32 of the patch up to the final 4 bytes.
  let patch_internal_crc32 = patch.hasher().finish();
  let expected_patch_crc32 = Crc32::new(patch.read_u32::<LittleEndian>()?);
  // The CRC32 of the entire patch file.
  let patch_whole_file_crc32 = patch.hasher().finish();

  if patch_internal_crc32 != expected_patch_crc32 {
    return Err(BadPatch);
  }

  // Read and hash the rest of the file. Note that HashingReader::seek will
  // not hash any skipped file contents.
  io::copy(&mut rom, &mut io::sink())?;
  let actual_source_crc32 = rom.hasher().finish();
  let actual_source_size = rom.position();

  if strict {
    if actual_source_crc32 != expected_source_crc32 || actual_source_size != expected_source_size {
      return Err(WrongInputFile);
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

fn apply_hunk(
  rom: &mut impl BufRead,
  patch: &mut impl BufRead,
  output: &mut impl BufWrite,
  output_buf: &mut CacheAlignedBuf,
) -> Result<(), Error> {
  // Each iteration consumes at most one buffer's worth of bytes from the patch
  // until the end of the hunk is found.
  loop {
    // If there is no data on the first iteration, or the end of the hunk
    // section is reached without having found the terminating NUL byte, the
    // patch is corrupt.
    let patch_buf: &[u8] = match patch.fill_buf() {
      Ok(buf) if buf.is_empty() => return Err(BadPatch),
      Ok(buf) => buf,
      Err(e) if e.kind() == Interrupted => continue,
      Err(e) => Err(e)?,
    };

    // The memchr crate uses SIMD to efficiently find the first occurrence of
    // a specified byte in a slice. We take advantage of this to find the NUL
    // byte that terminates the current hunk.
    let (data_len, is_end_of_hunk) = ::memchr::memchr(0, patch_buf)
      .map(|i| (i, true))
      .unwrap_or_else(|| (patch_buf.len(), false));

    output_buf.resize(data_len);
    rom.chain(io::repeat(0)).copy_to_slice(output_buf)?;
    let (patch_hunk, rom_hunk) = (&patch_buf[..data_len], &mut output_buf[..]);
    xor_hunks(patch_hunk, rom_hunk);
    output.write_all(rom_hunk)?;

    // If the delimiter was found, add 1 so it gets consumed too.
    patch.consume(data_len + 1 * (is_end_of_hunk as usize));
    if is_end_of_hunk {
      break;
    }
  }
  Ok(())
}

/// Uses rayon to XOR the `patch_hunk` and `rom_hunk` in parallel.
/// `rom_hunk` should be aligned to the cache line size to prevent false sharing.
fn xor_hunks(patch_hunk: &[u8], rom_hunk: &mut [u8]) {
  debug_assert_eq!(patch_hunk.len(), rom_hunk.len());
  patch_hunk
    .par_chunks(CACHELINE_ALIGN)
    .zip(rom_hunk.par_chunks_mut(CACHELINE_ALIGN))
    .for_each(xor_cache_line);
}

/// Uses SIMD to XOR two equal-length slices together.
fn xor_cache_line((patch_cache_line, rom_cache_line): (&[u8], &mut [u8])) {
  debug_assert_eq!(patch_cache_line.len(), rom_cache_line.len());
  iter::zip(
    patch_cache_line.chunks(SIMD_SIZE),
    rom_cache_line.chunks_mut(SIMD_SIZE),
  )
  .for_each(xor_simd);
}

fn xor_simd((patch_chunk, rom_chunk): (&[u8], &mut [u8])) {
  fn to_simd(chunk: &[u8]) -> u8x16 {
    let mut buffer = [0u8; SIMD_SIZE];
    buffer[..chunk.len()].copy_from_slice(chunk);
    u8x16::new(buffer)
  }
  let result = (to_simd(patch_chunk) ^ to_simd(rom_chunk)).to_array();
  rom_chunk.copy_from_slice(&result[..rom_chunk.len()]);
}

struct CacheAlignedBuf(AVec<u8>);

impl CacheAlignedBuf {
  pub fn new() -> Self {
    Self(avec![0u8; DEFAULT_BUF_SIZE])
  }

  /// Follows the same semantics as [`Vec::resize`].
  pub fn resize(&mut self, size: usize) {
    self.0.resize(size, 0);
  }
}

impl Deref for CacheAlignedBuf {
  type Target = [u8];

  fn deref(&self) -> &Self::Target {
    &self.0[..]
  }
}

impl DerefMut for CacheAlignedBuf {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.0[..]
  }
}
