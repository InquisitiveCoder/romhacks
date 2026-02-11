use crate::prelude::*;
use std::hash::Hasher;
use std::io;
use std::io::prelude::*;

mod reader;
pub use reader::*;

mod writer;
pub use writer::*;

mod contiguous;
pub use contiguous::*;
