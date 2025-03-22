use crate::io;
use crate::io::prelude::*;
use crate::patch::Error;
use crate::patch::vcd::cache::AddressCache;
use byteorder::ReadBytesExt;
use num_traits::{CheckedMul, Num};
use std::io::{BufReader, Read, Seek, Write};
use std::num::NonZeroU8;

/// The magic string for Vcdiff patch files.
///
/// The ASCII string "VCD" with the MSB bit of each byte set.
pub const MAGIC: &[u8] = &set_msb([b'V', b'C', b'D']);

const VCD_DECOMPRESS: u8 = 1;
const VCD_CODETABLE: u8 = 2;
const HAS_APPHEADER: u8 = 4;

pub fn patch(
  rom: &mut (impl Read + Seek),
  patch: &mut (impl Read + Seek),
  output: &mut (impl Read + Write + Seek),
) -> Result<(), Error> {
  let mut patch = BufReader::new(patch);

  // header
  {
    if &patch.read_array::<3>()? != MAGIC {
      return Err(Error::BadPatch);
    }

    let version = patch.read_u8()?;
    if version != 0 {
      return Err(Error::UnsupportedPatchFeature);
    }

    let hdr_indicator = patch.read_u8()?;
    if hdr_indicator & (VCD_CODETABLE | VCD_DECOMPRESS) != 0 {
      return Err(Error::UnsupportedPatchFeature);
    }

    if hdr_indicator & HAS_APPHEADER != 0 {
      // Skip over the app header.
      let header_size: u32 = patch.read_vcdiff_int()?;
      patch.seek_relative(header_size as i64)?;
    }
  }

  let mut patcher = Patcher::new(rom, patch, output);
  // window sections
  loop {
    patcher.process_window()?;
    if patcher.reached_eof()? {
      break;
    }
    patcher.clear_buffers();
  }

  Ok(())
}

struct Patcher<R, P, O> {
  files: Files<R, P, O>,
  buffers: Buffers,
}

impl<R, P, O> Patcher<R, P, O>
where
  R: Read + Seek,
  P: BufRead,
  O: Read + Write + Seek,
{
  pub const VCD_SOURCE: u8 = 0x01;
  pub const VCD_TARGET: u8 = 0x02;

  pub fn new(rom: R, patch: P, output: O) -> Self {
    Self {
      files: Files { rom, patch, output },
      buffers: Buffers::new(),
    }
  }

  fn process_window(&mut self) -> Result<(), Error> {
    let Files { rom, patch, output } = &mut self.files;
    let buffers = &mut self.buffers;

    let win_indicator = patch.read_u8()?;
    let source_window_len = match win_indicator {
      0 => 0,
      Self::VCD_SOURCE => {
        let source_len: u32 = patch.read_vcdiff_int()?;
        let source_position: u64 = patch.read_vcdiff_int()?;
        rom.seek(io::SeekFrom::Start(source_position))?;
        io::copy(&mut rom.take(source_len as u64), &mut buffers.superstring)?;
        source_len
      }
      Self::VCD_TARGET => {
        let source_len: u32 = patch.read_vcdiff_int()?;
        let source_position: u64 = patch.read_vcdiff_int()?;
        output.seek(io::SeekFrom::Start(source_position))?;
        io::copy(
          &mut output.take(source_len as u64),
          &mut buffers.superstring,
        )?;
        output.seek(io::SeekFrom::End(0))?;
        source_len
      }
      _ => return Err(Error::BadPatch),
    };

    let encoding_len: u32 = patch.read_vcdiff_int()?;
    let mut patch = patch.take(encoding_len as u64);

    let target_window_len: u32 = patch.read_vcdiff_int()?;
    buffers
      .superstring
      .resize(buffers.superstring.len() + target_window_len as usize, 0);

    let delta_indicator: u8 = patch.read_u8()?;
    if delta_indicator != 0 {
      // A valid patch can't reach this condition.
      // The flags in this byte indicate which of the buffers are compressed and
      // should only be set if the VC_DECOMPRESS bit was set. If VC_DECOMPRESS
      // was set, applying the patch will return UnsupportedPatchFeature while
      // processing the header.
      return Err(Error::BadPatch);
    }

    let data_len: u32 = patch.read_vcdiff_int()?;
    let instructions_len: u32 = patch.read_vcdiff_int()?;
    let addresses_len: u32 = patch.read_vcdiff_int()?;
    io::copy(
      &mut (&mut patch).take(data_len as u64),
      &mut buffers.add_and_run_data,
    )?;
    io::copy(
      &mut (&mut patch).take(instructions_len as u64),
      &mut buffers.instructions_and_sizes,
    )?;
    io::copy(
      &mut (&mut patch).take(addresses_len as u64),
      &mut buffers.copy_addresses,
    )?;

    let mut cursors = Cursors::new(buffers, source_window_len);
    loop {
      let instruction_code = cursors.instructions_and_sizes.read_u8()?;
      let (first, second) = Self::decode_instruction_pair(instruction_code);
      Self::execute_instruction(&mut cursors, first)?;
      Self::execute_instruction(&mut cursors, second)?;
      if cursors.instructions_and_sizes.reached_eof()? {
        break;
      }
    }
    output.write_all(cursors.superstring.target_window())?;

    Ok(())
  }

  fn execute_instruction(cursors: &mut Cursors<'_>, instruction: Instruction) -> Result<(), Error> {
    match instruction {
      Instruction::Noop => {}
      Instruction::Run => {
        let byte = cursors.add_and_run_data.read_u8()?;
        let size: u32 = cursors.instructions_and_sizes.read_vcdiff_int()?;
        (cursors.superstring).write_bytes(size, |_, mut dest: &mut [u8]| {
          io::copy(&mut io::repeat(byte).take(size as u64), &mut dest)
        })?;
      }
      Instruction::Add { size } => {
        let size: u32 = cursors.read_instruction_size(size)?;
        (cursors.superstring).write_bytes(size, |_, mut dest: &mut [u8]| {
          io::copy(
            &mut (&mut cursors.add_and_run_data).take(size as u64),
            &mut dest,
          )
        })?;
      }
      Instruction::Copy { size, mode } => {
        let size: u32 = cursors.read_instruction_size(size)?;
        let here: u32 = cursors.superstring.target_window_position();
        let address = cursors.copy_addresses.decode(here, mode)?;
        (cursors.superstring).write_bytes(size, |source: &[u8], mut dest: &mut [u8]| {
          let sequence_len = u32::min(address + size, source.len() as u32) as usize;
          let periodic_sequence: &[u8] = &source[address as usize..sequence_len];
          loop {
            dest.write(periodic_sequence)?;
            if dest.is_empty() {
              break;
            }
          }
          Ok(())
        })?;
      }
    }
    Ok(())
  }

  pub fn reached_eof(&mut self) -> io::Result<bool> {
    self.files.patch.reached_eof()
  }

  fn decode_instruction_pair(index: u8) -> (Instruction, Instruction) {
    use Instruction::*;
    match (index) {
      0 => (Run, Noop),
      1..=18 => (Add { size: NonZeroU8::new(index - 1) }, Noop),
      19..=162 => {
        let offset = index - 19;
        let size = NonZeroU8::new(if offset % 16 == 0 { 0 } else { 3 + offset });
        let mode = offset / 16;
        (Copy { size, mode }, Noop)
      }
      163..=234 => {
        let offset = index - 163;
        let size = NonZeroU8::new(1 + (offset / 3) % 4);
        let size2 = NonZeroU8::new(4 + offset % 3);
        let mode = offset / 12;
        (Add { size }, Copy { size: size2, mode })
      }
      235..=246 => {
        let offset = index - 235;
        let size = NonZeroU8::new(1 + offset % 4);
        let mode = offset / 4;
        (Add { size }, Copy { size: NonZeroU8::new(4), mode })
      }
      _ => {
        let offset = index - 247;
        (
          Copy { size: NonZeroU8::new(4), mode: offset },
          Add { size: NonZeroU8::new(1) },
        )
      }
    }
  }

  pub fn clear_buffers(&mut self) {
    self.buffers.clear_all();
  }
}

struct Files<R, P, O> {
  pub rom: R,
  pub patch: P,
  pub output: O,
}

// The Vcdiff standard doesn't specify maximum bounds for these buffers so it's
// not  possible to allocate them statically.
struct Buffers {
  pub superstring: Vec<u8>,
  pub add_and_run_data: Vec<u8>,
  pub instructions_and_sizes: Vec<u8>,
  pub copy_addresses: Vec<u8>,
}

impl Buffers {
  pub const fn new() -> Self {
    Self {
      superstring: vec![],
      add_and_run_data: vec![],
      instructions_and_sizes: vec![],
      copy_addresses: vec![],
    }
  }

  pub fn clear_all(&mut self) {
    self.superstring.clear();
    self.add_and_run_data.clear();
    self.instructions_and_sizes.clear();
    self.copy_addresses.clear();
  }
}

struct Cursors<'a> {
  pub superstring: WindowCursor<'a>,
  pub add_and_run_data: io::Cursor<&'a mut [u8]>,
  pub instructions_and_sizes: io::Cursor<&'a mut [u8]>,
  pub copy_addresses: AddressDecoder<io::Cursor<&'a mut [u8]>>,
}

impl<'a> Cursors<'a> {
  pub fn new(buffers: &'a mut Buffers, source_window_len: u32) -> Self {
    Self {
      superstring: WindowCursor::new(&mut buffers.superstring[..], source_window_len),
      add_and_run_data: io::Cursor::new(&mut buffers.add_and_run_data[..]),
      instructions_and_sizes: io::Cursor::new(&mut buffers.instructions_and_sizes[..]),
      copy_addresses: AddressDecoder::new(io::Cursor::new(&mut buffers.copy_addresses[..])),
    }
  }

  pub fn read_instruction_size(&mut self, encoded_size: Option<NonZeroU8>) -> io::Result<u32> {
    match encoded_size {
      Some(x) => Ok(x.get() as u32),
      None => self.instructions_and_sizes.read_vcdiff_int::<u32>(),
    }
  }
}

struct WindowCursor<'a> {
  source_len: u32,
  cursor: io::Cursor<&'a mut [u8]>,
}

impl<'a> WindowCursor<'a> {
  pub fn new(buffer: &'a mut [u8], source_len: u32) -> Self {
    let mut cursor = io::Cursor::new(buffer);
    cursor.set_position(source_len as u64);
    Self { cursor, source_len }
  }

  pub fn position(&self) -> u32 {
    self.cursor.position() as u32
  }

  /// The current position within the target window.
  pub fn target_window_position(&self) -> u32 {
    self.cursor.position() as u32 - self.source_len
  }

  pub fn target_window(&self) -> &[u8] {
    &self.cursor.get_ref()[self.source_len as usize..]
  }

  /// Write `size` bytes to the unwritten portion of the target window.
  pub fn write_bytes<T>(
    &mut self,
    size: u32,
    update_fn: impl FnOnce(&[u8], &mut [u8]) -> io::Result<T>,
  ) -> Result<T, Error> {
    let position: u32 = self.position();
    let (written, unwritten): (&[u8], &mut [u8]) =
      self.split_for_write(size).ok_or(Error::BadPatch)?;
    let result = update_fn(written, unwritten)?;
    self.cursor.set_position((position + size) as u64);
    Ok(result)
  }

  fn split_for_write(&mut self, size: u32) -> Option<(&[u8], &mut [u8])> {
    let position = self.position();
    let (written, unwritten) = self.cursor.get_mut().split_at_mut(position as usize);
    let destination: &mut [u8] = unwritten
      .get_mut(..size as usize)
      .filter(|slice| !slice.is_empty())?;
    Some((written, destination))
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Instruction {
  #[default]
  Noop,
  Run,
  Add {
    size: Option<NonZeroU8>,
  },
  Copy {
    size: Option<NonZeroU8>,
    mode: u8,
  },
}

impl Instruction {}

trait VcdiffRead: Read {
  /// Reads a big-endian varint. If the value overflows, returns an
  /// [InvalidData](std::io::ErrorKind::InvalidData) error.
  fn read_vcdiff_int<N>(&mut self) -> Result<N, io::Error>
  where
    N: Num + CheckedMul,
    u8: Into<N>,
  {
    let mut value: N = 0.into();
    loop {
      value = (value.checked_mul(&128.into())) // equivalent to `shift << 7`
        .ok_or(io::Error::from(io::ErrorKind::InvalidData))?;
      let byte = self.read_u8()?;
      value = value + (byte & 0x7F).into();
      if byte & 0x80 == 0 {
        break;
      }
    }
    Ok(value)
  }
}
impl<R> VcdiffRead for R where R: Read {}

trait ReadEof: BufRead {
  /// Returns `true` if the reader has reached EOF.
  ///
  /// Calling this method will refill the internal buffer if it was empty.
  fn reached_eof(&mut self) -> io::Result<bool> {
    // `BufRead::fill_buf` returns an empty array iff EOF has been reached.
    Ok(self.fill_buf()?.len() == 0)
  }
}
impl<R> ReadEof for R where R: BufRead {}

struct AddressDecoder<R> {
  cache: AddressCache,
  addresses: R,
}

impl<R: Read> AddressDecoder<R> {
  pub fn new(addresses: R) -> Self {
    Self { cache: AddressCache::new(), addresses }
  }

  pub fn decode(&mut self, here: u32, mode: u8) -> Result<u32, io::Error> {
    const MAX_NEAR: u8 = 2 + cache::NearCache::SIZE;
    const MAX_HERE: u8 = MAX_NEAR + cache::SameCache::NUM_BUCKETS;
    let address: u32 = match mode {
      0 => self.addresses.read_vcdiff_int()?,
      1 => here
        .checked_sub(self.addresses.read_vcdiff_int()?)
        .ok_or(io::Error::from(io::ErrorKind::InvalidData))?,
      2..MAX_NEAR => self.cache.near()[mode - 2] + self.addresses.read_vcdiff_int::<u32>()?,
      MAX_NEAR..MAX_HERE => {
        let index: u16 = (mode - MAX_NEAR) as u16 * 256 + self.addresses.read_u8()? as u16;
        self.cache.same()[index]
      }
      _ => return Err(io::Error::from(io::ErrorKind::InvalidData)),
    };
    self.cache.update(address);
    Ok(address)
  }
}

mod cache {
  use std::ops::Index;

  pub struct AddressCache {
    near: NearCache,
    same: SameCache,
  }

  impl AddressCache {
    pub const fn new() -> Self {
      Self { near: NearCache::new(), same: SameCache::new() }
    }

    pub fn update(&mut self, addr: u32) {
      self.near.update(addr);
      self.same.update(addr);
    }

    pub const fn near(&self) -> &NearCache {
      &self.near
    }

    pub const fn same(&self) -> &SameCache {
      &self.same
    }
  }

  pub(crate) struct NearCache {
    buf: [u32; NearCache::SIZE as usize],
    next_slot: u8,
  }

  impl NearCache {
    pub const SIZE: u8 = 4;

    pub const fn new() -> Self {
      Self { buf: [0; NearCache::SIZE as usize], next_slot: 0 }
    }

    pub fn update(&mut self, addr: u32) {
      self.buf[self.next_slot as usize] = addr;
      self.next_slot = (self.next_slot + 1) % Self::SIZE;
    }
  }

  impl Index<u8> for NearCache {
    type Output = u32;

    fn index(&self, index: u8) -> &Self::Output {
      &self.buf[index as usize]
    }
  }

  pub struct SameCache([u32; 3 * 256]);

  impl SameCache {
    pub const NUM_BUCKETS: u8 = 3;
    pub const SIZE: usize = SameCache::NUM_BUCKETS as usize * 256;

    const fn new() -> Self {
      Self([0; SameCache::SIZE])
    }

    pub fn update(&mut self, addr: u32) {
      self.0[addr as usize % Self::SIZE] = addr;
    }
  }

  impl Index<u16> for SameCache {
    type Output = u32;

    fn index(&self, index: u16) -> &Self::Output {
      &self.0[index as usize]
    }
  }
}

const fn set_msb<const N: usize>(arr: [u8; N]) -> [u8; N] {
  let mut result: [u8; N] = [0; N];
  let mut i = 0;
  while i < N {
    result[i] = arr[i] & 0x80;
    i += 1;
  }
  result
}
