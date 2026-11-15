use std::path::PathBuf;

/// Errors produced while loading an image.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("unsupported image format")]
    Unsupported,
    #[error("image is too large ({width}×{height})")]
    TooLarge { width: u32, height: u32 },
    #[error("{format} decoder: {message}")]
    Format {
        format: &'static str,
        message: String,
    },
    #[error("decoder panicked on malformed input")]
    Panicked,
    #[error("cancelled")]
    Cancelled,
}

impl DecodeError {
    pub fn format(format: &'static str, err: impl std::fmt::Display) -> Self {
        Self::Format {
            format,
            message: err.to_string(),
        }
    }
}

pub type Result<T, E = DecodeError> = std::result::Result<T, E>;
