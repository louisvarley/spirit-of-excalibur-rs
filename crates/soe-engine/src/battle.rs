//! Battle unit records.
//!
//! Live army records are 0x38 bytes:
//! - `0x0A..0x15`: NUL-padded name
//! - `0x00..0x02`: three combat/armour fields (exact names still open)
//! - `0x26`: current strength
//! - `0x28`: maximum/starting strength
//! - `0x2A`: primary battle sprite image id
//! - `0x2C`: secondary image id
//!
//! The battle UI ("Round Table"/"Foes", defender/challenger) reads these
//! records through `DAT_3364_9dc2`/`9dd6`. Round resolution is not yet ported.

#![forbid(unsafe_code)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CombatFields {
    pub armour: u8,
    pub attack: u8,
    pub defence: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BattleUnit {
    pub name: String,
    pub stats: CombatFields,
    pub strength: u16,
    pub max_strength: u16,
    pub image: u16,
    pub icon: u16,
}

impl BattleUnit {
    /// Parse a live 0x38-byte army record.
    #[must_use]
    pub fn from_record(record: &[u8]) -> Option<Self> {
        if record.len() < 0x38 {
            return None;
        }
        let name_bytes = &record[0x0a..0x15];
        let end = name_bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(name_bytes.len());
        if end == 0 {
            return None;
        }
        Some(Self {
            name: String::from_utf8_lossy(&name_bytes[..end]).into_owned(),
            stats: CombatFields {
                armour: record[0],
                attack: record[1],
                defence: record[2],
            },
            strength: u16::from_le_bytes([record[0x26], record[0x27]]),
            max_strength: u16::from_le_bytes([record[0x28], record[0x29]]),
            image: u16::from_le_bytes([record[0x2a], record[0x2b]]),
            icon: u16::from_le_bytes([record[0x2c], record[0x2d]]),
        })
    }

    #[must_use]
    pub fn is_routed(&self) -> bool {
        self.strength == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_live_record_shape() {
        let mut record = [0u8; 0x38];
        record[0x00] = 40;
        record[0x01] = 5;
        record[0x02] = 0;
        record[0x0a..0x11].copy_from_slice(b"knight\0");
        record[0x26..0x28].copy_from_slice(&100u16.to_le_bytes());
        record[0x28..0x2a].copy_from_slice(&100u16.to_le_bytes());
        record[0x2a..0x2c].copy_from_slice(&677u16.to_le_bytes());
        record[0x2c..0x2e].copy_from_slice(&700u16.to_le_bytes());
        let unit = BattleUnit::from_record(&record).unwrap();
        assert_eq!(unit.name, "knight");
        assert_eq!(unit.stats.armour, 40);
        assert_eq!((unit.strength, unit.max_strength), (100, 100));
        assert_eq!((unit.image, unit.icon), (677, 700));
        assert!(!unit.is_routed());
    }
}
