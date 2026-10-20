//! File-system boundary with DOS-style case-insensitive lookup.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Errors from the file boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileError {
    NotFound(String),
    Io(String),
    UnsafePath(String),
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(name) => write!(f, "file not found: {name}"),
            Self::Io(message) => write!(f, "I/O error: {message}"),
            Self::UnsafePath(name) => write!(f, "unsafe path: {name}"),
        }
    }
}

impl std::error::Error for FileError {}

/// The platform file boundary.
pub trait GameFiles {
    fn read(&self, name: &str) -> Result<Vec<u8>, FileError>;
    fn exists(&self, name: &str) -> bool;
    /// Create or replace a file.
    fn write(&mut self, name: &str, data: &[u8]) -> Result<(), FileError>;
    /// List files with a case-insensitive suffix (for example `.sav`),
    /// returned as upper-case names in sorted order.
    fn list(&self, suffix: &str) -> Vec<String>;
}

/// A real directory, resolving names case-insensitively as DOS did.
#[derive(Debug, Clone)]
pub struct CaseInsensitiveDir {
    root: PathBuf,
}

impl CaseInsensitiveDir {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn resolve(&self, name: &str) -> Result<PathBuf, FileError> {
        if name.contains("..") || Path::new(name).is_absolute() {
            return Err(FileError::UnsafePath(name.to_string()));
        }
        let direct = self.root.join(name);
        if direct.exists() {
            return Ok(direct);
        }
        let mut current = self.root.clone();
        for part in name.split(['/', '\\']).filter(|p| !p.is_empty()) {
            let Ok(entries) = std::fs::read_dir(&current) else {
                return Err(FileError::NotFound(name.to_string()));
            };
            let found = entries
                .flatten()
                .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(part));
            match found {
                Some(e) => current = e.path(),
                None => return Err(FileError::NotFound(name.to_string())),
            }
        }
        Ok(current)
    }
}

impl GameFiles for CaseInsensitiveDir {
    fn read(&self, name: &str) -> Result<Vec<u8>, FileError> {
        let path = self.resolve(name)?;
        std::fs::read(&path).map_err(|e| FileError::Io(e.to_string()))
    }

    fn exists(&self, name: &str) -> bool {
        self.resolve(name).is_ok()
    }

    fn write(&mut self, name: &str, data: &[u8]) -> Result<(), FileError> {
        if name.contains("..") || Path::new(name).is_absolute() {
            return Err(FileError::UnsafePath(name.to_string()));
        }
        let path = match self.resolve(name) {
            Ok(path) => path,
            Err(FileError::NotFound(_)) => self.root.join(name),
            Err(error) => return Err(error),
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| FileError::Io(e.to_string()))?;
        }
        std::fs::write(&path, data).map_err(|e| FileError::Io(e.to_string()))
    }

    fn list(&self, suffix: &str) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter(|entry| entry.path().is_file())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| {
                name.to_ascii_lowercase()
                    .ends_with(&suffix.to_ascii_lowercase())
            })
            .map(|name| name.to_ascii_uppercase())
            .collect();
        names.sort();
        names
    }
}

/// In-memory files for tests, keyed case-insensitively.
#[derive(Debug, Default, Clone)]
pub struct MemoryFiles {
    files: BTreeMap<String, Vec<u8>>,
}

impl MemoryFiles {
    pub fn insert(&mut self, name: &str, data: Vec<u8>) {
        self.files.insert(name.to_ascii_uppercase(), data);
    }
}

impl GameFiles for MemoryFiles {
    fn read(&self, name: &str) -> Result<Vec<u8>, FileError> {
        self.files
            .get(&name.to_ascii_uppercase())
            .cloned()
            .ok_or_else(|| FileError::NotFound(name.to_string()))
    }

    fn exists(&self, name: &str) -> bool {
        self.files.contains_key(&name.to_ascii_uppercase())
    }

    fn write(&mut self, name: &str, data: &[u8]) -> Result<(), FileError> {
        self.files.insert(name.to_ascii_uppercase(), data.to_vec());
        Ok(())
    }

    fn list(&self, suffix: &str) -> Vec<String> {
        let suffix = suffix.to_ascii_uppercase();
        self.files
            .keys()
            .filter(|name| name.ends_with(&suffix))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_files_are_case_insensitive() {
        let mut files = MemoryFiles::default();
        files.insert("Game.CFG", b"cfg".to_vec());
        assert_eq!(files.read("game.cfg").unwrap(), b"cfg");
        assert!(files.exists("GAME.CFG"));
        assert!(!files.exists("missing"));
        files.write("slot1.sav", b"save").unwrap();
        assert_eq!(files.read("SLOT1.SAV").unwrap(), b"save");
        assert_eq!(files.list(".sav"), vec!["SLOT1.SAV"]);
    }

    #[test]
    fn real_directory_resolves_case_insensitively() {
        let dir = std::env::temp_dir().join(format!("soe-files-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Episode1.DAT"), b"ep1").unwrap();
        let mut files = CaseInsensitiveDir::new(&dir);
        assert_eq!(files.read("EPISODE1.DAT").unwrap(), b"ep1");
        assert_eq!(files.read("episode1.dat").unwrap(), b"ep1");
        assert!(!files.exists("missing"));
        assert!(matches!(files.read("../x"), Err(FileError::UnsafePath(_))));
        files.write("new.sav", b"save").unwrap();
        assert_eq!(files.read("NEW.SAV").unwrap(), b"save");
        assert_eq!(files.list(".SAV"), vec!["NEW.SAV"]);
        std::fs::remove_dir_all(&dir).ok();
    }
}
