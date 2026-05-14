use crate::error::PersistenceError;
use chrono::{DateTime, Utc};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

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

pub fn save_to(path: &Path, registry: &Registry) -> Result<(), PersistenceError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_extension("json.tmp");
    {
        // Scope: drop the file handle before rename (required on Windows).
        let mut tmp = std::fs::File::create(&tmp_path)?;
        let bytes = serde_json::to_vec_pretty(registry)?;
        tmp.write_all(&bytes)?;
        tmp.sync_all()?;
    }
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

pub struct Persistence {
    path: PathBuf,
    registry: Registry,
}

impl Persistence {
    /// Resolve the platform-correct registry path and load existing state.
    pub fn open() -> Result<Self, PersistenceError> {
        let path = default_registry_path()?;
        let registry = load_from(&path);
        Ok(Self { path, registry })
    }

    /// Open against an explicit path. Intended for tests and internal tooling
    /// only — production code should use `open()`.
    #[doc(hidden)]
    pub fn open_at(path: PathBuf) -> Self {
        let registry = load_from(&path);
        Self { path, registry }
    }

    pub fn get(&self, key: &str) -> Option<&Position> {
        self.registry.books.get(key)
    }

    pub fn upsert(&mut self, key: String, pos: Position) {
        self.registry.books.insert(key, pos);
    }

    pub fn flush(&mut self) -> Result<(), PersistenceError> {
        save_to(&self.path, &self.registry)
    }
}

fn default_registry_path() -> Result<PathBuf, PersistenceError> {
    let dirs = ProjectDirs::from("", "", "cleader")
        .ok_or(PersistenceError::NoDataDir)?;
    Ok(dirs.data_dir().join("registry.json"))
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
                title: "Frankenstein".into(),
                author: "Mary Shelley".into(),
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

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.json");
        let mut reg = Registry::default();
        reg.books.insert(
            "/book.epub".into(),
            Position {
                title: "X".into(),
                author: "Y".into(),
                chapter_idx: 2,
                line_offset: 7,
                last_read: Utc::now(),
            },
        );
        save_to(&path, &reg).unwrap();
        let loaded = load_from(&path);
        assert_eq!(loaded.books.len(), 1);
        assert_eq!(loaded.books["/book.epub"].chapter_idx, 2);
    }

    #[test]
    fn save_creates_parent_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/deeper/registry.json");
        let reg = Registry::default();
        save_to(&path, &reg).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn save_does_not_leave_tmp_file_on_success() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.json");
        save_to(&path, &Registry::default()).unwrap();
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn persistence_open_at_returns_empty_for_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.json");
        let p = Persistence::open_at(path);
        assert!(p.get("/anything").is_none());
    }

    #[test]
    fn persistence_upsert_then_flush_then_reopen_persists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.json");
        {
            let mut p = Persistence::open_at(path.clone());
            p.upsert(
                "/book.epub".into(),
                Position {
                    title: "T".into(),
                    author: "A".into(),
                    chapter_idx: 3,
                    line_offset: 99,
                    last_read: Utc::now(),
                },
            );
            p.flush().unwrap();
        }
        let p = Persistence::open_at(path);
        let pos = p.get("/book.epub").expect("position should persist");
        assert_eq!(pos.chapter_idx, 3);
        assert_eq!(pos.line_offset, 99);
    }

    #[test]
    fn persistence_upsert_overwrites_existing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.json");
        let mut p = Persistence::open_at(path);
        let make = |ch| Position {
            title: "T".into(),
            author: "A".into(),
            chapter_idx: ch,
            line_offset: 0,
            last_read: Utc::now(),
        };
        p.upsert("/k".into(), make(1));
        p.upsert("/k".into(), make(2));
        assert_eq!(p.get("/k").unwrap().chapter_idx, 2);
    }
}
