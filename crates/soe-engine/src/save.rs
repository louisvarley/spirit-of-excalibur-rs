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
/// Runtime character slots. The save tables store 64 party/character records
/// followed by 28 NPC/monster templates used by the event table (actors up to
/// index 92). They end at `0x1450`, before the location table at `0x1473`.
pub const CHAR_COUNT: usize = 92;
pub const LOC_BASE: usize = 0x1473;
pub const LOC_SIZE: usize = 0x19;
pub const LOC_COUNT: usize = 49;
pub const ITEM_BASE: usize = 0x1C4A;
pub const ITEM_SIZE: usize = 0x1B;
/// The object table is 49 records long: items, then prepared spells, then
/// unused slots. `0x1C4A + 49 * 0x1B == 0x2175`.
pub const ITEM_COUNT: usize = 49;
/// Legacy view of the six records at `0x1FFB` (object indices 36..=41).
pub const SPELL_BASE: usize = 0x1FFB;
pub const SPELL_COUNT: usize = 6;
/// Live object instances (def index, owner, scene/amount, flags).
pub const INSTANCE_BASE: usize = 0x2175;
pub const INSTANCE_SIZE: usize = 4;
pub const INSTANCE_COUNT: usize = (0x22C9 - 0x2175) / INSTANCE_SIZE;
pub const FLAG_BASE: usize = INSTANCE_BASE;
pub const FLAG_LEN: usize = 0x22C9 - 0x2175;
pub const REL_BASE: usize = 0x22C9;
pub const REL_COUNT: usize = 50;
pub const REL_SIZE: usize = 3;
/// Dialogue/event trigger records (`DS:46AB`, file `0x1A4F`).
///
/// Each 6-byte record is `{ actor, relation, flags:u16, line:u16 }`. `actor`
/// and `line` are 1-based character and `HINT.RES` record indices. `flags`
/// bit 0x70 is the priority used to pick the best line; 0x2000 marks a plea
/// and 0x4000 an offer to join the party.
pub const EVENT_BASE: usize = 0x1A4F;
pub const EVENT_SIZE: usize = 6;
pub const EVENT_COUNT: usize = 80;
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
    /// Stat byte offsets verified from `1ba8:1005` and the status table.
    pub const HEALTH: usize = 0x00;
    pub const HITS: usize = 0x01;
    pub const MOVEMENT: usize = 0x02;
    pub const WEAPON: usize = 0x03;
    pub const COMBAT: usize = 0x04;
    pub const ARMOR: usize = 0x05;
    pub const NOBILITY: usize = 0x06;
    pub const FAITH: usize = 0x07;
    pub const MAGIC: usize = 0x08;
    pub const LEVEL: usize = 0x09;
    /// Party purse. The runtime reads the word at character `+0x28`, which maps
    /// to file record `+0x32` because the runtime slot is name-based.
    pub const GOLD: usize = 0x32;
    /// Runtime presence bits (`+0x16`, file `+0x20`). Bit 0x08 is "in scene".
    pub const PRESENCE: usize = 0x20;
    /// Maximum/starting army strength, verified from the battle records.
    pub const MAX_STRENGTH: usize = 0x26;
    /// Current army strength, reduced by battle casualties.
    pub const CURRENT_STRENGTH: usize = 0x28;
    /// Battle defence field (`name + 0x2B`) used by `FUN_20b5_1dff`.
    pub const BATTLE_DEFENCE: usize = 0x35;
    /// Battle hit points (`name + 0x2C`) used by `FUN_20b5_1dff`.
    pub const BATTLE_HP: usize = 0x36;
    /// Battle attack skill (`name + 0x2D`) used by `FUN_20b5_1dff`.
    pub const BATTLE_SKILL: usize = 0x37;

    #[must_use]
    pub fn name(&self) -> String {
        c_string(&self.raw[0x0A..0x16])
    }

    pub fn set_name(&mut self, name: &str) {
        self.raw[0x0A..0x16].fill(0);
        let bytes = name.as_bytes();
        let len = bytes.len().min(0x0B);
        self.raw[0x0A..0x0A + len].copy_from_slice(&bytes[..len]);
    }

    #[must_use]
    pub fn stat(&self, offset: usize) -> u8 {
        self.raw[offset]
    }

    pub fn set_stat(&mut self, offset: usize, value: u8) {
        self.raw[offset] = value;
    }

    #[must_use]
    pub fn health(&self) -> u8 {
        self.raw[Self::HEALTH]
    }

    #[must_use]
    pub fn hits(&self) -> u8 {
        self.raw[Self::HITS]
    }

    #[must_use]
    pub fn movement(&self) -> u8 {
        self.raw[Self::MOVEMENT]
    }

    #[must_use]
    pub fn combat(&self) -> u8 {
        self.raw[Self::COMBAT]
    }

    #[must_use]
    pub fn armor(&self) -> u8 {
        self.raw[Self::ARMOR]
    }

    #[must_use]
    pub fn nobility(&self) -> u8 {
        self.raw[Self::NOBILITY]
    }

    #[must_use]
    pub fn faith(&self) -> u8 {
        self.raw[Self::FAITH]
    }

    #[must_use]
    pub fn magic(&self) -> u8 {
        self.raw[Self::MAGIC]
    }

    #[must_use]
    pub fn level(&self) -> u8 {
        self.raw[Self::LEVEL]
    }

    #[must_use]
    pub fn gold(&self) -> u16 {
        u16::from_le_bytes([self.raw[Self::GOLD], self.raw[Self::GOLD + 1]])
    }

    pub fn set_gold(&mut self, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.raw[Self::GOLD] = lo;
        self.raw[Self::GOLD + 1] = hi;
    }

    /// Runtime character flags at `+0x16` (file `+0x20`).
    #[must_use]
    pub fn presence_flags(&self) -> u16 {
        u16::from_le_bytes([self.raw[Self::PRESENCE], self.raw[Self::PRESENCE + 1]])
    }

    pub fn set_presence_flags(&mut self, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.raw[Self::PRESENCE] = lo;
        self.raw[Self::PRESENCE + 1] = hi;
    }

    /// Maximum/starting army strength at `+0x26`.
    #[must_use]
    pub fn max_strength(&self) -> u16 {
        u16::from_le_bytes([
            self.raw[Self::MAX_STRENGTH],
            self.raw[Self::MAX_STRENGTH + 1],
        ])
    }

    /// Current army strength at `+0x28`.
    #[must_use]
    pub fn current_strength(&self) -> u16 {
        u16::from_le_bytes([
            self.raw[Self::CURRENT_STRENGTH],
            self.raw[Self::CURRENT_STRENGTH + 1],
        ])
    }

    pub fn set_current_strength(&mut self, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.raw[Self::CURRENT_STRENGTH] = lo;
        self.raw[Self::CURRENT_STRENGTH + 1] = hi;
    }

    /// Battle hit points at `+0x36` (`name + 0x2C`).
    #[must_use]
    pub fn battle_hp(&self) -> u8 {
        self.raw[Self::BATTLE_HP]
    }

    pub fn set_battle_hp(&mut self, value: u8) {
        self.raw[Self::BATTLE_HP] = value;
    }

    /// Battle defence at `+0x35` (`name + 0x2B`).
    #[must_use]
    pub fn battle_defence(&self) -> u8 {
        self.raw[Self::BATTLE_DEFENCE]
    }

    /// Battle attack skill at `+0x37` (`name + 0x2D`).
    #[must_use]
    pub fn battle_skill(&self) -> u8 {
        self.raw[Self::BATTLE_SKILL]
    }

    /// Current world X (`+0x2A` in the save record).
    #[must_use]
    pub fn world_x(&self) -> u16 {
        u16::from_le_bytes([self.raw[0x2A], self.raw[0x2B]])
    }

    /// Current world Y (`+0x2C` in the save record).
    #[must_use]
    pub fn world_y(&self) -> u16 {
        u16::from_le_bytes([self.raw[0x2C], self.raw[0x2D]])
    }

    /// Whether the original would include this record in the room list.
    #[must_use]
    pub fn is_present(&self) -> bool {
        self.presence_flags() & 0x8 != 0
    }
}

/// One event/dialogue trigger record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventRecord {
    pub raw: [u8; EVENT_SIZE],
}
impl EventRecord {
    /// 1-based character index being spoken to.
    #[must_use]
    pub fn actor(&self) -> u8 {
        self.raw[0]
    }
    /// Relationship/auxiliary byte (not used for line selection).
    #[must_use]
    pub fn relation(&self) -> u8 {
        self.raw[1]
    }
    #[must_use]
    pub fn flags(&self) -> u16 {
        u16::from_le_bytes([self.raw[2], self.raw[3]])
    }
    pub fn set_flags(&mut self, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.raw[2] = lo;
        self.raw[3] = hi;
    }
    /// 1-based `HINT.RES` record id holding the spoken line.
    #[must_use]
    pub fn line(&self) -> u16 {
        u16::from_le_bytes([self.raw[4], self.raw[5]])
    }
    #[must_use]
    pub fn priority(&self) -> u16 {
        self.flags() & 0x70
    }
    #[must_use]
    pub fn is_plea(&self) -> bool {
        self.flags() & 0x2000 != 0
    }
    #[must_use]
    pub fn offers_to_join(&self) -> bool {
        self.flags() & 0x4000 != 0
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

    /// Map icon resource id (`INTF#600..=615` in the shipped data).
    #[must_use]
    pub fn icon_id(&self) -> u16 {
        self.kind() & 0x0FFF
    }

    /// Legacy type-code scene guess.
    ///
    /// Travel now resolves scenes from the *location number* with
    /// [`crate::worldmap::resolve_scene_id`]; this method is kept only for
    /// callers that want the generic type mapping.
    #[must_use]
    pub fn scene_id(&self) -> Option<u16> {
        crate::worldmap::generic_scene_for_icon(self.icon_id())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub raw: [u8; ITEM_SIZE],
}
impl Item {
    /// Object definition layout verified from the `1ba8` object routines.
    pub const KIND: usize = 0x12;
    pub const SUBTYPE: usize = 0x13;
    pub const POWER: usize = 0x14;
    pub const CLASS: usize = 0x15;
    pub const EFFECT: usize = 0x16;
    pub const EFFECT_ID: usize = 0x17;
    pub const FLAGS: usize = 0x18;
    pub const COMPONENTS: usize = 0x1A;

    #[must_use]
    pub fn name(&self) -> String {
        c_string(&self.raw[0..0x12])
    }
    pub fn set_name(&mut self, name: &str) {
        self.raw[..0x12].fill(0);
        let bytes = name.as_bytes();
        let len = bytes.len().min(0x11);
        self.raw[..len].copy_from_slice(&bytes[..len]);
    }

    #[must_use]
    pub fn kind(&self) -> u8 {
        self.raw[Self::KIND]
    }
    #[must_use]
    pub fn subtype(&self) -> u8 {
        self.raw[Self::SUBTYPE]
    }
    #[must_use]
    pub fn power(&self) -> u8 {
        self.raw[Self::POWER]
    }
    #[must_use]
    pub fn item_class(&self) -> u8 {
        self.raw[Self::CLASS]
    }
    /// Magnitude of the effect (heal amount, armor value, ...).
    #[must_use]
    pub fn effect(&self) -> u8 {
        self.raw[Self::EFFECT]
    }
    /// Effect handler id. `44`/`4` heal, `36`/`46` defend, `64` charm.
    #[must_use]
    pub fn effect_id(&self) -> u8 {
        self.raw[Self::EFFECT_ID]
    }
    /// Shop value. The original uses byte `+0x16` (also the effect magnitude).
    #[must_use]
    pub fn price(&self) -> u16 {
        u16::from(self.raw[Self::EFFECT])
    }
    #[must_use]
    pub fn flags(&self) -> u16 {
        u16::from_le_bytes([self.raw[Self::FLAGS], self.raw[Self::FLAGS + 1]])
    }
    #[must_use]
    pub fn components(&self) -> u8 {
        self.raw[Self::COMPONENTS]
    }
    #[must_use]
    pub fn is_spell(&self) -> bool {
        self.kind() == 0 && self.subtype() == 0 && !self.name().is_empty()
    }
    #[must_use]
    pub fn is_gold(&self) -> bool {
        self.kind() == 49
    }
    /// Weapon/shield/helm/body/gauntlet kinds verified from `EPISODE1.DAT`.
    #[must_use]
    pub fn is_equipment(&self) -> bool {
        matches!(self.kind(), 55..=62)
    }
    /// Equipment slot; weapons and armour of the same slot replace each other.
    #[must_use]
    pub fn equipment_slot(&self) -> Option<u8> {
        match self.kind() {
            55..=57 => Some(0),
            58 => Some(1),
            59 => Some(2),
            60..=61 => Some(3),
            62 => Some(4),
            _ => None,
        }
    }
}

/// One live object placement (`DS:4DD1`, file `0x2175`, 4 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectInstance {
    pub raw: [u8; INSTANCE_SIZE],
}
impl ObjectInstance {
    pub const ITEM: usize = 0;
    pub const OWNER: usize = 1;
    pub const LOCATION: usize = 2;
    pub const FLAGS: usize = 3;

    #[must_use]
    pub fn item(&self) -> u8 {
        self.raw[Self::ITEM]
    }
    pub fn set_item(&mut self, value: u8) {
        self.raw[Self::ITEM] = value;
    }
    /// Character index (1-based) owning the object; zero for scenery/ground.
    #[must_use]
    pub fn owner(&self) -> u8 {
        self.raw[Self::OWNER]
    }
    pub fn set_owner(&mut self, value: u8) {
        self.raw[Self::OWNER] = value;
    }
    /// Scene id for normal objects; amount of money for the gold record.
    #[must_use]
    pub fn location(&self) -> u8 {
        self.raw[Self::LOCATION]
    }
    pub fn set_location(&mut self, value: u8) {
        self.raw[Self::LOCATION] = value;
    }
    #[must_use]
    pub fn flags(&self) -> u8 {
        self.raw[Self::FLAGS]
    }
    pub fn set_flags(&mut self, value: u8) {
        self.raw[Self::FLAGS] = value;
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.raw == [0; INSTANCE_SIZE]
    }
    #[must_use]
    pub fn is_carried(&self) -> bool {
        self.flags() & 0x18 == 0x18
    }
    #[must_use]
    pub fn is_equipped(&self) -> bool {
        self.is_carried() && self.flags() & 1 != 0
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
    /// All 49 object definitions (items and prepared spells).
    pub items: Vec<Item>,
    /// Read-only legacy view of definitions 36..=41. Serialisation uses
    /// `items`, so mutating this view alone does not change the save.
    pub spells: Vec<Item>,
    /// The 85 live object instances.
    pub instances: Vec<ObjectInstance>,
    /// The 80 dialogue/event triggers at `0x1A4F`.
    pub events: Vec<EventRecord>,
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
        let instances = (0..INSTANCE_COUNT)
            .map(|i| ObjectInstance {
                raw: array_at(data, INSTANCE_BASE + i * INSTANCE_SIZE),
            })
            .collect();
        let events = (0..EVENT_COUNT)
            .map(|i| EventRecord {
                raw: array_at(data, EVENT_BASE + i * EVENT_SIZE),
            })
            .collect();
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
            instances,
            events,
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
        for (i, instance) in self.instances.iter().enumerate() {
            let o = INSTANCE_BASE + i * INSTANCE_SIZE;
            data[o..o + INSTANCE_SIZE].copy_from_slice(&instance.raw);
        }
        for (i, event) in self.events.iter().enumerate() {
            let o = EVENT_BASE + i * EVENT_SIZE;
            data[o..o + EVENT_SIZE].copy_from_slice(&event.raw);
        }
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
        assert_eq!(state.locations[0].icon_id(), 605);
        assert_eq!(
            (state.locations[0].world_x(), state.locations[0].world_y()),
            (993, 475)
        );
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
