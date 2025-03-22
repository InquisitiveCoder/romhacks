use crate::io::prelude::*;
use crate::{io, mem, patch};
use std::num;

pub const MAGIC: &[u8] = b"PAT";

pub fn patch(
  rom: &mut (impl Write + Seek + Resize),
  patch: &mut (impl Read + Seek),
) -> Result<(), patch::Error> {
  const FOOTER_LEN: usize = 6;
  let patch_eof = patch.seek(io::SeekFrom::End(-(FOOTER_LEN as i64)))? + FOOTER_LEN as u64;
  let (end_of_records, new_file_size) = match (&patch.read_array::<FOOTER_LEN>()?).split_at(3) {
    (_, b"EOF") => (patch_eof - 3, None),
    (b"EOF", new_size) => {
      let buf = mem::init([0u8; 4], |buf| {
        (&mut buf[1..]).copy_from_slice(new_size);
      });
      let new_file_size: u32 = u32::from_be_bytes(buf);
      let new_size = num::NonZeroU32::new(new_file_size).ok_or(patch::Error::BadPatch)?;
      (patch_eof - 6, Some(new_size))
    }
    _ => return Err(patch::Error::BadPatch),
  };

  patch.seek(io::SeekFrom::Start(0))?;
  let mut patch = io::BufReader::new(patch).take(end_of_records);
  if &patch.read_array::<5>()? != MAGIC {
    return Err(patch::Error::BadPatch);
  }

  loop {
    let offset: u32 = patch.read_u24::<BE>()?;
    rom.seek(io::SeekFrom::Start(offset.into()))?;
    match num::NonZeroU16::new(patch.read_u16::<BE>()?) {
      Some(hunk_size) => {
        let mut hunk = (&mut patch).take(hunk_size.get().into());
        io::copy(&mut hunk, rom)?;
      }
      None => {
        let size = num::NonZeroU16::new(patch.read_u16::<BE>()?).ok_or(patch::Error::BadPatch)?;
        let value: u8 = patch.read_u8()?;
        io::copy(&mut io::repeat(value).take(size.get().into()), rom)?;
      }
    }
    if patch.limit() == 0 {
      break;
    }
  }

  if let Some(new_size) = new_file_size {
    rom.set_len(new_size.get().into())?;
  }

  rom.flush()?;
  Ok(())
}
