use crate::crc;
use crate::io::prelude::*;
use crate::patch::Error;
use crate::patch::varint::{ReadByuuVarInt, overflow_err};
use ::rayon::prelude::*;
use std::ops::{Deref, DerefMut};
use std::{io, iter};
use wide::u8x16;

pub const MAGIC: &[u8] = b"UPS";

const FOOTER_SIZE: usize = 3 * size_of::<u32>();
const BUF_SIZE: usize = 8 * 1024; // default buffer size used by std::io
const SIMD_SIZE: usize = u8x16::LANES as usize;

pub fn patch(
  rom: &mut (impl Read + Write + Seek + Resize),
  patch: &mut (impl Read + Seek),
  file_checksum: crc::Crc32,
  patch_checksum: crc::Crc32,
) -> Result<(), Error> {
  let mut patch = io::BufReader::with_capacity(BUF_SIZE, patch);

  let start_of_checksums = patch.seek(io::SeekFrom::End(-(FOOTER_SIZE as i64)))? as i64;
  validate_checksums(&mut patch, file_checksum, patch_checksum)?;

  patch.seek(io::SeekFrom::Start(0))?;
  if &patch.read_array::<4>()? != b"UPS1" {
    return Err(Error::BadPatch);
  }

  let input_rom_size: u64 = patch.read_varint()?;
  let output_rom_size: u64 = patch.read_varint()?;

  // don't make a syscall if it's not necessary
  if input_rom_size != output_rom_size {
    rom.set_len(output_rom_size)?;
  }
  rom.seek(io::SeekFrom::Start(0))?;

  let mut rom_buf = CacheAlignedBuffer([0u8; BUF_SIZE]);
  let mut hunks = patch.take(start_of_checksums as u64 - MAGIC.len() as u64);
  loop {
    let offset = i64::try_from(hunks.read_varint()?) //
      .map_err(|_| overflow_err())?;
    rom.seek_relative(offset)?;
    apply_hunk(rom, &mut hunks, &mut rom_buf)?;
    if hunks.limit() == 0 {
      break;
    }
  }

  Ok(())
}

fn validate_checksums(
  patch: &mut io::BufReader<&mut (impl Read + Seek + Sized)>,
  file_checksum: crc::Crc32,
  patch_checksum: crc::Crc32,
) -> Result<(), Error> {
  let expected_file_checksum = crc::Crc32::new(patch.read_u32::<LE>()?);
  let result_checksum = crc::Crc32::new(patch.read_u32::<LE>()?);
  let expected_patch_checksum = crc::Crc32::new(patch.read_u32::<LE>()?);

  // Check if the patch is valid before anything else.
  if patch_checksum != expected_patch_checksum {
    return Err(Error::BadPatch);
  }

  if file_checksum != expected_file_checksum {
    return Err(if file_checksum == result_checksum {
      Error::AlreadyPatched
    } else {
      Error::WrongInputFile
    });
  }

  Ok(())
}

fn apply_hunk(
  rom: &mut (impl Read + Write + Seek + Resize + Sized),
  hunks: &mut io::Take<io::BufReader<&mut (impl Read + Seek + Sized)>>,
  rom_buf: &mut CacheAlignedBuffer,
) -> Result<(), Error> {
  loop {
    let hunks_buf: &[u8] = hunks.fill_buf()?;
    if hunks_buf.is_empty() {
      return Err(Error::BadPatch);
    }
    // The memchr crate uses SIMD to find the first NUL byte efficiently.
    let (size, is_end_of_hunk) = ::memchr::memchr(0, hunks_buf)
      .map(|i| (i, true))
      .unwrap_or_else(|| (hunks_buf.len(), false));
    let (patch_hunk, rom_hunk) = (&hunks_buf[..size], &mut rom_buf[..size]);
    rom.read_exact(rom_hunk)?;
    xor_hunks(patch_hunk, rom_hunk);
    rom.seek_relative(-(rom_hunk.len() as i64))?;
    rom.write_all(rom_hunk)?;
    // Add 1 to account for the NUL byte.
    hunks.consume(size + 1);
    if is_end_of_hunk {
      break;
    }
  }
  Ok(())
}

fn xor_hunks(patch_hunk: &[u8], rom_hunk: &mut [u8]) {
  const CACHE_LINE_SIZE: usize = 64;
  (patch_hunk.par_chunks(CACHE_LINE_SIZE))
    .zip(rom_hunk.par_chunks_mut(CACHE_LINE_SIZE))
    .for_each(xor_cache_line);
}

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

#[repr(align(64))]
struct CacheAlignedBuffer([u8; BUF_SIZE]);

impl Deref for CacheAlignedBuffer {
  type Target = [u8];

  fn deref(&self) -> &Self::Target {
    &self.0[..]
  }
}

impl DerefMut for CacheAlignedBuffer {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.0[..]
  }
}
