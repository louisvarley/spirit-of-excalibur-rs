//! Resource manager: archive cache, record lookup and decompressed-payload
//! cache. Mirrors the original's "Resource not found" boundary.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use soe_res::ResFile;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceError {
    MissingFile(String),
    MissingRecord { file: String, id: u16 },
    Decode(String),
}

impl std::fmt::Display for ResourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingFile(file) => write!(f, "missing resource file {file}"),
            Self::MissingRecord { file, id } => {
                write!(f, "record {id} not found in {file}")
            }
            Self::Decode(message) => write!(f, "resource decode error: {message}"),
        }
    }
}

impl std::error::Error for ResourceError {}

/// Loaded `.RES` archives plus decoded-record cache.
#[derive(Default)]
pub struct ResourceManager {
    archives: BTreeMap<String, ResFile>,
    decoded: HashMap<(String, u16), Vec<u8>>,
}

impl ResourceManager {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Load an archive from bytes under a case-insensitive name.
    pub fn load(&mut self, name: &str, data: Vec<u8>) -> Result<(), ResourceError> {
        let file = ResFile::parse(data).map_err(|e| ResourceError::Decode(e.to_string()))?;
        self.archives.insert(name.to_ascii_uppercase(), file);
        Ok(())
    }

    /// Load an archive from a directory, resolving the name case-insensitively.
    pub fn load_from_dir(&mut self, dir: &Path, name: &str) -> Result<(), ResourceError> {
        let files = crate::files::CaseInsensitiveDir::new(dir);
        use crate::files::GameFiles;
        let data = files
            .read(name)
            .map_err(|_| ResourceError::MissingFile(name.to_string()))?;
        self.load(name, data)
    }

    /// First archive (in name order) containing `id`. Scene object ids are
    /// pooled across the region archives; a few ids overlap between archives.
    #[must_use]
    pub fn find_file(&self, id: u16) -> Option<&str> {
        self.archives
            .iter()
            .find(|(_, archive)| archive.records().iter().any(|record| record.id == id))
            .map(|(name, _)| name.as_str())
    }

    /// Decode one record, caching the result.
    pub fn record(&mut self, file: &str, id: u16) -> Result<&[u8], ResourceError> {
        let key = (file.to_ascii_uppercase(), id);
        if !self.decoded.contains_key(&key) {
            let archive = self
                .archives
                .get(&key.0)
                .ok_or_else(|| ResourceError::MissingFile(file.to_string()))?;
            let rec = archive
                .records()
                .into_iter()
                .find(|r| r.id == id)
                .ok_or_else(|| ResourceError::MissingRecord {
                    file: file.to_string(),
                    id,
                })?;
            let data = archive
                .decode(rec)
                .map_err(|e| ResourceError::Decode(e.to_string()))?;
            self.decoded.insert(key.clone(), data);
        }
        Ok(&self.decoded[&key])
    }

    #[must_use]
    pub fn has_file(&self, file: &str) -> bool {
        self.archives.contains_key(&file.to_ascii_uppercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_and_decodes_music_first_record() {
        let path = Path::new("../../game/MUSIC.RES");
        let Ok(data) = std::fs::read(path) else {
            eprintln!("SKIP: MUSIC.RES absent");
            return;
        };
        let mut rm = ResourceManager::new();
        rm.load("music.res", data).unwrap();
        let rec = rm.record("MUSIC.RES", 2).unwrap();
        assert!(rec.starts_with(b"FORM"));
        assert_eq!(rec.len(), 1216);
    }

    #[test]
    fn scene_objects_resolve_across_region_archives() {
        let dir = Path::new("../../game/RES32");
        let Ok(entries) = std::fs::read_dir(dir) else {
            eprintln!("SKIP: RES32 absent");
            return;
        };
        let mut rm = ResourceManager::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.to_ascii_uppercase().ends_with(".RES") {
                rm.load_from_dir(dir, &name).unwrap();
            }
        }
        assert_eq!(rm.find_file(1039), Some("EXTER.RES"));
        assert_eq!(rm.find_file(1403), Some("URBAN.RES"));
        assert_eq!(rm.find_file(908), Some("ANI.RES"));
        let data = rm.record("EXTER.RES", 1039).unwrap().to_vec();
        let image = soe_video::decode_planar(&data).unwrap();
        assert!(image.width > 0 && image.height > 0);
    }

    #[test]
    fn missing_record_is_reported() {
        let mut rm = ResourceManager::new();
        assert_eq!(
            rm.record("NOPE.RES", 1),
            Err(ResourceError::MissingFile("NOPE.RES".into()))
        );
    }
}
