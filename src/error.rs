use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EpubError {
    #[error("no such file: {0}")]
    NotFound(PathBuf),
    #[error("not a valid EPUB: {reason}")]
    Malformed { reason: String },
    #[error("this EPUB has no readable chapters")]
    NoChapters,
    #[error("EPUB I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum PersistenceError {
    #[error("could not resolve data directory")]
    NoDataDir,
    #[error("persistence I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("persistence serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn epub_error_displays_path() {
        let err = EpubError::NotFound(PathBuf::from("/no/such/file.epub"));
        assert_eq!(err.to_string(), "no such file: /no/such/file.epub");
    }

    #[test]
    fn epub_error_malformed_includes_reason() {
        let err = EpubError::Malformed { reason: "missing OPF".into() };
        assert_eq!(err.to_string(), "not a valid EPUB: missing OPF");
    }

    #[test]
    fn persistence_error_no_data_dir_message() {
        let err = PersistenceError::NoDataDir;
        assert_eq!(err.to_string(), "could not resolve data directory");
    }
}
