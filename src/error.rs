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
    #[error("failed to read EPUB: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum PersistenceError {
    #[error("data directory not found")]
    NoDataDir,
    #[error("failed to read state file: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse state file: {0}")]
    Serde(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(err.to_string(), "data directory not found");
    }

    #[test]
    fn epub_error_no_chapters_message() {
        let err = EpubError::NoChapters;
        assert_eq!(err.to_string(), "this EPUB has no readable chapters");
    }

    #[test]
    fn epub_error_io_message() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let err: EpubError = io_err.into();
        assert!(err.to_string().starts_with("failed to read EPUB:"));
    }
}
