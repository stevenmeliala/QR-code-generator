pub mod color;
pub mod qr;
#[cfg(feature = "cli")]
pub mod server;
#[cfg(feature = "wasm")]
pub mod wasm;

use std::fmt;

/// Unified error type for QR generation, image decoding, color parsing, and HTTP serving.
#[derive(Debug)]
pub enum QrError {
    InvalidInput(String),
    InvalidColor(String),
    EncodeError(String),
    ImageError(String),
    IoError(String),
}

impl fmt::Display for QrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QrError::InvalidInput(msg) => write!(f, "invalid input: {msg}"),
            QrError::InvalidColor(msg) => write!(f, "invalid color: {msg}"),
            QrError::EncodeError(msg) => write!(f, "QR encode error: {msg}"),
            QrError::ImageError(msg) => write!(f, "image error: {msg}"),
            QrError::IoError(msg) => write!(f, "I/O error: {msg}"),
        }
    }
}

impl std::error::Error for QrError {}

impl From<std::io::Error> for QrError {
    fn from(err: std::io::Error) -> Self {
        QrError::IoError(err.to_string())
    }
}

impl From<image::ImageError> for QrError {
    fn from(err: image::ImageError) -> Self {
        QrError::ImageError(err.to_string())
    }
}
