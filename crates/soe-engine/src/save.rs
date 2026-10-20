//! EPISODE*.DAT / *.SAV loader.
//!
//! File layout is verified (see `notes/SAVESTATE.md`). The DGROUP mapping was
//! resolved from the loader at `1a51:059d`:
//! - file `0x0000..0x235E` (9055 bytes) -> DS `0x2C5C..0x4FBA`
//! - file `0x235F..0x2374` (22 bytes) -> DS `0x4FBF..0x4FD4`
//! so character 0 is at DS `0x2C8C` and its name at `0x2C96`.

#![forbid(unsafe_code)]

pub const FILE_SIZE: usize = 9077;
pub const HEADER_LEN: usize = 0x30;
pub const CHAR_BASE: usize = 0x30;
pub const CHAR_SIZE: usize = 0x38;
pub const CHAR_COUNT: usize = 64;
pub const LOC_BASE: usize = 0x1473;
pub const LOC_SIZE: usize = 0x19;
pub const LOC_COUNT: usize = 49;
pub const ITEM_BASE: usize = 0x1C4A;
pub const ITEM_SIZE: usize = 0x1B;
pub const ITEM_COUNT: usize = 35;
pub const SPELL_BASE: usize = 0x1FFB;
pub const SPELL_COUNT: usize = 6;
pub const FLAG_BASE: usize = 0x2175;
pub const FLAG_LEN: usize = 0x22C9 - 0x2175;
pub const REL_BASE: usize = 0x22C9;
pub const REL_COUNT: usize = 50;
pub const REL_SIZE: usize = 3;
pub const TAIL_BASE: usize = 0x235F;
pub const TAIL_LEN: usize = 22;

pub const DS_MAIN: usize = 0x2C5C;
pub const DS_TAIL: usize = 0x4FBF;
pub const DS_IMAGE_LEN: usize = 0x4FD5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Character {
    pub raw: [u8; CHAR_SIZE],
}
impl Character {
    #[must_use]
    pub fn name(&self) -> String {
        c_string(&self.raw[0x0A..0x16])
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub raw: [u8; LOC_SIZE],
}
impl Location {
    #[must_use]
    pub fn name(&self) -> String {
        c_string(&self.raw[0..0x12])
    }
    #[must_use]
    pub fn world_x(&self) -> u16 {
        u16::from_le_bytes([self.raw[0x12], self.raw[0x13]])
    }
    #[must_use]
    pub fn world_y(&self) -> u16 {
        u16::from_le_bytes([self.raw[0x14], self.raw[0x15]])
    }
    #[must_use]
    pub fn kind(&self) -> u16 {
        u16::from_le_bytes([self.raw[0x16], self.raw[0x17]])
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub raw: [u8; ITEM_SIZE],
}
impl Item {
    #[must_use]
    pub fn name(&self) -> String {
        c_string(&self.raw[0..0x12])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Relationship {
    pub a: u8,
    pub b: u8,
    pub value: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameState {
    pub header: [u8; HEADER_LEN],
    pub characters: Vec<Character>,
    pub locations: Vec<Location>,
    pub items: Vec<Item>,
    pub spells: Vec<Item>,
    pub flags: Vec<u8>,
    pub relationships: Vec<Relationship>,
    pub tail22: [u8; TAIL_LEN],
    /// Original bytes; preserves gaps between the parsed tables on write.
    raw: Vec<u8>,
}

impl GameState {
    /// Parse a 9,077-byte episode or save file.
    pub fn parse(data: &[u8]) -> Result<Self, String> {
        if data.len() != FILE_SIZE {
            return Err(format!("expected {FILE_SIZE} bytes, got {}", data.len()));
        }
        let mut header = [0u8; HEADER_LEN];
        header.copy_from_slice(&data[..HEADER_LEN]);
        let characters = (0..CHAR_COUNT)
            .map(|i| Character {
                raw: array_at(data, CHAR_BASE + i * CHAR_SIZE),
            })
            .collect();
        let locations = (0..LOC_COUNT)
            .map(|i| Location {
                raw: array_at(data, LOC_BASE + i * LOC_SIZE),
            })
            .collect();
        let items = (0..ITEM_COUNT)
            .map(|i| Item {
                raw: array_at(data, ITEM_BASE + i * ITEM_SIZE),
            })
            .collect();
        let spells = (0..SPELL_COUNT)
            .map(|i| Item {
                raw: array_at(data, SPELL_BASE + i * ITEM_SIZE),
            })
            .collect();
        let flags = data[FLAG_BASE..FLAG_BASE + FLAG_LEN].to_vec();
        let relationships = (0..REL_COUNT)
            .map(|i| {
                let o = REL_BASE + i * REL_SIZE;
                Relationship {
                    a: data[o],
                    b: data[o + 1],
                    value: data[o + 2],
                }
            })
            .collect();
        let mut tail22 = [0u8; TAIL_LEN];
        tail22.copy_from_slice(&data[TAIL_BASE..TAIL_BASE + TAIL_LEN]);
        Ok(Self {
            header,
            characters,
            locations,
            items,
            spells,
            flags,
            relationships,
            tail22,
            raw: data.to_vec(),
        })
    }

    /// Serialize back to the 9,077-byte episode/save layout. Unknown gaps keep
    /// their original bytes.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut data = self.raw.clone();
        data[..HEADER_LEN].copy_from_slice(&self.header);
        for (i, character) in self.characters.iter().enumerate() {
            let o = CHAR_BASE + i * CHAR_SIZE;
            data[o..o + CHAR_SIZE].copy_from_slice(&character.raw);
        }
        for (i, location) in self.locations.iter().enumerate() {
            let o = LOC_BASE + i * LOC_SIZE;
            data[o..o + LOC_SIZE].copy_from_slice(&location.raw);
        }
        for (i, item) in self.items.iter().enumerate() {
            let o = ITEM_BASE + i * ITEM_SIZE;
            data[o..o + ITEM_SIZE].copy_from_slice(&item.raw);
        }
        for (i, spell) in self.spells.iter().enumerate() {
            let o = SPELL_BASE + i * ITEM_SIZE;
            data[o..o + ITEM_SIZE].copy_from_slice(&spell.raw);
        }
        data[FLAG_BASE..FLAG_BASE + FLAG_LEN].copy_from_slice(&self.flags);
        for (i, relationship) in self.relationships.iter().enumerate() {
            let o = REL_BASE + i * REL_SIZE;
            data[o] = relationship.a;
            data[o + 1] = relationship.b;
            data[o + 2] = relationship.value;
        }
        data[TAIL_BASE..TAIL_BASE + TAIL_LEN].copy_from_slice(&self.tail22);
        data
    }

    /// Reconstruct the DGROUP bytes the original loader writes.
    #[must_use]
    pub fn dgroup_image(&self, file: &[u8]) -> Vec<u8> {
        let mut ds = vec![0u8; DS_IMAGE_LEN];
        ds[DS_MAIN..DS_MAIN + TAIL_BASE].copy_from_slice(&file[..TAIL_BASE]);
        ds[DS_TAIL..DS_TAIL + TAIL_LEN].copy_from_slice(&file[TAIL_BASE..FILE_SIZE]);
        ds
    }
}

fn array_at<const N: usize>(data: &[u8], off: usize) -> [u8; N] {
    let mut out = [0u8; N];
    out.copy_from_slice(&data[off..off + N]);
    out
}

fn c_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn episode() -> Option<Vec<u8>> {
        let dir = std::env::var_os("SOE_GAME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("../../game"));
        std::fs::read(dir.join("EPISODE1.DAT")).ok()
    }

    #[test]
    fn parses_episode_tables_and_dgroup_mapping() {
        let Some(data) = episode() else {
            eprintln!("SKIP: EPISODE1.DAT absent");
            return;
        };
        let state = GameState::parse(&data).unwrap();
        assert_eq!(state.characters[0].name(), "Constantine");
        assert_eq!(state.characters[1].name(), "Bedivere");
        assert_eq!(state.locations[0].name(), "Colchester");
        assert_eq!(state.items[0].name(), "gold besant");
        assert_eq!(state.spells[0].name(), "Repletion");
        assert_eq!(state.tail22.len(), 22);

        let ds = state.dgroup_image(&data);
        assert_eq!(&ds[0x2C96..0x2CA2], b"Constantine\0");
        assert_eq!(ds[0x2C8C + 0x0A], b'C');
        assert_eq!(&ds[0x4FBF..0x4FD5], &data[TAIL_BASE..]);
    }

    #[test]
    fn parses_every_provided_save() {
        let dir = PathBuf::from("../../game");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            eprintln!("SKIP: game dir absent");
            return;
        };
        let mut count = 0;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.to_ascii_lowercase().ends_with(".sav") {
                continue;
            }
            let data = std::fs::read(entry.path()).unwrap();
            let state = GameState::parse(&data).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(state.to_bytes(), data, "{name} round-trip");
            count += 1;
        }
        assert!(count > 0, "no .sav files found");
    }

    #[test]
    fn roundtrips_and_keeps_unknown_gaps() {
        let Some(data) = episode() else {
            eprintln!("SKIP: EPISODE1.DAT absent");
            return;
        };
        let mut state = GameState::parse(&data).unwrap();
        assert_eq!(state.to_bytes(), data);
        state.characters[0].raw[0x0A..0x10].copy_from_slice(b"Artur\0");
        let changed = state.to_bytes();
        assert_eq!(&changed[CHAR_BASE + 0x0A..CHAR_BASE + 0x10], b"Artur\0");
        assert_eq!(
            GameState::parse(&changed).unwrap().characters[0].name(),
            "Artur"
        );
    }
}
