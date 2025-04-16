use crate::io_utils::DEFAULT_BUF_SIZE;
use crate::io_utils::prelude::*;
use crate::patch::Error;
use crate::patch::byuu::varint::ReadNumber;
use crate::patch::byuu::*;
use ::rayon::prelude::*;
use aligned_vec::{AVec, CACHELINE_ALIGN, avec};
use std::cmp::Ordering;
use std::io::ErrorKind::Interrupted;
use std::io::prelude::*;
use std::ops::{Deref, DerefMut};
use std::{io, iter};
use wide::u8x16;

// Documentation: https://www.romhacking.net/documents/392/

pub const MAGIC: &[u8] = b"UPS";

const SIMD_SIZE: usize = u8x16::LANES as usize;

pub fn patch(
  rom: &mut impl BufRead,
  patch: &mut (impl BufRead + Seek),
  output: &mut impl CopyWriteBuf,
) -> Result<(), Error> {
  let mut patch = PositionTracker::from_start(patch);
  if &patch.read_array::<4>()? != b"UPS1" {
    return Err(Error::BadPatch);
  }

  let input_rom_size: u64 = patch.read_number()?;
  let output_rom_size: u64 = patch.read_number()?;

  let mut rom = PositionTracker::from_start(rom.take(input_rom_size));
  let mut output_buf = CacheAlignedBuf::new();
  loop {
    let offset = patch.read_number()?;
    (&mut rom)
      .take(offset)
      .exact(|rom| rom.copy_using_writer_buf(output))?;
    apply_hunk(&mut rom, &mut patch, output, &mut output_buf)?;
    match patch
      .look_ahead(FOOTER_LEN + 1, &mut io::sink())?
      .cmp(&FOOTER_LEN)
    {
      Ordering::Greater => continue,
      Ordering::Equal => break,
      Ordering::Less => return Err(Error::BadPatch),
    }
  }

  rom
    .map_inner(|inner| inner.chain(io::repeat(0)).buffer_reads())
    .until_position(output_rom_size)?
    .exact(|rom| rom.copy_using_writer_buf(output).map(|_| ()))
    .map_err(|_| Error::BadPatch)
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
      Ok(buf) if buf.is_empty() => return Err(Error::BadPatch),
      Ok(buf) => buf,
      Err(e) if e.kind() == Interrupted => continue,
      Err(e) => Err(e)?,
    };

    // The memchr crate uses SIMD to efficiently find the first occurrence of
    // a specified byte in a slice. We take advantage of this to find the NUL
    // byte that terminates the current hunk.
    let (size, is_end_of_hunk) = ::memchr::memchr(0, patch_buf)
      .map(|i| (i, true))
      .unwrap_or_else(|| (patch_buf.len(), false));
    let patch_buf = &patch_buf[..size];

    output_buf.resize(size);
    rom.chain(io::repeat(0)).copy_to_slice(output_buf)?;
    let (patch_hunk, rom_hunk) = (patch_buf, &mut output_buf[..]);
    xor_hunks(patch_hunk, rom_hunk);
    output.write_all(rom_hunk)?;

    // Add 1 to the amount consumed from the BufReader to account for the NUL
    // terminator.
    patch.consume(size + 1);
    if is_end_of_hunk {
      break;
    }
  }
  Ok(())
}

/// Uses rayon to XOR the `patch_hunk` and `rom_hunk` in parallel.
/// `rom_hunk` should be aligned to the cache line size to prevent false sharing.
fn xor_hunks(patch_hunk: &[u8], rom_hunk: &mut [u8]) {
  (patch_hunk.par_chunks(CACHELINE_ALIGN))
    .zip(rom_hunk.par_chunks_mut(CACHELINE_ALIGN))
    .for_each(xor_cache_line);
}

/// Uses SIMD to XOR two equal-length slices together.
fn xor_cache_line((patch_cache_line, rom_cache_line): (&[u8], &mut [u8])) {
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
