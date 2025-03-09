use crate::io::prelude::*;
use crate::{io, path};
use buf::ReadBuffer;
use fs_err as fs;
use std::sync::{Arc, Barrier, RwLock};

type Buffer = buf::Buffer<{ 8 * 1024 }>;

pub fn try_hash(file: &impl AsRef<path::Path>) -> io::Result<Crc32> {
  try_hash_path(file.as_ref())
}

fn try_hash_path(file: &path::Path) -> io::Result<Crc32> {
  let mut file = fs::File::open(file)?;
  Crc32::read_and_hash(&mut file)
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct Crc32(u32);

impl Crc32 {
  pub fn new(value: u32) -> Self {
    Self(value)
  }

  pub fn value(&self) -> u32 {
    self.0
  }

  pub fn read_and_hash<R: Read>(reader: &mut R) -> io::Result<Self> {
    // The crc32 is computed in parallel.
    // The current thread updates a shared buffer which the crc32 thread reads.
    // The barrier is used to coordinate the handoff of read and write locks.
    let rw_lock = Arc::new(RwLock::new(Buffer::new()));
    let barrier = Arc::new(Barrier::new(2));

    // This thread will wait on the barrier immediately.
    let crc32 = spawn_crc32_thread(&rw_lock, &barrier);

    loop {
      let eof: bool = {
        // Acquiring the lock fails iff a writer panicked while holding it.
        // Since this thread is the only writer, acquiring the lock can't fail.
        let mut write_lock = rw_lock.write().unwrap();
        let buffer: &mut Buffer = &mut (*write_lock);
        reader.read_into(buffer)?;
        buffer.len() == 0
      };
      barrier.wait();
      // The crc32 thread is now holding a read lock to the buffer.
      barrier.wait();
      // The crc32 thread has released its read lock and has either
      // returned if EOF was reached, or is updating the digest.
      if eof {
        break;
      }
    }
    Ok(Self(crc32.join().unwrap()))
  }
}

fn spawn_crc32_thread(
  lock: &Arc<RwLock<Buffer>>,
  barrier: &Arc<Barrier>,
) -> std::thread::JoinHandle<u32> {
  let lock = Arc::clone(lock);
  let barrier = Arc::clone(barrier);
  std::thread::spawn(move || {
    let mut hasher = crc32fast::Hasher::new();
    loop {
      // The parent thread is holding the write lock.
      barrier.wait();
      let buffer: Buffer = {
        // Acquiring the lock fails iff a writer panicked while holding it.
        // The parent thread doesn't do anything that could panic while holding
        // the write lock, so acquiring the lock can't fail.
        let read_lock = lock.read().unwrap();
        // Only copy the bytes for now, so the lock can be released quickly.
        (*read_lock).clone()
      };
      barrier.wait();
      // Update the hash while the parent thread is busy copying more bytes into
      // the buffer.
      if buffer.len() == 0 {
        return hasher.finalize();
      } else {
        hasher.update(&buffer);
      }
    }
  })
}

mod buf {
  use super::*;
  use std::ops::Deref;

  #[derive(Clone, Debug)]
  pub struct Buffer<const N: usize> {
    array: [u8; N],
    len: usize,
  }

  pub trait ReadBuffer: Read {
    fn read_into<const N: usize>(&mut self, buf: &mut Buffer<N>) -> io::Result<()> {
      buf.len = self.read(&mut buf.array[..])?;
      Ok(())
    }
  }

  impl<R: Read> ReadBuffer for R {}

  impl<const N: usize> Buffer<N> {
    pub const fn new() -> Self {
      Self { array: [0u8; N], len: 0 }
    }
  }

  impl<const N: usize> Deref for Buffer<N> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
      self.as_ref()
    }
  }

  impl<const N: usize> AsRef<[u8]> for Buffer<N> {
    fn as_ref(&self) -> &[u8] {
      &self.array[..self.len]
    }
  }
}
