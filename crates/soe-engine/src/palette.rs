//! Palette tables from `EXCAL_UNPACKED.EXE`.
//!
//! 32-colour tables of 4-bit RGB components live in DGROUP at 96-byte
//! strides from `DS 0x1F79` to `DS 0x2339`; values are expanded with `v * 17`
//! for 8-bit output. Room `DS 0x2099`, map `DS 0x20F9` and UI `DS 0x2159` are
//! the common ones. Rooms start from `0x2099` and are then patched per scene
//! object (`scene_palette`, ported from `FUN_2b79_0003`).

#![forbid(unsafe_code)]

use std::path::Path;

use soe_assets::{Palette, Rgb};

/// The room/map/UI palettes plus the raw DGROUP tables the scene renderer
/// patches from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteSet {
    pub room: Palette,
    pub map: Palette,
    pub ui: Palette,
    /// Raw DGROUP bytes `DS RAW_START..RAW_END` (palette tables and the
    /// 3-byte patch colours at `DS 0x2399..0x23E7`). Empty when unavailable.
    raw: Vec<u8>,
}

/// Offset of DGROUP inside the unpacked payload (`3364:0000` - `1000:0000`).
pub const DGROUP_OFFSET: usize = 0x23640;
pub const ROOM_DS: usize = 0x2099;
pub const MAP_DS: usize = 0x20f9;
pub const UI_DS: usize = 0x2159;
pub const PALETTE_BYTES: usize = 96;
const RAW_START: usize = 0x1f79;
const RAW_END: usize = 0x23e7;

impl PaletteSet {
    /// All-black palettes, for headless sessions without the executable.
    #[must_use]
    pub fn black() -> Self {
        Self {
            room: Palette::black(),
            map: Palette::black(),
            ui: Palette::black(),
            raw: Vec::new(),
        }
    }

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
            raw: bytes.get(dgroup + RAW_START..dgroup + RAW_END)?.to_vec(),
        })
    }

    /// The palette the room renderer leaves active after drawing a scene
    /// whose objects have these resource ids, in draw order.
    ///
    /// Port of the palette path of `FUN_2b79_0003` (`2b79:0055..0094` and the
    /// per-object rules at `2b79:094f..0c42`, which Ghidra splits across
    /// `FUN_2b79_0397`, `FUN_2b79_0a7f` and `FUN_2b79_0aca`). The working
    /// buffer is `DS 0x9BA0`, seeded from `0x2099`; `DS:8E5D` points at either
    /// that buffer or a fixed table. `first` is the local at `[BP-3]`.
    /// Falls back to `room` when the raw tables are unavailable.
    #[must_use]
    pub fn scene_palette(&self, ids: impl IntoIterator<Item = u16>) -> Palette {
        if self.raw.is_empty() {
            return self.room.clone();
        }
        let mut buf = [[0u8; 3]; 32];
        for (index, entry) in buf.iter_mut().enumerate() {
            *entry = self.triple(ROOM_DS + index * 3);
        }
        // `None` = the 0x9BA0 buffer, `Some(ds)` = a fixed DGROUP table.
        let mut active: Option<usize> = None;
        let mut first = true;

        for id in ids {
            if (0x44c..0x4b0).contains(&id) && first {
                active = Some(MAP_DS);
            }
            if (0x514..0x578).contains(&id) && first {
                for index in 18..22 {
                    buf[index] = self.triple(0x2399 + (index - 18) * 3);
                }
                buf[27] = self.triple(0x23a5);
                first = false;
            }
            if id == 0x40d {
                buf[18] = self.triple(0x23a8);
                buf[19] = self.triple(0x23ab);
                buf[20] = self.triple(0x23ae);
                buf[27] = self.triple(0x23b1);
                buf[31] = self.triple(0x23b4);
                first = false;
            }
            if id == 0x47f {
                for index in 0..27 {
                    buf[index] = self.triple(MAP_DS + index * 3);
                }
                for index in 27..32 {
                    buf[index] = self.triple(0x23c6 + (index - 27) * 3);
                }
                active = None;
                first = false;
                continue;
            }
            // `2b79:0aca` chain: the first matching rule applies, then
            // `2b79:0c3e` clears `first`.
            let matched = match id {
                0x578..=0x5db => {
                    buf[27] = self.triple(0x23c3);
                    true
                }
                0x402..=0x404 => {
                    buf[27] = self.triple(0x23b7);
                    true
                }
                0x40a => {
                    buf[21] = self.triple(0x23ba);
                    buf[27] = self.triple(0x23bd);
                    true
                }
                0x40b => {
                    buf[27] = self.triple(0x23c0);
                    true
                }
                0x40c => {
                    for index in 27..32 {
                        buf[index] = self.triple(0x23d5 + (index - 27) * 3);
                    }
                    true
                }
                0x414 => {
                    active = Some(0x2219);
                    true
                }
                0x455 | 0x5dc..=0x63f => {
                    active = Some(UI_DS);
                    true
                }
                0x640..=0x6a3 => {
                    active = Some(0x2279);
                    true
                }
                0x6a4..=0x707 => {
                    active = Some(0x22d9);
                    true
                }
                0x708..=0x76b => {
                    active = Some(0x2339);
                    true
                }
                0x76c..=0x77a => {
                    active = Some(0x21b9);
                    true
                }
                _ => false,
            };
            if matched {
                first = false;
            }
        }

        let colors = (0..32)
            .map(|index| {
                let [r, g, b] = match active {
                    None => buf[index],
                    Some(ds) => self.triple(ds + index * 3),
                };
                [r * 17, g * 17, b * 17]
            })
            .collect();
        Palette::new(colors)
    }

    /// One 3-byte DGROUP entry at `DS ds`.
    fn triple(&self, ds: usize) -> [u8; 3] {
        let at = ds - RAW_START;
        [self.raw[at], self.raw[at + 1], self.raw[at + 2]]
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

    fn exe_set() -> Option<PaletteSet> {
        let bytes = std::fs::read("../../ghidra/EXCAL_UNPACKED.EXE").ok()?;
        PaletteSet::from_exe(&bytes)
    }

    fn table(set: &PaletteSet, ds: usize) -> Palette {
        Palette::new(
            (0..32)
                .map(|index| {
                    let [r, g, b] = set.triple(ds + index * 3);
                    [r * 17, g * 17, b * 17]
                })
                .collect(),
        )
    }

    fn expand(set: &PaletteSet, ds: usize) -> Rgb {
        let [r, g, b] = set.triple(ds);
        [r * 17, g * 17, b * 17]
    }

    #[test]
    fn scene_palette_without_rules_is_room_table() {
        let Some(set) = exe_set() else {
            eprintln!("SKIP: EXCAL_UNPACKED.EXE absent");
            return;
        };
        // 0x40F is the shared sky image and triggers no rule.
        assert_eq!(set.scene_palette([0x40f, 0x3ed]), set.room);
        assert_eq!(PaletteSet::black().scene_palette([0x521]), Palette::black());
    }

    #[test]
    fn scene_palette_patches_and_first_flag() {
        let Some(set) = exe_set() else {
            eprintln!("SKIP: EXCAL_UNPACKED.EXE absent");
            return;
        };
        // 0x514..0x577 while `first`: entries 18..21 and 27.
        let village = set.scene_palette([0x40f, 0x521]);
        for index in 18..22 {
            assert_eq!(
                village.colors[index],
                expand(&set, 0x2399 + (index - 18) * 3)
            );
        }
        assert_eq!(village.colors[27], expand(&set, 0x23a5));
        assert_eq!(village.colors[0..18], set.room.colors[0..18]);

        // 0x40D clears `first`, so a later 0x514.. object no longer patches.
        let after = set.scene_palette([0x40d, 0x521]);
        assert_eq!(after.colors[18], expand(&set, 0x23a8));
        assert_eq!(after.colors[31], expand(&set, 0x23b4));

        // Chain rules patch one entry and clear `first`.
        let thatch = set.scene_palette([0x578, 0x521]);
        assert_eq!(thatch.colors[27], expand(&set, 0x23c3));
        assert_eq!(thatch.colors[18], set.room.colors[18]);
    }

    #[test]
    fn scene_palette_table_switches() {
        let Some(set) = exe_set() else {
            eprintln!("SKIP: EXCAL_UNPACKED.EXE absent");
            return;
        };
        assert_eq!(set.scene_palette([0x414]), table(&set, 0x2219));
        assert_eq!(set.scene_palette([0x455]), table(&set, UI_DS));
        assert_eq!(set.scene_palette([0x5dc]), table(&set, UI_DS));
        assert_eq!(set.scene_palette([0x640]), table(&set, 0x2279));
        assert_eq!(set.scene_palette([0x6a4]), table(&set, 0x22d9));
        assert_eq!(set.scene_palette([0x708]), table(&set, 0x2339));
        assert_eq!(set.scene_palette([0x77a]), table(&set, 0x21b9));
        assert_eq!(set.scene_palette([0x77b]), set.room);
        // 0x44C.. selects the map table without clearing `first`; later
        // buffer patches stay invisible while a fixed table is active.
        assert_eq!(set.scene_palette([0x44c, 0x521]), table(&set, MAP_DS));
        // 0x47F rebuilds the buffer from the map table plus 0x23C6.. and
        // makes it active again.
        let rebuilt = set.scene_palette([0x414, 0x47f]);
        assert_eq!(rebuilt.colors[0..27], table(&set, MAP_DS).colors[0..27]);
        assert_eq!(rebuilt.colors[27], expand(&set, 0x23c6));
        assert_eq!(rebuilt.colors[31], expand(&set, 0x23d2));
    }
}
