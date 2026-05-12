//! User preferences (view mode, etc.) persisted to JSON at
//! `<data_dir>/prefs.json`. Mirrors `persistence.rs` for save semantics
//! (atomic temp+rename, default-on-missing) but simpler — no migration
//! and no versioning yet; the file is small enough that a future
//! schema break can be handled by wiping and starting fresh.

use crate::error::PersistenceError;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ViewMode {
    #[default]
    Grid,
    List,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Prefs {
    #[serde(default)]
    pub view_mode: ViewMode,
}

pub fn load_from(path: &Path) -> Prefs {
    match std::fs::read_to_string(path) {
        Ok(s) => match serde_json::from_str::<Prefs>(&s) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("cleader: prefs corrupt ({e}), starting fresh");
                Prefs::default()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Prefs::default(),
        Err(e) => {
            eprintln!("cleader: could not read prefs ({e}), starting fresh");
            Prefs::default()
        }
    }
}

pub fn save_to(path: &Path, prefs: &Prefs) -> Result<(), PersistenceError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_extension("json.tmp");
    {
        // Scope: drop file handle before rename (required on Windows).
        let mut tmp = std::fs::File::create(&tmp_path)?;
        let bytes = serde_json::to_vec_pretty(prefs)?;
        tmp.write_all(&bytes)?;
        tmp.sync_all()?;
    }
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

pub struct PrefsStore {
    path: PathBuf,
    prefs: Prefs,
}

impl PrefsStore {
    /// Resolve the platform-correct prefs path and load existing state.
    pub fn open() -> Result<Self, PersistenceError> {
        let path = default_prefs_path()?;
        let prefs = load_from(&path);
        Ok(Self { path, prefs })
    }

    /// Open against an explicit path. Intended for tests and internal tooling
    /// only — production code should use `open()`.
    #[doc(hidden)]
    pub fn open_at(path: PathBuf) -> Self {
        let prefs = load_from(&path);
        Self { path, prefs }
    }

    pub fn view_mode(&self) -> ViewMode {
        self.prefs.view_mode
    }

    pub fn set_view_mode(&mut self, mode: ViewMode) -> Result<(), PersistenceError> {
        self.prefs.view_mode = mode;
        save_to(&self.path, &self.prefs)
    }
}

fn default_prefs_path() -> Result<PathBuf, PersistenceError> {
    let dirs = ProjectDirs::from("", "", "cleader")
        .ok_or(PersistenceError::NoDataDir)?;
    Ok(dirs.data_dir().join("prefs.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_view_mode_is_grid() {
        assert_eq!(Prefs::default().view_mode, ViewMode::Grid);
    }

    #[test]
    fn view_mode_serializes_as_lowercase_string() {
        // Wire format must stay "grid"/"list" — variant rename or
        // serde attribute change would break existing prefs files.
        assert_eq!(serde_json::to_string(&ViewMode::Grid).unwrap(), "\"grid\"");
        assert_eq!(serde_json::to_string(&ViewMode::List).unwrap(), "\"list\"");
    }

    #[test]
    fn load_from_missing_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nope.json");
        assert_eq!(load_from(&path), Prefs::default());
    }

    #[test]
    fn load_from_corrupt_json_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, b"this is not json").unwrap();
        assert_eq!(load_from(&path), Prefs::default());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prefs.json");
        let prefs = Prefs { view_mode: ViewMode::List };
        save_to(&path, &prefs).unwrap();
        assert_eq!(load_from(&path), prefs);
    }

    #[test]
    fn set_view_mode_writes_immediately() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prefs.json");
        let mut store = PrefsStore::open_at(path.clone());
        store.set_view_mode(ViewMode::List).unwrap();
        let on_disk = load_from(&path);
        assert_eq!(on_disk.view_mode, ViewMode::List);
    }

    #[test]
    fn open_at_returns_default_for_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = PrefsStore::open_at(dir.path().join("nope.json"));
        assert_eq!(store.view_mode(), ViewMode::Grid);
    }

    #[test]
    fn save_does_not_leave_tmp_file_on_success() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prefs.json");
        save_to(&path, &Prefs::default()).unwrap();
        assert!(!path.with_extension("json.tmp").exists());
    }
}
