//! Map encounters.
//!
//! `fun_2920_0a12` is the collision callback the UI framework calls (through
//! `DS:067A`, installed by `FUN_2d65_2cd9`) when two map entities meet. It
//! decides whether nothing happens, the meeting is resolved without a dialog
//! (a failed random check or a fight started by `FUN_20b5_2c06`), or the
//! "X encounters Y. What shall X do?" dialog opens with Stop to talk / Join
//! Forces / Continue on buttons.
//!
//! Character fields are read from DGROUP at `DS:2C5E + 0x38 * id` (the name
//! pointer used by the original); offsets below are relative to that.

#![forbid(unsafe_code)]

const CHAR_BASE: usize = 0x2c5e;
const CHAR_SIZE: usize = 0x38;
const LOC_BASE: usize = 0x40b6;
const LOC_SIZE: usize = 0x19;

// Character fields, relative to the name pointer.
const FLAGS: usize = 0x16;
const ESCAPE: usize = 0x18;
const COUNT: usize = 0x1e;
const HOME_LO: usize = 0x20;
const HOME_HI: usize = 0x22;
const KIND: usize = 0x26;

/// What `FUN_15b8_16ca(0, 0xD3, id)` exposes of a map entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityView {
    pub x: i16,
    pub y: i16,
    /// `+0x0E`: moved this frame (set by the travel step).
    pub moved: u8,
}

/// Collaborators of the encounter check.
pub trait EncounterWorld {
    fn entity(&self, id: u8) -> Option<EntityView>;
    /// `FUN_15b8_17e0(n)`: random number in `0..n`.
    fn random(&mut self, n: u16) -> u16;
    /// `FUN_20b5_2c06`: start a fight between `DS:9DAE` / `DS:9DB0`;
    /// `true` when it took over.
    fn combat(&mut self, first: u8, second: u8) -> bool;
}

/// The encounter dialog `FUN_2920_0a12` opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncounterDialog {
    /// Subject and object after the swap (`DS:9DAE`, `DS:9DB0`).
    pub first: u8,
    pub second: u8,
    /// "Stop to talk to <second>" button.
    pub talk: bool,
    /// "Join Forces" button.
    pub join: bool,
    /// The dialog offers choices ("What shall %s do?" and "Continue on");
    /// without it the meeting is resolved by `FUN_2920_0a00` at once.
    pub choice: bool,
    /// Window height passed to `FUN_1772_000e` (`[BP-0x48]`).
    pub height: u16,
    /// Index into the subject prefix table at `DS:79E0`.
    pub prefix: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncounterOutcome {
    /// Returns 0: the entities pass each other.
    Ignore,
    /// Returns 1 without a dialog.
    Handled,
    /// Returns 1 after opening the dialog.
    Dialog(EncounterDialog),
}

fn word(ds: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([ds[at], ds[at + 1]])
}

fn char_at(id: u8, field: usize) -> usize {
    CHAR_BASE + usize::from(id) * CHAR_SIZE + field
}

/// `FUN_2d65_09d4`: index of the location at exactly `(x, y)`, else 0.
#[must_use]
pub fn location_at(ds: &[u8], x: i16, y: i16) -> u8 {
    for i in 0..=0x3cu8 {
        let at = LOC_BASE + usize::from(i) * LOC_SIZE;
        if word(ds, at + 0x12) == x as u16 && word(ds, at + 0x14) == y as u16 {
            return i;
        }
    }
    0
}

/// `FUN_2920_0a12(first, second)`. `ds` is the DGROUP image; `clock` the
/// 32-bit game clock at `DS:4FBF`; `line` the font height `DS:1E8A`.
pub fn fun_2920_0a12(
    ds: &[u8],
    clock: u32,
    first: u8,
    second: u8,
    world: &mut impl EncounterWorld,
) -> EncounterOutcome {
    let line = u16::from(ds[0x1e8a]);
    let mut a = first;
    let mut b = second;
    let mut fa = word(ds, char_at(a, FLAGS));
    let mut fb = word(ds, char_at(b, FLAGS));
    let ea = world.entity(a);
    let eb = world.entity(b);

    if clock < 0xa00 {
        return EncounterOutcome::Ignore;
    }
    if fa & 0x400 != 0 && fb & 0x400 != 0 {
        return EncounterOutcome::Ignore;
    }
    if fa & 0x700 == 0 && fb & 0x700 == 0 {
        if ds[char_at(a, KIND)] == 4 {
            fa |= 0x200;
        } else if ds[char_at(b, KIND)] == 4 {
            fb |= 0x200;
        } else {
            return EncounterOutcome::Ignore;
        }
    }
    if fb & 0x100 != 0 {
        if fa & 0x200 == 0 {
            return EncounterOutcome::Ignore;
        }
        let roll = world.random(100);
        if roll > u16::from(ds[char_at(b, ESCAPE)]) {
            return EncounterOutcome::Handled;
        }
    }
    if let Some(e) = ea {
        if fa & 0x200 != 0 && fb & 0x200 != 0 {
            // 32-bit squared distance from (0x2D2, 0x2B0).
            let dx = i32::from(e.x) - 0x2d2;
            let dy = i32::from(e.y) - 0x2b0;
            let d = (dx.wrapping_mul(dx) as u32).wrapping_add(dy.wrapping_mul(dy) as u32) as i32;
            if d < 0x240 {
                return EncounterOutcome::Ignore;
            }
        }
    }
    let side_ok = |f: u16| matches!(f & 0x7000, 0 | 0x6000 | 0x7000);
    if !side_ok(fa) || !side_ok(fb) {
        return EncounterOutcome::Ignore;
    }
    // The original dereferences both entity pointers here without a null
    // check; a missing entity reads 0000:0010 (interrupt table), which never
    // matches a location, so it is treated as "no location".
    for (pos, f) in [(ea, fa), (eb, fb)] {
        let loc = pos.map_or(0, |p| location_at(ds, p.x, p.y));
        if loc != 0
            && word(ds, LOC_BASE + usize::from(loc) * LOC_SIZE + 0x16) & 0x8000 != 0
            && f & 0x200 == 0
        {
            return EncounterOutcome::Ignore;
        }
    }
    if world.combat(a, b) {
        return EncounterOutcome::Handled;
    }
    if fa & 0x8000 != 0 && fb & 0x8000 != 0 {
        return EncounterOutcome::Ignore;
    }
    if fa & 0x200 == 0 && fb & 0x200 == 0 {
        return EncounterOutcome::Ignore;
    }
    let (Some(pa), Some(pb)) = (world.entity(a), world.entity(b)) else {
        return EncounterOutcome::Ignore;
    };
    // 2920:0ce1: the party side (0x200) speaks first; otherwise whoever was
    // standing still is the one being met.
    let swap = (fa & 0x200 == 0 && fb & 0x200 != 0) || (pa.moved == 0 && pb.moved == 1);
    if swap {
        std::mem::swap(&mut a, &mut b);
        std::mem::swap(&mut fa, &mut fb);
    }

    let mut height = line + 4;
    let mut talk = false;
    let mut join = false;
    let mut choice = false;
    let plain = fb & 0x700 == 0
        && word(ds, char_at(b, HOME_HI)) == 0x2be
        && word(ds, char_at(b, HOME_LO)) == 0;
    if !plain {
        height += line;
        if fa & 0x8000 == 0 && fb & 0x8000 == 0 {
            talk = true;
            height += line + 6;
        }
        if fa & 0x200 != 0 && fb & 0x200 != 0 {
            join = true;
            height += line + 6;
        }
        height += line + 0xc;
        choice = true;
    }
    let plural = word(ds, char_at(a, COUNT)) as i16 > 1;
    let prefix = usize::from(fa & 0x800 != 0) | (usize::from(plural) << 1);
    EncounterOutcome::Dialog(EncounterDialog {
        first: a,
        second: b,
        talk,
        join,
        choice,
        height,
        prefix,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct Replay {
        views: HashMap<u8, Option<EntityView>>,
        rng: Vec<u16>,
        combat: Option<bool>,
        combat_called: bool,
    }

    impl EncounterWorld for Replay {
        fn entity(&self, id: u8) -> Option<EntityView> {
            self.views.get(&id).copied().flatten()
        }
        fn random(&mut self, _n: u16) -> u16 {
            assert!(!self.rng.is_empty(), "port drew a random number the original did not");
            self.rng.remove(0)
        }
        fn combat(&mut self, _a: u8, _b: u8) -> bool {
            self.combat_called = true;
            self.combat.expect("port called FUN_20b5_2c06 but the original did not")
        }
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    fn view(v: &serde_json::Value) -> Option<EntityView> {
        let a = v.as_array()?;
        Some(EntityView {
            x: a[0].as_i64().unwrap() as i16,
            y: a[1].as_i64().unwrap() as i16,
            moved: a[2].as_u64().unwrap() as u8,
        })
    }

    /// Replays calls recorded from the original by
    /// `tests/oracle/encounter_vectors.py`.
    #[test]
    fn matches_original_encounter_checks() {
        let Ok(text) = std::fs::read_to_string("../../tests/vectors/encounter.json") else {
            eprintln!("SKIP: tests/vectors/encounter.json absent");
            return;
        };
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let mut counts = [0usize; 3];
        for (n, call) in v["calls"].as_array().unwrap().iter().enumerate() {
            let mut ds = vec![0u8; 0x10000];
            ds[0x1e8a] = call["line"].as_u64().unwrap() as u8;
            let locs = hex(call["locations"].as_str().unwrap());
            ds[LOC_BASE..LOC_BASE + locs.len()].copy_from_slice(&locs);
            for (id, raw) in call["chars"].as_object().unwrap() {
                let at = CHAR_BASE + id.parse::<usize>().unwrap() * CHAR_SIZE;
                let raw = hex(raw.as_str().unwrap());
                ds[at..at + raw.len()].copy_from_slice(&raw);
            }
            let a = call["a"].as_u64().unwrap() as u8;
            let b = call["b"].as_u64().unwrap() as u8;
            let views = &call["views"];
            let mut world = Replay {
                views: HashMap::from([(a, view(&views["a"])), (b, view(&views["b"]))]),
                rng: call["rng"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|r| r.as_u64().unwrap() as u16)
                    .collect(),
                combat: call["combat"].as_u64().map(|c| c != 0),
                combat_called: false,
            };
            let clock = call["clock"].as_u64().unwrap() as u32;
            let out = fun_2920_0a12(&ds, clock, a, b, &mut world);
            let ret = call["ret"].as_u64().unwrap();
            assert_eq!(world.combat_called, !call["combat"].is_null(), "call {n} ({a},{b}): combat call");
            assert!(world.rng.is_empty(), "call {n}: unused random numbers");
            match out {
                EncounterOutcome::Ignore => assert_eq!(ret, 0, "call {n} ({a},{b})"),
                EncounterOutcome::Handled => {
                    assert_eq!(ret, 1, "call {n} ({a},{b})");
                    assert!(call.get("height").is_none(), "call {n}: original opened a dialog");
                }
                EncounterOutcome::Dialog(d) => {
                    assert_eq!(ret, 1, "call {n} ({a},{b})");
                    let buttons: Vec<&str> = call["buttons"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|b| b.as_str().unwrap())
                        .collect();
                    let mut expect = Vec::new();
                    if d.talk {
                        expect.push("talk");
                    }
                    if d.join {
                        expect.push("join");
                    }
                    if d.choice {
                        expect.push("continue");
                    }
                    assert_eq!(buttons, expect, "call {n} ({a},{b}): buttons");
                    assert_eq!(u64::from(d.height), call["height"].as_u64().unwrap(), "call {n}: height");
                    assert_eq!(u64::from(d.choice), call["choice"].as_u64().unwrap(), "call {n}: choice");
                    let fin = call["final"].as_array().unwrap();
                    assert_eq!(
                        (u64::from(d.first), u64::from(d.second)),
                        (fin[0].as_u64().unwrap(), fin[1].as_u64().unwrap()),
                        "call {n}: subject/object"
                    );
                }
            }
            counts[match out {
                EncounterOutcome::Ignore => 0,
                EncounterOutcome::Handled => 1,
                EncounterOutcome::Dialog(_) => 2,
            }] += 1;
        }
        eprintln!("encounter checks matched: ignore={} handled={} dialog={}", counts[0], counts[1], counts[2]);
        assert!(counts.iter().sum::<usize>() > 0);
    }
}
