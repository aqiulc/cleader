use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Position {
    pub title: String,
    pub author: String,
    pub chapter_idx: usize,
    pub line_offset: usize,
    pub last_read: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Registry {
    pub version: u32,
    #[serde(default)]
    pub books: BTreeMap<String, Position>,
}

impl Default for Registry {
    fn default() -> Self {
        Self { version: 1, books: BTreeMap::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
