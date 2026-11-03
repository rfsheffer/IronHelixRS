use thiserror::Error;

#[derive(Debug, Error)]
pub enum PacoError {
    #[error("unexpected end of data at offset {offset} (wanted {wanted} more bytes)")]
    UnexpectedEof { offset: usize, wanted: usize },

    #[error("invalid data: {0}")]
    Invalid(String),

    #[error("RLE error at byte {offset}: {message}")]
    Rle { offset: usize, message: String },
}

pub type Result<T> = std::result::Result<T, PacoError>;
