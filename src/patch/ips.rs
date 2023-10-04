use crate::error::prelude::*;
use crate::io::prelude::*;
use crate::{io, mem};
use std::num;

pub fn patch(
  rom: &mut (impl Write + Seek + Resize),
  ips: &mut (impl Read + Seek),
) -> Result<(), Error> {
  let eof: u64 = ips.seek(io::SeekFrom::End(-6))? + 6;
  let (end_of_patch, new_size) = match (&ips.read_array::<6>()?).split_at(3) {
    (_, b"EOF") => (eof - 3, None),
    (b"EOF", new_size) => {
      let buf = mem::init([0u8; 4], |buf| {
        (&mut buf[1..]).copy_from_slice(new_size);
      });
      let new_size: u32 = u32::from_be_bytes(buf);
      let new_size = num::NonZeroU32::new(new_size).ok_or(Error::FileLength)?;
      (eof - 6, Some(new_size))
    }
    _ => return Err(Error::EOF),
  };

  ips.seek(io::SeekFrom::Start(0))?;
  let mut ips = io::BufReader::new(ips).take(end_of_patch);
  if &ips.read_array::<5>()? != b"PATCH" {
    return Err(Error::Header);
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
        let size = num::NonZeroU16::new(ips.read_u16::<BE>()?).ok_or(Error::HunkLength)?;
        let value: u8 = ips.read_u8()?;
        rom.write_repeated(value, size.get().into())?;
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

#[derive(Debug, Error)]
pub enum Error {
  #[error("IPS patch did not end with the EOF delimiter. It may be corrupt.")]
  EOF,
  #[error("New file length was 0. The IPS file may be corrupt.")]
  FileLength,
  #[error("IPS patch didn't start with the correct magic string.")]
  Header,
  #[error("Encountered a 0-length hunk. The IPS file may be corrupt.")]
  HunkLength,
  #[error(transparent)]
  IO(#[from] io::Error),
}
