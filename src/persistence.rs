use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const REGISTRY_VERSION: u32 = 1;

fn default_version() -> u32 {
    REGISTRY_VERSION
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Position {
    pub title: String,
    pub author: String,
    pub chapter_idx: u32,
    pub line_offset: u32,
    pub last_read: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Registry {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub books: BTreeMap<String, Position>,
}

impl Default for Registry {
    fn default() -> Self {
        Self { version: REGISTRY_VERSION, books: BTreeMap::new() }
    }
}

use std::path::Path;

pub fn load_from(path: &Path) -> Registry {
    match std::fs::read_to_string(path) {
        Ok(s) => match serde_json::from_str::<Registry>(&s) {
            Ok(reg) if reg.version == REGISTRY_VERSION => reg,
            Ok(_) => {
                eprintln!(
                    "cleader: registry has unknown version, starting fresh"
                );
                Registry::default()
            }
            Err(e) => {
                eprintln!(
                    "cleader: registry is corrupt ({e}), starting fresh"
                );
                Registry::default()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Registry::default()
        }
        Err(e) => {
            eprintln!("cleader: could not read registry ({e}), starting fresh");
            Registry::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn registry_roundtrips_through_json() {
        let mut reg = Registry::default();
        reg.books.insert(
            "/path/to/book.epub".into(),
            Position {
                title: "Firefly".into(),
                author: "Tim Lebbon".into(),
                chapter_idx: 4,
                line_offset: 312,
                last_read: DateTime::parse_from_rfc3339("2026-05-03T11:53:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            },
        );
        let json = serde_json::to_string(&reg).unwrap();
        let parsed: Registry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, reg);
    }

    #[test]
    fn registry_default_is_version_1_empty() {
        let reg = Registry::default();
        assert_eq!(reg.version, 1);
        assert!(reg.books.is_empty());
    }

    #[test]
    fn registry_parses_with_missing_optional_fields() {
        // Empty object — both fields use serde defaults
        let reg: Registry = serde_json::from_str("{}").unwrap();
        assert_eq!(reg.version, 1);
        assert!(reg.books.is_empty());

        // Only version
        let reg: Registry = serde_json::from_str(r#"{"version": 1}"#).unwrap();
        assert!(reg.books.is_empty());

        // Only books
        let reg: Registry =
            serde_json::from_str(r#"{"books": {}}"#).unwrap();
        assert_eq!(reg.version, 1);
    }

    #[test]
    fn load_from_missing_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nope.json");
        let reg = load_from(&path);
        assert_eq!(reg, Registry::default());
    }

    #[test]
    fn load_from_corrupt_json_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"this is not json").unwrap();
        let reg = load_from(&path);
        assert_eq!(reg, Registry::default());
    }

    #[test]
    fn load_from_unknown_version_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("future.json");
        std::fs::write(&path, r#"{"version":999,"books":{}}"#).unwrap();
        let reg = load_from(&path);
        assert_eq!(reg, Registry::default());
    }

    #[test]
    fn load_from_valid_file_returns_registry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ok.json");
        std::fs::write(&path, r#"{"version":1,"books":{}}"#).unwrap();
        let reg = load_from(&path);
        assert_eq!(reg.version, 1);
        assert!(reg.books.is_empty());
    }
}
