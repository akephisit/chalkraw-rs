use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("file not found: {0}")]
    NotFound(PathBuf),

    #[error("unsupported format for {0}")]
    UnsupportedFormat(PathBuf),

    #[error("decode failed for {path}: {source}")]
    DecodeFailed {
        path: PathBuf,
        #[source]
        source: image::ImageError,
    },

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
