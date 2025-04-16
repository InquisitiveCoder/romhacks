use crate::crc::HasCrc32;
use crate::io_utils::prelude::*;
use crate::patch::Error::BadPatch;
use crate::patch::byuu::varint::ReadNumber;
use crate::patch::byuu::{FOOTER_LEN, validate_checksums};
use crate::patch::{Error, HasInternalCrc32};
use crate::{io_utils, patch};
use std::io;
use std::io::prelude::*;

pub const MAGIC: &[u8] = b"BPS";

pub fn patch(
  rom: &mut (impl Read + Seek + HasCrc32),
  patch: &mut (impl Read + Seek + HasInternalCrc32),
  output: &mut (impl Read + Write + Seek),
) -> Result<(), patch::Error> {
  let mut patch = io::BufReader::new(patch);

  let start_of_checksums = patch.seek(io::SeekFrom::End(-(FOOTER_LEN as i64)))?;
  validate_checksums(&mut patch, rom.crc32())?;

  patch.seek(io::SeekFrom::Start(0))?;
  let mut header = (&mut patch).track_position_from_start();
  if &header.read_array::<4>()? != b"BPS1" {
    return Err(BadPatch);
  }
  let _source_size: u64 = header.read_number()?;
  let _target_size: u64 = header.read_number()?;
  let metadata_size: i64 = header.read_number()?.try_into().map_err(|_| BadPatch)?;
  header.seek_relative(metadata_size)?;
  let start_of_commands: u64 = header.position();

  let mut rom = rom.track_position_from_start().buffer_reads();
  let mut commands = patch.take(start_of_checksums - start_of_commands);
  let mut output = output.track_position_from_start().buffer_writes();
  let mut source_relative_offset: u64 = 0;
  let mut target_relative_offset: u64 = 0;
  let mut target_copy_buffer: Vec<u8> = Vec::new();
  loop {
    match commands.decode_command()? {
      Command::SourceRead { length } => {
        eprintln!("source read");
        rom.seek_relative((output.position() - rom.position()) as i64)?;
        io_utils::copy_exactly(length, &mut rom, &mut output)?;
      }
      Command::TargetRead { length } => {
        eprintln!("target read");
        io_utils::copy_exactly(length, &mut commands, &mut output)?;
      }
      Command::SourceCopy { length, offset } => {
        eprintln!("source copy, length: {length}, offset: {offset}");
        source_relative_offset = source_relative_offset
          .checked_add_signed(offset)
          .ok_or(BadPatch)?;
        rom.seek_relative((source_relative_offset - rom.position()) as i64)?;
        io_utils::copy_exactly(length, &mut rom, &mut output)?;
        source_relative_offset = source_relative_offset.checked_add(length).ok_or(BadPatch)?;
      }
      Command::TargetCopy { length, offset } => {
        eprintln!("target copy");
        target_relative_offset = target_relative_offset
          .checked_add_signed(offset)
          .ok_or(BadPatch)?;
        let output_offset = output.position();
        let sequence_len = output_offset
          .checked_sub(target_relative_offset)
          .map(|offset_diff| u64::min(offset_diff, length))
          .ok_or(BadPatch)?;
        output = {
          let mut output_file = output.unbuffered()?;
          target_copy_buffer.reserve(sequence_len as usize);
          output_file.seek(io::SeekFrom::Start(target_relative_offset))?;
          io_utils::copy_exactly(sequence_len, &mut output_file, &mut target_copy_buffer)?;
          if output_file.position() < output_offset {
            output_file.seek(io::SeekFrom::Start(output_offset))?;
          }
          io_utils::copy_exactly(
            length,
            &mut io_utils::RepeatSlice::new(&target_copy_buffer[..]),
            &mut output_file,
          )?;
          target_copy_buffer.clear();
          output_file.buffer_writes()
        };
        target_relative_offset = target_relative_offset.checked_add(length).ok_or(BadPatch)?;
      }
    }
    if commands.limit() == 0 {
      break;
    }
  }

  Ok(())
}

trait ReadBPS: Read + ReadNumber {
  fn decode_command(&mut self) -> Result<Command, Error> {
    let encoded = self.read_number()?;
    let length: u64 = (encoded >> 2) + 1;
    Ok(match encoded & 3 {
      0 => Command::SourceRead { length },
      1 => Command::TargetRead { length },
      2 => Command::SourceCopy { length, offset: self.decode_signed_number()? },
      _ => Command::TargetCopy { length, offset: self.decode_signed_number()? },
    })
  }

  fn decode_signed_number(&mut self) -> io::Result<i64> {
    let encoded = self.read_number()?;
    Ok(((encoded >> 1) as i64) * (if encoded & 1 == 1 { -1 } else { 1 }))
  }
}

impl<R: Read> ReadBPS for R {}

enum Command {
  SourceRead { length: u64 },
  TargetRead { length: u64 },
  SourceCopy { length: u64, offset: i64 },
  TargetCopy { length: u64, offset: i64 },
}
