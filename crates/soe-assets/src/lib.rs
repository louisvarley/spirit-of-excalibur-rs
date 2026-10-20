//! Resource IDs and layered asset sources.
//!
//! No image-format decoding is implemented here yet. The types are the stable
//! seam for later original `.RES` loading and RGBA override packs.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A stable identifier for one image/resource.
///
/// `record` is the index inside the container; `name` is optional metadata
/// recovered from the resource directory (some records are unnamed).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ResourceId {
    pub file: String,
    pub record: u32,
    pub name: Option<String>,
}

impl ResourceId {
    /// Construct a resource ID.
    #[must_use]
    pub fn new(file: impl Into<String>, record: u32, name: Option<impl Into<String>>) -> Self {
        Self {
            file: file.into(),
            record,
            name: name.map(Into::into),
        }
    }

    /// The canonical key used by override manifests.
    #[must_use]
    pub fn key(&self) -> String {
        match &self.name {
            Some(name) => format!("{}:{}:{}", self.file, self.record, name),
            None => format!("{}:{}", self.file, self.record),
        }
    }
}

impl fmt::Display for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.key())
    }
}

/// An RGB palette entry.
pub type Rgb = [u8; 3];

/// An 8-bit indexed palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub colors: Vec<Rgb>,
}

impl Palette {
    /// Build a palette from RGB triples.
    #[must_use]
    pub fn new(colors: Vec<Rgb>) -> Self {
        Self { colors }
    }

    /// The default 256-entry black palette.
    #[must_use]
    pub fn black() -> Self {
        Self {
            colors: vec![[0, 0, 0]; 256],
        }
    }

    /// Number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.colors.len()
    }

    /// True when there are no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.colors.is_empty()
    }
}

/// Image data, either original indexed or an RGBA override.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Image {
    /// Original 8-bit indexed image plus palette.
    Indexed {
        width: u16,
        height: u16,
        pixels: Vec<u8>,
        palette: Palette,
    },
    /// RGBA override image. Decoding is intentionally not implemented yet.
    Rgba {
        width: u16,
        height: u16,
        rgba: Vec<u8>,
    },
}

impl Image {
    /// Pixel dimensions.
    #[must_use]
    pub const fn dimensions(&self) -> (u16, u16) {
        match self {
            Self::Indexed { width, height, .. } | Self::Rgba { width, height, .. } => {
                (*width, *height)
            }
        }
    }
}

/// Where an asset will be loaded from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetLocation {
    /// Read from the original `.RES` container.
    Original(ResourceId),
    /// Read from an override pack PNG.
    Override {
        resource: ResourceId,
        path: PathBuf,
        scale: u32,
    },
}

/// Errors from the asset layer.
#[derive(Debug, Error)]
pub enum AssetError {
    #[error("manifest I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("manifest parse error: {0}")]
    Manifest(#[from] toml::de::Error),
    #[error("override pack scale must be >= 1")]
    InvalidScale,
}

/// The TOML manifest of an override pack.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OverrideManifest {
    /// Integer scale of the PNGs in this pack (2, 3, 4, ...).
    pub scale: u32,
    /// Mapping from resource ID key to PNG path relative to the pack root.
    #[serde(rename = "map", default)]
    pub map: BTreeMap<String, PathBuf>,
}

impl OverrideManifest {
    /// Parse a manifest from a TOML string.
    pub fn parse(text: &str) -> Result<Self, AssetError> {
        let manifest: Self = toml::from_str(text)?;
        if manifest.scale < 1 {
            return Err(AssetError::InvalidScale);
        }
        Ok(manifest)
    }
}

/// One override pack on disk.
#[derive(Debug, Clone)]
pub struct OverridePack {
    pub root: PathBuf,
    pub manifest: OverrideManifest,
}

impl OverridePack {
    /// Load `pack.toml` from a directory.
    pub fn load(root: impl Into<PathBuf>) -> Result<Self, AssetError> {
        let root = root.into();
        let text = std::fs::read_to_string(root.join("pack.toml"))?;
        let manifest = OverrideManifest::parse(&text)?;
        Ok(Self { root, manifest })
    }

    /// Resolve one resource in this pack.
    #[must_use]
    pub fn resolve(&self, id: &ResourceId) -> Option<AssetLocation> {
        let path = self.manifest.map.get(&id.key())?;
        Some(AssetLocation::Override {
            resource: id.clone(),
            path: self.root.join(path),
            scale: self.manifest.scale,
        })
    }
}

/// A source of assets.
pub trait AssetSource {
    /// Resolve a resource ID to a location, if this source provides it.
    fn resolve(&self, id: &ResourceId) -> Result<Option<AssetLocation>, AssetError>;
}

/// Original `.RES` source. Loading/decoding is wired in a later step.
#[derive(Debug, Clone)]
pub struct OriginalResSource {
    pub directory: PathBuf,
}

impl AssetSource for OriginalResSource {
    fn resolve(&self, id: &ResourceId) -> Result<Option<AssetLocation>, AssetError> {
        // The presence check is intentionally conservative; no decoding yet.
        if self.directory.join(&id.file).exists() {
            Ok(Some(AssetLocation::Original(id.clone())))
        } else {
            Ok(None)
        }
    }
}

/// Layered source: original first, then override packs (last pack wins).
#[derive(Default)]
pub struct LayeredAssets {
    pub original: Option<Box<dyn AssetSource>>,
    pub overrides: Vec<OverridePack>,
}

impl LayeredAssets {
    /// Build a layered source with an original source and override packs.
    #[must_use]
    pub fn new(original: Option<Box<dyn AssetSource>>, overrides: Vec<OverridePack>) -> Self {
        Self {
            original,
            overrides,
        }
    }

    /// Resolve an asset, preferring the last matching override pack.
    pub fn resolve(&self, id: &ResourceId) -> Result<Option<AssetLocation>, AssetError> {
        for pack in self.overrides.iter().rev() {
            if let Some(location) = pack.resolve(id) {
                return Ok(Some(location));
            }
        }
        match &self.original {
            Some(source) => source.resolve(id),
            None => Ok(None),
        }
    }
}

impl AssetSource for LayeredAssets {
    fn resolve(&self, id: &ResourceId) -> Result<Option<AssetLocation>, AssetError> {
        Self::resolve(self, id)
    }
}

/// Simple in-memory source used by tests and tools.
#[derive(Debug, Default, Clone)]
pub struct MapSource {
    pub entries: BTreeMap<String, PathBuf>,
}

impl AssetSource for MapSource {
    fn resolve(&self, id: &ResourceId) -> Result<Option<AssetLocation>, AssetError> {
        Ok(self
            .entries
            .get(&id.key())
            .map(|path| AssetLocation::Override {
                resource: id.clone(),
                path: path.clone(),
                scale: 1,
            }))
    }
}

/// Convenience for a path relative to a pack root.
#[must_use]
pub fn pack_path(root: &Path, relative: &Path) -> PathBuf {
    root.join(relative)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> ResourceId {
        ResourceId::new("HINT.RES", 3, Some("intro"))
    }

    #[test]
    fn manifest_parses_and_validates_scale() {
        let text = r#"
scale = 2
[map]
"HINT.RES:3:intro" = "hint/3.png"
"#;
        let manifest = OverrideManifest::parse(text).expect("parse");
        assert_eq!(manifest.scale, 2);
        assert_eq!(manifest.map.len(), 1);
        assert!(OverrideManifest::parse("scale = 0").is_err());
    }

    #[test]
    fn override_wins_and_missing_falls_back() {
        let manifest =
            OverrideManifest::parse("scale = 3\n[map]\n\"HINT.RES:3:intro\" = \"hd/intro.png\"\n")
                .expect("parse");
        let pack = OverridePack {
            root: PathBuf::from("/packs/hd"),
            manifest,
        };
        let mut original = MapSource::default();
        original
            .entries
            .insert("HINT.RES:3:intro".into(), PathBuf::from("orig/hint"));
        let layered = LayeredAssets::new(Some(Box::new(original)), vec![pack]);

        let resolved = layered.resolve(&id()).expect("resolve").expect("some");
        match resolved {
            AssetLocation::Override { path, scale, .. } => {
                assert_eq!(scale, 3);
                assert!(path.ends_with("hd/intro.png"));
            }
            AssetLocation::Original(_) => panic!("override should win"),
        }

        let missing = ResourceId::new("HINT.RES", 99, None::<String>);
        assert!(layered.resolve(&missing).expect("resolve").is_none());
    }

    #[test]
    fn resource_key_is_stable() {
        assert_eq!(id().key(), "HINT.RES:3:intro");
        assert_eq!(
            ResourceId::new("HINT.RES", 3, None::<String>).key(),
            "HINT.RES:3"
        );
    }
}
