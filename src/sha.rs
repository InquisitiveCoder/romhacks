use crate::io::prelude::*;
use crate::{fs, io, path};
use buf::Buffer;
use std::sync::{Arc, Barrier, RwLock};
use std::thread::JoinHandle;
use std::{fmt, thread};

pub fn try_hash(file: &path::FilePath) -> Result<Digests, fs::Error> {
  fs::File::open(&file)
    .and_then(|mut file| Digests::from_reader(&mut file))
    .map_err(|err| fs::Error::file(err, file))
}

#[derive(Clone, Debug)]
pub struct Digests {
  sha1: String,
  sha256: String,
  crc32: String,
}

impl Digests {
  pub fn sha1(&self) -> &str {
    &self.sha1
  }

  pub fn sha256(&self) -> &str {
    &self.sha256
  }

  pub fn crc32(&self) -> &str {
    &self.crc32
  }
  pub fn from_reader<R: Read>(reader: &mut R) -> io::Result<Self> {
    // The file hashes are computed in parallel.
    // The current thread updates a shared buffer which the hashing threads read.
    // The barrier is used to coordinate the handoff of read and write locks.
    let rw_lock = Arc::new(RwLock::new(Buffer::new()));
    let barrier = Arc::new(Barrier::new(4));

    // These threads will wait on the barrier immediately.
    let sha1 = spawn_thread::<sha1::Sha1>(&rw_lock, &barrier);
    let sha256 = spawn_thread::<sha2::Sha256>(&rw_lock, &barrier);
    let crc32 = spawn_thread::<crc32fast::Hasher>(&rw_lock, &barrier);

    loop {
      let eof: bool = {
        // Acquiring the lock fails iff a writer panicked while holding it.
        // Since this thread is the only writer, acquiring the lock can't fail.
        let mut write_lock = rw_lock.write().unwrap();
        let buffer: &mut Buffer = &mut (*write_lock);
        let len = buffer.update(reader)?;
        len == 0
      };
      barrier.wait();
      // The hashing threads are now holding read locks to the buffer.
      barrier.wait();
      // The hashing threads have released their read locks and have either
      // returned if EOF was reached, or are hashing the bytes.
      if eof {
        break;
      }
    }
    Ok(Digests {
      sha1: sha1.join().unwrap(),
      sha256: sha256.join().unwrap(),
      crc32: crc32.join().unwrap(),
    })
  }
}

fn spawn_thread<D: SimpleDigest>(
  lock: &Arc<RwLock<Buffer>>,
  barrier: &Arc<Barrier>,
) -> JoinHandle<String> {
  let lock = Arc::clone(lock);
  let barrier = Arc::clone(barrier);
  thread::spawn(move || {
    let mut hasher = D::new();
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
        hasher.update(buffer);
      }
    }
  })
}

/// Private, stripped-down version of the [Digest](sha2::Digest) trait.
/// Unfortunately [crc32fast::Hasher] doesn't implement Digest and the crc32_digest
/// crate depends on a version of digest that's incompatible with the sha1 and
/// sha2 crates, so this is the only way to get all 3 hashing algorithms to
/// implement the same trait.
trait SimpleDigest {
  fn new() -> Self;
  fn update(&mut self, data: impl AsRef<[u8]>);
  fn finalize(self) -> String;
}

impl SimpleDigest for sha1::Sha1 {
  fn new() -> Self {
    <Self as sha1::Digest>::new()
  }

  fn update(&mut self, data: impl AsRef<[u8]>) {
    <Self as sha1::Digest>::update(self, data)
  }

  fn finalize(self) -> String {
    const_hex::encode(<Self as sha1::Digest>::finalize(self))
  }
}

impl SimpleDigest for sha2::Sha256 {
  fn new() -> Self {
    <Self as sha2::Digest>::new()
  }

  fn update(&mut self, data: impl AsRef<[u8]>) {
    <Self as sha2::Digest>::update(self, data)
  }

  fn finalize(self) -> String {
    const_hex::encode(<Self as sha2::Digest>::finalize(self))
  }
}

impl SimpleDigest for crc32fast::Hasher {
  fn new() -> Self {
    Self::new()
  }

  fn update(&mut self, data: impl AsRef<[u8]>) {
    Self::update(self, data.as_ref())
  }

  fn finalize(self) -> String {
    const_hex::encode(Self::finalize(self).to_be_bytes())
  }
}

impl fmt::Display for Digests {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(
      f,
      "CRC32: {}, SHA-1: {}, SHA-256: {}",
      self.crc32(),
      self.sha1(),
      self.sha256()
    )
  }
}

mod buf {
  use super::*;
  use std::ops::Deref;

  #[derive(Clone, Debug)]
  pub struct Buffer {
    bytes: [u8; Buffer::SIZE],
    len: u16,
  }

  impl Buffer {
    pub const SIZE: usize = 8 * 1024;

    pub const fn new() -> Self {
      Self { bytes: [0u8; Self::SIZE], len: 0 }
    }

    /// Calls [Read::read] with this buffer; the bytes can then be accessed by
    /// borrowing or dereferencing the buffer as a slice. Calling `update` again
    /// will overwrite the bytes from previous calls.
    pub fn update(&mut self, reader: &mut impl Read) -> io::Result<usize> {
      self.len = reader.read(&mut self.bytes)? as u16;
      Ok(self.len as usize)
    }
  }

  impl Deref for Buffer {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
      self.as_ref()
    }
  }

  impl AsRef<[u8]> for Buffer {
    fn as_ref(&self) -> &[u8] {
      &self.bytes[..self.len as usize]
    }
  }
}
