//! Palette tables from `EXCAL_UNPACKED.EXE`.
//!
//! Three 96-byte 6-bit RGB tables live in DGROUP: room `DS 0x2099`, map
//! `DS 0x20F9`, UI `DS 0x2159`. Each is 32 colours; values are expanded with
//! `v * 17` for 8-bit VGA output.

#![forbid(unsafe_code)]

use std::path::Path;

use soe_assets::{Palette, Rgb};

/// The room/map/UI palettes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteSet {
    pub room: Palette,
    pub map: Palette,
    pub ui: Palette,
}

/// Offset of DGROUP inside the unpacked payload (`3364:0000` - `1000:0000`).
pub const DGROUP_OFFSET: usize = 0x23640;
pub const ROOM_DS: usize = 0x2099;
pub const MAP_DS: usize = 0x20f9;
pub const UI_DS: usize = 0x2159;
pub const PALETTE_BYTES: usize = 96;

impl PaletteSet {
    /// Parse palettes from a complete MZ executable image.
    #[must_use]
    pub fn from_exe(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 0x1c || &bytes[..2] != b"MZ" {
            return None;
        }
        let header_paras = usize::from(u16::from_le_bytes([bytes[8], bytes[9]]));
        let payload = header_paras.checked_mul(16)?;
        let dgroup = payload.checked_add(DGROUP_OFFSET)?;
        Some(Self {
            room: palette_at(bytes, dgroup + ROOM_DS)?,
            map: palette_at(bytes, dgroup + MAP_DS)?,
            ui: palette_at(bytes, dgroup + UI_DS)?,
        })
    }

    /// Load palettes from an executable path.
    pub fn load(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let bytes = std::fs::read(path)?;
        Self::from_exe(&bytes).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "EXCAL image has no DGROUP palette tables",
            )
        })
    }
}

fn palette_at(bytes: &[u8], offset: usize) -> Option<Palette> {
    let raw = bytes.get(offset..offset + PALETTE_BYTES)?;
    if !raw.iter().all(|value| *value <= 0x3f) {
        return None;
    }
    let colors: Vec<Rgb> = raw
        .chunks_exact(3)
        .map(|c| [c[0] * 17, c[1] * 17, c[2] * 17])
        .collect();
    Some(Palette::new(colors))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_three_palettes() {
        let Ok(bytes) = std::fs::read("../../ghidra/EXCAL_UNPACKED.EXE") else {
            eprintln!("SKIP: EXCAL_UNPACKED.EXE absent");
            return;
        };
        let set = PaletteSet::from_exe(&bytes).unwrap();
        assert_eq!(set.room.colors.len(), 32);
        assert_eq!(set.map.colors.len(), 32);
        assert_eq!(set.ui.colors.len(), 32);
        // The room palette starts with several dark entries but is not all zero.
        assert!(set.room.colors.iter().any(|c| *c != [0, 0, 0]));
    }
}
