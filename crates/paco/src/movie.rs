//! A parsed (but not yet decoded) PACo movie.

use crate::error::{PacoError, Result};
use crate::header::{CAVF_HEADER_SIZE, CavfHeader, ChunkHeader};
use crate::ops::Op;
use crate::reader::Reader;

#[derive(Debug, Clone)]
pub struct Chunk {
    pub header: ChunkHeader,
    /// Byte offset of the chunk's data within the file.
    pub offset: usize,
    pub ops: Vec<Op>,
}

#[derive(Debug, Clone)]
pub struct Movie {
    pub header: CavfHeader,
    pub chunks: Vec<Chunk>,
}

impl Movie {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let header = CavfHeader::parse(&mut r)?;

        if header.header_size as usize != CAVF_HEADER_SIZE {
            return Err(PacoError::Invalid(format!(
                "unexpected header size {} (expected {CAVF_HEADER_SIZE}); not a CAVF file?",
                header.header_size
            )));
        }
        if header.width <= 0 || header.height <= 0 || header.num_chunks < 0 {
            return Err(PacoError::Invalid("bad dimensions or chunk count".into()));
        }

        let chunk_headers = (0..header.num_chunks)
            .map(|_| r.u32().map(ChunkHeader))
            .collect::<Result<Vec<_>>>()?;

        let mut chunks = Vec::with_capacity(chunk_headers.len());
        for chunk_header in chunk_headers {
            let offset = r.position();
            let mut cr = Reader::new(r.bytes(chunk_header.len())?);
            let mut ops = Vec::new();
            while !cr.is_empty() {
                let op = Op::parse(&mut cr)?;
                let end = matches!(op, Op::EndOfChunk);
                ops.push(op);
                if end {
                    break;
                }
            }
            chunks.push(Chunk {
                header: chunk_header,
                offset,
                ops,
            });
        }

        Ok(Self { header, chunks })
    }

    pub fn width(&self) -> usize {
        self.header.width as usize
    }

    pub fn height(&self) -> usize {
        self.header.height as usize
    }

    /// All opcodes in playback order.
    pub fn ops(&self) -> impl Iterator<Item = &Op> {
        self.chunks.iter().flat_map(|c| c.ops.iter())
    }

    /// The movie's sound track: every SOUND opcode's samples joined in order.
    /// Samples are unsigned 8-bit mono PCM. Returns `None` for silent movies.
    pub fn audio(&self) -> Option<Audio> {
        let mut audio: Option<Audio> = None;
        for op in self.ops() {
            if let Op::Sound(s) = op {
                let a = audio.get_or_insert_with(|| Audio {
                    sample_rate: s.sample_rate_hz(),
                    samples: Vec::new(),
                });
                a.samples.extend_from_slice(&s.data);
            }
        }
        audio.filter(|a| !a.samples.is_empty())
    }
}

#[derive(Debug, Clone)]
pub struct Audio {
    pub sample_rate: u32,
    /// Unsigned 8-bit mono PCM (0x80 is silence).
    pub samples: Vec<u8>,
}
