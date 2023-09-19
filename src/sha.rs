use crate::{fs, paths};
use std::{fmt, io};

pub fn try_hash(file: &paths::FilePath) -> Result<Digests, fs::Error> {
  fs::File::open(&file)
    .and_then(|mut file| Digests::try_from(&mut file))
    .map_err(|err| fs::Error::ReadError(err, file.into()))
}

#[derive(Clone, Debug)]
pub struct Digests {
  sha1: String,
  sha256: String,
}

impl Digests {
  pub fn sha1(&self) -> &str {
    &self.sha1
  }

  pub fn sha256(&self) -> &str {
    &self.sha256
  }
}

impl TryFrom<&mut fs::File> for Digests {
  type Error = io::Error;

  fn try_from(file: &mut fs::File) -> Result<Self, Self::Error> {
    use digest::Digest;
    use io::Read;

    let mut buf = [0u8; 65536];
    let mut sha1 = sha1::Sha1::new();
    let mut sha256 = sha2::Sha256::new();
    loop {
      let bytes = file.read(&mut buf)?;
      if bytes == 0 {
        return Ok(Digests {
          sha1: format!("{:X}", sha1.finalize()),
          sha256: format!("{:X}", sha256.finalize()),
        });
      }
      let slice = &buf[..bytes];
      sha1.update(slice);
      sha256.update(slice);
    }
  }
}

impl fmt::Display for Digests {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "SHA-1: {}, SHA-256: {}", self.sha1(), self.sha256())
  }
}
