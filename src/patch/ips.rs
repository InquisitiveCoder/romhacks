use crate::io::prelude::*;
use crate::{io, mem, patch};
use std::num;

pub fn patch(
  rom: &mut (impl Write + Seek + Resize),
  ips: &mut (impl Read + Seek),
  patch_eof: u64,
) -> Result<(), patch::Error> {
  const FOOTER_LEN: usize = 6;
  ips.seek(io::SeekFrom::End(-(FOOTER_LEN as i64)))?;
  let (end_of_patch, new_size) = match (&ips.read_array::<FOOTER_LEN>()?).split_at(3) {
    (_, b"EOF") => (patch_eof - 3, None),
    (b"EOF", new_size) => {
      let buf = mem::init([0u8; 4], |buf| {
        (&mut buf[1..]).copy_from_slice(new_size);
      });
      let new_size: u32 = u32::from_be_bytes(buf);
      let new_size = num::NonZeroU32::new(new_size).ok_or(patch::Error::BadPatch)?;
      (patch_eof - 6, Some(new_size))
    }
    _ => return Err(patch::Error::BadPatch),
  };

  ips.seek(io::SeekFrom::Start(0))?;
  let mut ips = io::BufReader::new(ips).take(end_of_patch);
  if &ips.read_array::<5>()? != b"PATCH" {
    return Err(patch::Error::BadPatch);
  }

  loop {
    let offset: u32 = ips.read_u24::<BE>()?;
    rom.seek(io::SeekFrom::Start(offset.into()))?;
    match num::NonZeroU16::new(ips.read_u16::<BE>()?) {
      Some(hunk_size) => {
        let mut hunk = (&mut ips).take(hunk_size.get().into());
        io::copy(&mut hunk, rom)?;
      }
      None => {
        let size = num::NonZeroU16::new(ips.read_u16::<BE>()?).ok_or(patch::Error::BadPatch)?;
        let value: u8 = ips.read_u8()?;
        io::copy(&mut io::repeat(value).take(size.get().into()), rom)?;
      }
    }
    if ips.limit() == 0 {
      break;
    }
  }

  if let Some(new_size) = new_size {
    rom.set_len(new_size.get().into())?;
  }

  rom.flush()?;
  Ok(())
}
