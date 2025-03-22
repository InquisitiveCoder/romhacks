use crate::io;
use crate::io::prelude::*;
use std::ops::DerefMut;
use std::sync;

const BUF_SIZE: usize = 8 * 1024;

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
    let rw_lock = sync::Arc::new(sync::RwLock::new(io::Cursor::new([0u8; BUF_SIZE])));
    let barrier = sync::Arc::new(sync::Barrier::new(2));

    // This thread will wait on the barrier immediately.
    let crc32 = spawn_crc32_thread(&rw_lock, &barrier);

    loop {
      let eof: bool = {
        // Acquiring the lock fails iff a writer panicked while holding it.
        // Since this thread is the only writer, acquiring the lock can't fail.
        let mut write_lock: sync::RwLockWriteGuard<_> = rw_lock.write().unwrap();
        let buffer: &mut io::Cursor<[u8; BUF_SIZE]> = write_lock.deref_mut();
        let bytes_copied = reader.read(&mut buffer.get_mut()[..])?;
        buffer.set_position(bytes_copied as u64);
        bytes_copied == 0
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
  lock: &sync::Arc<sync::RwLock<io::Cursor<[u8; BUF_SIZE]>>>,
  barrier: &sync::Arc<sync::Barrier>,
) -> std::thread::JoinHandle<u32> {
  let lock = sync::Arc::clone(lock);
  let barrier = sync::Arc::clone(barrier);
  std::thread::spawn(move || {
    let mut hasher = crc32fast::Hasher::new();
    loop {
      // The parent thread is holding the write lock.
      barrier.wait();
      let buffer: io::Cursor<[u8; BUF_SIZE]> = {
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
      if buffer.position() == 0 {
        return hasher.finalize();
      } else {
        hasher.update(&buffer.get_ref()[..buffer.position() as usize]);
      }
    }
  })
}
