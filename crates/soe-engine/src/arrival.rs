//! Map arrival.
//!
//! `fun_2920_026c` is the arrival callback (`DS:0672`) the travel step raises
//! when an entity reaches its target. It advances the character's route
//! (`fun_2920_001d`), reports "%S%sreached %S." at named locations (or
//! "...reached the location you selected." for the party elsewhere), and lets
//! an army (flag `0x400`) put a location to the torch when its strength is
//! high enough. Burning Camelot loses the game.
//!
//! Everything works on the DGROUP image and the live map entities; the
//! original's message windows become returned strings, and its modal wait
//! (`FUN_15b8_09f7`) is left to the caller.

#![forbid(unsafe_code)]

use crate::encounter::location_at;
use crate::names::display_name;
use crate::travel::MapEntity;

const CHAR_NAME: usize = 0x2c5e;
const CHAR_SIZE: usize = 0x38;
const LOC_BASE: usize = 0x40b6;
const LOC_SIZE: usize = 0x19;
const CHARACTERS: u8 = 0x5c;
/// `DS:4FCE..4FD1`: the last four burnt locations, newest first.
const BURNT: usize = 0x4fce;
/// Location field holding the map icon (and burn state).
const ICON: usize = 0x16;
/// Icon of Camelot; burning it ends the game.
const CAMELOT: u16 = 0x25f;

fn name_at(id: u8) -> usize {
    CHAR_NAME + usize::from(id) * CHAR_SIZE
}

fn loc_at(loc: u8) -> usize {
    LOC_BASE + usize::from(loc) * LOC_SIZE
}

fn word(ds: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([ds[at], ds[at + 1]])
}

fn set_word(ds: &mut [u8], at: usize, v: u16) {
    ds[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

/// `FUN_2a6e_07c5`: a location's name (after the first `_`, if any).
#[must_use]
pub fn location_name(ds: &[u8], loc: u8) -> String {
    let at = loc_at(loc);
    let end = ds[at..].iter().position(|&b| b == 0).map_or(ds.len(), |n| at + n);
    let raw = &ds[at..end];
    let raw = raw.iter().position(|&b| b == b'_').map_or(raw, |i| &raw[i + 1..]);
    raw.iter().map(|&b| char::from(b)).collect()
}

/// `FUN_15b8_174d(count)`.
fn has_have(count: u16) -> &'static str {
    if count > 1 { " have " } else { " has " }
}

/// Result of an arrival.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArrivalOutcome {
    /// Messages in the order the original shows them (each waits for a
    /// click on the original).
    pub messages: Vec<String>,
    /// Camelot fell (`DS:01EC = -4`).
    pub game_lost: bool,
}

/// `FUN_2920_001d(id)`: take the next stop from the character's route
/// (`name+0x32..0x37`) and make it the entity's target. A stop with bit 7
/// set follows that character; otherwise it is a location index.
pub fn fun_2920_001d(ds: &mut [u8], entities: &mut [MapEntity], id: u8) {
    let n = name_at(id);
    let head = ds[n + 0x32];
    if head == 0 {
        return;
    }
    for k in 1..6 {
        ds[n + 0x31 + k] = ds[n + 0x32 + k];
    }
    ds[n + 0x37] = 0;
    let me = entities.iter().position(|e| e.id() == u16::from(id));
    let (tx, ty) = if head & 0x80 != 0 {
        let other = head & 0x7f;
        ds[n + 0x18] = other;
        match me {
            Some(i) => {
                entities[i].0[0x27] = other;
                entities
                    .iter()
                    .find(|e| e.id() == u16::from(other))
                    .map_or((0, 0), |e| (e.x() as u16, e.y() as u16))
            }
            None => (word(ds, name_at(other) + 0x0e), word(ds, name_at(other) + 0x10)),
        }
    } else {
        ds[n + 0x18] = 0;
        if let Some(i) = me {
            entities[i].0[0x27] = 0;
        }
        (word(ds, loc_at(head) + 0x12), word(ds, loc_at(head) + 0x14))
    };
    match me {
        Some(i) => {
            let e = &mut entities[i].0;
            e[0x1e..0x20].copy_from_slice(&tx.to_le_bytes());
            e[0x1c..0x1e].copy_from_slice(&ty.to_le_bytes());
            let (x, y) = ([e[0x12], e[0x13]], [e[0x10], e[0x11]]);
            e[0x22..0x24].copy_from_slice(&x);
            e[0x20..0x22].copy_from_slice(&y);
            e[0x28..0x34].fill(0);
        }
        None => {
            set_word(ds, n + 0x12, tx);
            set_word(ds, n + 0x14, ty);
        }
    }
}

/// `FUN_2920_026c`: the entity at `index` has reached its target.
pub fn fun_2920_026c(ds: &mut [u8], entities: &mut [MapEntity], index: usize) -> ArrivalOutcome {
    let mut out = ArrivalOutcome::default();
    let id = entities[index].id() as u8;
    let loc = location_at(ds, entities[index].x(), entities[index].y());
    let n = name_at(id);
    let was_following = ds[n + 0x18] | entities[index].0[0x27];
    ds[n + 0x18] = 0;
    entities[index].0[0x27] = 0;
    fun_2920_001d(ds, entities, id);

    let flags = word(ds, n + 0x16);
    if loc == 0 {
        if flags & 0x200 != 0 && was_following == 0 {
            out.messages.push(format!(
                "{}{}reached the\nlocation you selected.",
                display_name(ds, id),
                has_have(word(ds, n + 0x1e))
            ));
        }
        return out;
    }
    if ds[loc_at(loc)] != 0 {
        out.messages.push(format!(
            "{}{}reached {}.",
            display_name(ds, id),
            has_have(word(ds, n + 0x1e)),
            location_name(ds, loc)
        ));
    }
    if flags & 0x400 == 0 {
        return out;
    }

    // Army strength: this character plus everyone on the map it leads.
    let mut strength = word(ds, n + 0x1e);
    for c in 0..=CHARACTERS {
        let cn = name_at(c);
        if word(ds, cn + 0x16) & 0x08 != 0 && ds[cn + 0x31] == id {
            strength = strength.wrapping_add(word(ds, cn + 0x1e));
        }
    }
    let icon = word(ds, loc_at(loc) + ICON);
    // CS:043C jump table: icon - 600 -> (burnt icon, strength needed).
    let (burnt, needed) = match icon.wrapping_sub(600) {
        0 => (0x25e, 0x4b),
        1 => (0x263, 0x96),
        3 => (0x25e, 0x1e),
        4 => (0x264, 0x1f4),
        5 => (0x265, 0x14),
        7 => (0x263, 0x01),
        _ => (0, 0),
    };
    if burnt == 0 || strength < needed {
        return out;
    }
    out.messages.push(format!("{} is put to the torch.", location_name(ds, loc)));
    if icon == CAMELOT {
        out.game_lost = true;
        return out;
    }
    set_word(
        ds,
        loc_at(loc) + ICON,
        (icon.wrapping_sub(600) << 12) | 0x8000 | burnt,
    );
    // The oldest of the last four burnt places is rebuilt.
    let oldest = ds[BURNT + 3];
    if oldest != 0 {
        let at = loc_at(oldest) + ICON;
        let restored = ((word(ds, at) >> 12) & 7) + 600;
        set_word(ds, at, restored);
    }
    ds[BURNT + 3] = ds[BURNT + 2];
    ds[BURNT + 2] = ds[BURNT + 1];
    ds[BURNT + 1] = ds[BURNT];
    ds[BURNT] = loc;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    fn overlay(ds: &mut [u8], regions: &serde_json::Value) {
        for (addr, bytes) in regions.as_object().unwrap() {
            let at = usize::from_str_radix(addr, 16).unwrap();
            let bytes = hex(bytes.as_str().unwrap());
            ds[at..at + bytes.len()].copy_from_slice(&bytes);
        }
    }

    fn entities(v: &serde_json::Value) -> Vec<MapEntity> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|e| MapEntity(hex(e.as_str().unwrap()).try_into().unwrap()))
            .collect()
    }

    /// The original's message: its format with `%S`/`%s` filled in order.
    fn render(message: &serde_json::Value) -> String {
        let mut args = message["args"].as_array().unwrap().iter();
        let format = message["format"].as_str().unwrap();
        let mut out = String::new();
        let mut rest = format;
        while let Some(i) = rest.find('%') {
            out.push_str(&rest[..i]);
            out.push_str(args.next().and_then(|a| a.as_str()).unwrap_or(""));
            rest = &rest[i + 2..];
        }
        out + rest
    }

    /// Replays arrivals produced by the original (`tests/oracle/arrival_vectors.py`).
    #[test]
    fn matches_original_arrivals() {
        let Ok(text) = std::fs::read_to_string("../../tests/vectors/arrival.json") else {
            eprintln!("SKIP: tests/vectors/arrival.json absent");
            return;
        };
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let base = hex(v["dgroup"].as_str().unwrap());
        let mut messages = 0;
        for (n, call) in v["calls"].as_array().unwrap().iter().enumerate() {
            let mut ds = base.clone();
            overlay(&mut ds, &call["before"]["regions"]);
            let mut ents = entities(&call["before"]["entities"]);
            let index = call["entity"].as_u64().unwrap() as usize;
            let out = fun_2920_026c(&mut ds, &mut ents, index);

            let mut want = base.clone();
            overlay(&mut want, &call["after"]["regions"]);
            let lost = u16::from_le_bytes([want[0x1ec], want[0x1ed]]) == 0xfffc;
            assert_eq!(out.game_lost, lost, "call {n}: game lost");
            want[0x1ec..0x1ee].copy_from_slice(&ds[0x1ec..0x1ee]);
            for (addr, bytes) in call["after"]["regions"].as_object().unwrap() {
                let at = usize::from_str_radix(addr, 16).unwrap();
                let len = bytes.as_str().unwrap().len() / 2;
                assert_eq!(&ds[at..at + len], &want[at..at + len], "call {n}: DS:{addr}");
            }
            assert_eq!(ents, entities(&call["after"]["entities"]), "call {n}: entities");
            let expect: Vec<String> = call["messages"].as_array().unwrap().iter().map(render).collect();
            assert_eq!(out.messages, expect, "call {n}: messages");
            messages += expect.len();
        }
        eprintln!(
            "{} original arrivals matched ({messages} messages)",
            v["calls"].as_array().unwrap().len()
        );
    }
}
