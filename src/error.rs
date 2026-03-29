use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("audio error: {0}")]
    Audio(String),

    #[error("metadata error: {path}: {message}")]
    Metadata { path: String, message: String },

    #[error("scan error: {0}")]
    Scan(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}
