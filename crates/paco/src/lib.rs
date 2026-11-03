//! Decoder for PACo movies.
//!
//! PACo (the PICS Animation Compiler, by the Company of Science and Art) packs
//! animations into the CAVF format. Iron Helix stores nearly all of its
//! pre-rendered graphics this way.
//!
//! A CAVF file is laid out as:
//!
//! ```text
//! CavfHeader (38 bytes)
//! ChunkHeader[num_chunks] (4 bytes each)
//! chunk 0 data, chunk 1 data, ...
//! ```
//!
//! Each chunk is a list of opcodes (palette changes, bitmaps, sounds, delays,
//! ...). Every BITMAP opcode produces one displayed frame. All multi-byte
//! values are big-endian, even in the Windows release, because the format was
//! born on the Mac.

pub mod decoder;
pub mod error;
pub mod header;
pub mod movie;
pub mod ops;
pub mod palette;
pub mod reader;
pub mod rle;

pub use decoder::{Decoder, Frame};
pub use error::{PacoError, Result};
pub use header::{CavfHeader, ChunkHeader};
pub use movie::{Audio, Chunk, Movie};
pub use ops::Op;
pub use palette::{Palette, Rgb};
