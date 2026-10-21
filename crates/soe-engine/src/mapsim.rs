//! Regional map simulation: entities, clock, collisions.
//!
//! Mirrors the map part of the original UI loop (`FUN_15b8_09f7`):
//! every map entity (widget type `0xD3`) takes a travel step
//! (`travel::fun_1b06_0007`), then the 32-bit clock at `DS:4FBF` advances by
//! `1 << DS:4FC4` and the collision scan (`FUN_1b06_079c`) runs the encounter
//! check (`encounter::fun_2920_0a12`) on entities that touch.
//!
//! State lives in a DGROUP image: the EXE's initialised data with the save
//! file laid over it (`file 0..0x235F -> DS 0x2C5C`, tail -> `DS 0x4FBF`), so
//! the ported functions read exactly the fields the original reads.
//!
//! Not ported yet: `FUN_20b5_2c06` (fights started by a collision; always
//! "no fight" here) and the arrival callback `2920:026C` (the runtime handles
//! the party's arrival itself).

#![forbid(unsafe_code)]

use crate::encounter::{self, EncounterDialog, EncounterOutcome, EncounterWorld, EntityView};
use crate::fight::{self, FightSetup};
use crate::travel::{self, ENTITY_BYTES, MapEntity, TravelScratch, TravelTables, TravelWorld};

const DS_MAIN: usize = 0x2c5c;
const DS_TAIL: usize = 0x4fbf;
const SAVE_MAIN_LEN: usize = 0x235f;
const SAVE_TAIL: usize = 0x235f;
const SAVE_TAIL_LEN: usize = 22;
const CHAR_NAME: usize = 0x2c5e;
const CHAR_SIZE: usize = 0x38;
const CHARACTERS: u8 = 0x5c;
const CLOCK: usize = 0x4fbf;
const SHIFT: usize = 0x4fc4;
const RADIUS: usize = 0x067e;
const COOLDOWN_SLOTS: usize = 0x32;

/// DGROUP image from a complete MZ `EXCAL_UNPACKED.EXE` (64 KiB, zero padded).
#[must_use]
pub fn dgroup_from_exe(exe: &[u8]) -> Option<Vec<u8>> {
    if exe.len() < 0x1c || &exe[..2] != b"MZ" {
        return None;
    }
    let payload = usize::from(u16::from_le_bytes([exe[8], exe[9]])) * 16;
    let start = payload + crate::palette::DGROUP_OFFSET;
    let mut ds = vec![0u8; 0x10000];
    let src = exe.get(start..)?;
    let n = src.len().min(ds.len());
    ds[..n].copy_from_slice(&src[..n]);
    Some(ds)
}

/// Something that happened during a round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapEvent {
    /// The entity for character `id` reached its target at `location`
    /// (`FUN_2d65_09d4`, 0 when not on a location); the arrival callback
    /// (`arrival::fun_2920_026c`) produced `messages` and maybe lost the game.
    Arrived {
        id: u8,
        location: u8,
        messages: Vec<String>,
        game_lost: bool,
    },
    /// An encounter dialog opened.
    Encounter(EncounterDialog),
    /// `FUN_20b5_2c06` started a confrontation (challenge, ambush, army
    /// sighting, contest).
    Fight(FightSetup),
}

pub struct MapSim {
    ds: Vec<u8>,
    tables: TravelTables,
    tiles: Vec<u8>,
    tile_width: u16,
    /// Entities in widget order (ascending character id).
    entities: Vec<MapEntity>,
    scratch: TravelScratch,
    /// `DS:9AAD` / `DS:9ADF` / `DS:9B3B`: recent pairs and expiry.
    cooldown: [(u8, u8, u16); COOLDOWN_SLOTS],
    rng: u32,
    /// Set by the encounter check's fight hook.
    pending_fight: Option<FightSetup>,
}

fn word(ds: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([ds[at], ds[at + 1]])
}

fn c_string(ds: &[u8], at: usize) -> String {
    let end = ds[at..].iter().position(|&b| b == 0).map_or(ds.len(), |n| at + n);
    ds[at..end].iter().map(|&b| char::from(b)).collect()
}

fn name(id: u8) -> usize {
    CHAR_NAME + usize::from(id) * CHAR_SIZE
}

impl MapSim {
    /// Build the map like `FUN_2d65_0d39`: one entity per character with
    /// flag `0x08` and a non-zero x, created by `FUN_15b8_0793`.
    #[must_use]
    pub fn new(mut ds: Vec<u8>, save: &[u8], tiles: &[u8], tile_width: u16) -> Option<Self> {
        ds.get_mut(DS_MAIN..DS_MAIN + SAVE_MAIN_LEN)?
            .copy_from_slice(save.get(..SAVE_MAIN_LEN)?);
        ds.get_mut(DS_TAIL..DS_TAIL + SAVE_TAIL_LEN)?
            .copy_from_slice(save.get(SAVE_TAIL..SAVE_TAIL + SAVE_TAIL_LEN)?);
        let tables = TravelTables::from_dgroup(&ds)?;
        let mut entities = Vec::new();
        // FUN_2d65_0d39 creates them from 0x5C down to 1; the widget list
        // ends up in ascending id order (observed in the original).
        for id in 1..=CHARACTERS {
            let n = name(id);
            let flags = word(&ds, n + 0x16);
            let x = word(&ds, n + 0x0e);
            if flags & 0x08 == 0 || x == 0 {
                continue;
            }
            let mut y = word(&ds, n + 0x10);
            if y == 0xd3 {
                y = 0xd4; // FUN_15b8_0793 avoids the widget-type value
            }
            let mut raw = [0u8; ENTITY_BYTES];
            let mut put = |at: usize, v: u16| raw[at..at + 2].copy_from_slice(&v.to_le_bytes());
            put(0x0a, u16::from(id));
            put(0x12, x);
            put(0x10, y);
            put(0x1e, word(&ds, n + 0x12));
            put(0x1c, word(&ds, n + 0x14));
            put(0x22, x);
            put(0x20, y);
            put(0x34, word(&ds, n + 0x1a));
            raw[0x0f] = 0xd3;
            let hidden = (flags >> 8) & 1;
            raw[0x26] = ((hidden << 7) | (((flags >> 4) & 3) << 4) | (flags & 7)) as u8;
            raw[0x27] = if hidden != 0 { 0 } else { ds[n + 0x18] };
            entities.push(MapEntity(raw));
        }
        Some(Self {
            ds,
            tables,
            tiles: tiles.to_vec(),
            tile_width,
            entities,
            scratch: TravelScratch::default(),
            cooldown: [(0, 0, 0xffff); COOLDOWN_SLOTS],
            rng: 0x1234_5678,
            pending_fight: None,
        })
    }

    #[must_use]
    pub fn clock(&self) -> u32 {
        u32::from(word(&self.ds, CLOCK)) | u32::from(word(&self.ds, CLOCK + 2)) << 16
    }

    fn set_clock(&mut self, v: u32) {
        self.ds[CLOCK..CLOCK + 4].copy_from_slice(&v.to_le_bytes());
    }

    /// The live DGROUP image (save state plus EXE data).
    #[must_use]
    pub fn ds(&self) -> &[u8] {
        &self.ds
    }

    #[must_use]
    pub fn entities(&self) -> &[MapEntity] {
        &self.entities
    }

    /// Map icon resource of character `id` (`name+0x20`, the first argument
    /// `FUN_2d65_0d39` passes to `FUN_15b8_0793`).
    #[must_use]
    pub fn icon(&self, id: u8) -> u16 {
        word(&self.ds, name(id) + 0x20)
    }

    #[must_use]
    pub fn entity(&self, id: u8) -> Option<&MapEntity> {
        self.entities.iter().find(|e| e.id() == u16::from(id))
    }

    /// First party-side entity (character flag `0x200`).
    #[must_use]
    pub fn party(&self) -> Option<u8> {
        self.entities
            .iter()
            .map(|e| e.id() as u8)
            .find(|&id| word(&self.ds, name(id) + 0x16) & 0x200 != 0)
    }

    /// Send character `id` towards `(x, y)`, as a map click does.
    pub fn set_target(&mut self, id: u8, x: i16, y: i16) -> bool {
        let Some(e) = self.entities.iter_mut().find(|e| e.id() == u16::from(id)) else {
            return false;
        };
        e.0[0x1e..0x20].copy_from_slice(&(x as u16).to_le_bytes());
        e.0[0x1c..0x1e].copy_from_slice(&(y as u16).to_le_bytes());
        true
    }

    #[must_use]
    pub fn moving(&self, id: u8) -> bool {
        self.entity(id).is_some_and(|e| e.target().0 != 0)
    }

    /// One pass of the original loop over all map entities.
    pub fn round(&mut self) -> Vec<MapEvent> {
        let mut events = Vec::new();
        let clock = self.clock();
        for i in 0..self.entities.len() {
            let mut e = self.entities[i].clone();
            // The scratch globals are shared by all entities, as on the original.
            let mut scratch = self.scratch;
            let ev = {
                let world = World { sim: self };
                travel::fun_1b06_0007(&mut e, &self.tables, &mut scratch, &world, clock, true)
            };
            self.scratch = scratch;
            self.entities[i] = e;
            if ev.arrived {
                let id = self.entities[i].id() as u8;
                let location =
                    encounter::location_at(&self.ds, self.entities[i].x(), self.entities[i].y());
                // The callback runs inside the round, before later entities step.
                let outcome = crate::arrival::fun_2920_026c(&mut self.ds, &mut self.entities, i);
                events.push(MapEvent::Arrived {
                    id,
                    location,
                    messages: outcome.messages,
                    game_lost: outcome.game_lost,
                });
            }
        }
        let shift = self.ds[SHIFT] & 0x1f;
        self.set_clock(clock.wrapping_add(1u32.checked_shl(u32::from(shift)).unwrap_or(0)));
        self.sync_characters();
        if let Some(event) = self.collisions() {
            events.push(event);
        }
        events
    }

    /// Copy entity positions and targets back into the character records.
    fn sync_characters(&mut self) {
        for e in &self.entities {
            let n = name(e.id() as u8);
            let (tx, ty) = e.target();
            for (at, v) in [(0x0e, e.x()), (0x10, e.y()), (0x12, tx), (0x14, ty)] {
                self.ds[n + at..n + at + 2].copy_from_slice(&(v as u16).to_le_bytes());
            }
        }
    }

    /// `FUN_1b06_079c`: find the first touching pair and run the encounter
    /// check on it.
    fn collisions(&mut self) -> Option<MapEvent> {
        let now = (self.clock() >> 8) as u16;
        let mut pairs = Vec::new();
        for i in 0..self.entities.len() {
            let a = &self.entities[i];
            if a.0[0x26] & 0x80 != 0 || a.x() == 0 {
                continue;
            }
            for b in &self.entities[i + 1..] {
                if b.x() == 0 || (a.0[0x0e] != 1 && b.0[0x0e] != 1) {
                    continue;
                }
                let class = ((a.0[0x26] >> 4) & 3).max((b.0[0x26] >> 4) & 3);
                let mut radius = u16::from(self.ds[RADIUS + usize::from(class)]);
                if u16::from(a.0[0x27]) == b.id() || u16::from(b.0[0x27]) == a.id() {
                    radius += 2;
                }
                let dx = a.x().wrapping_sub(b.x()).wrapping_abs() as u16;
                let dy = a.y().wrapping_sub(b.y()).wrapping_abs() as u16;
                if dx >= radius || dy >= radius {
                    continue;
                }
                let (ia, ib) = (a.id() as u8, b.id() as u8);
                if self
                    .cooldown
                    .iter()
                    .any(|&(p, q, _)| (p, q) == (ia, ib) || (p, q) == (ib, ia))
                {
                    continue;
                }
                pairs.push((ia, ib));
            }
        }
        for (ia, ib) in pairs {
            let clock = self.clock();
            let ds = self.ds.clone();
            match encounter::fun_2920_0a12(&ds, clock, ia, ib, self) {
                EncounterOutcome::Ignore => {}
                outcome => {
                    // FUN_1b06_06b5(a, b, 10): remember the pair for a while.
                    if let Some(slot) = self.cooldown.iter_mut().find(|s| s.0 == 0) {
                        *slot = (ia, ib, now.wrapping_add(10));
                    }
                    return match outcome {
                        EncounterOutcome::Dialog(d) => Some(MapEvent::Encounter(d)),
                        _ => self.pending_fight.take().map(MapEvent::Fight),
                    };
                }
            }
        }
        for slot in &mut self.cooldown {
            if now > slot.2 {
                *slot = (0, 0, 0xffff);
            }
        }
        None
    }

    /// The confrontation message (`%S` = first, second, first display names).
    #[must_use]
    pub fn fight_message(&self, setup: &FightSetup) -> String {
        let name = |id| crate::text::Arg::Text(crate::names::display_name(&self.ds, id));
        self.format(setup.message, &[name(setup.first), name(setup.second), name(setup.first)])
    }

    /// The fight's two sides as 0-based character indices (`DS:9DC3`,
    /// `DS:9DD7`).
    #[must_use]
    pub fn fight_sides(&self) -> (Vec<usize>, Vec<usize>) {
        let side = |list: usize, count: usize| {
            (0..usize::from(self.ds[count]))
                .map(|i| usize::from(self.ds[list + i]).saturating_sub(1))
                .collect()
        };
        (side(fight::LIST1, fight::COUNT1), side(fight::LIST2, fight::COUNT2))
    }

    /// Apply the player's choice: follow-up messages and the final mode.
    pub fn resolve_fight(&mut self, setup: &FightSetup, mode: u16) -> (Vec<String>, u16) {
        let (formats, mode) = fight::fun_20b5_2c06_resolve(&mut self.ds, setup, mode);
        let attacker = self.ds[0x9c54];
        let defender = self.ds[0x9c55];
        let name = |id| crate::text::Arg::Text(crate::names::display_name(&self.ds, id));
        let messages = formats
            .into_iter()
            .map(|f| match f {
                0x6478 => self.format(f, &[name(attacker)]),
                0x6492 => {
                    let feminine = word(&self.ds, crate::mapsim::name(attacker) + 0x16) & 0x800 != 0;
                    let pronoun = c_string(&self.ds, usize::from(word(&self.ds, 0x79d0 + 4 * usize::from(feminine))));
                    self.format(f, &[name(attacker), crate::text::Arg::Text(pronoun), name(defender)])
                }
                _ => self.format(f, &[]),
            })
            .collect();
        (messages, mode)
    }

    fn format(&self, at: u16, args: &[crate::text::Arg]) -> String {
        crate::text::format_message(&c_string(&self.ds, usize::from(at)), args, |_| None)
    }

    /// Write positions, targets and the clock into save bytes.
    pub fn write_back(&self, save: &mut [u8]) {
        save[..SAVE_MAIN_LEN].copy_from_slice(&self.ds[DS_MAIN..DS_MAIN + SAVE_MAIN_LEN]);
        save[SAVE_TAIL..SAVE_TAIL + SAVE_TAIL_LEN]
            .copy_from_slice(&self.ds[DS_TAIL..DS_TAIL + SAVE_TAIL_LEN]);
    }
}

struct World<'a> {
    sim: &'a MapSim,
}

impl TravelWorld for World<'_> {
    fn terrain(&self, x: i16, y: i16) -> u8 {
        travel::terrain_at(&self.sim.tiles, self.sim.tile_width, x, y)
    }
    fn follow(&self, id: u8) -> Option<(i16, i16)> {
        self.sim.entity(id).map(|e| (e.x(), e.y()))
    }
}

impl EncounterWorld for MapSim {
    fn entity(&self, id: u8) -> Option<EntityView> {
        MapSim::entity(self, id).map(|e| EntityView { x: e.x(), y: e.y(), moved: e.0[0x0e] })
    }
    fn random(&mut self, n: u16) -> u16 {
        // Stand-in for the timer-seeded FUN_15b8_17e0.
        self.rng = self.rng.wrapping_mul(0x015a_4e35).wrapping_add(1);
        if n == 0 { 0 } else { ((self.rng >> 16) % u32::from(n)) as u16 }
    }
    fn combat(&mut self, first: u8, second: u8) -> bool {
        // FUN_2920_0a12 leaves the pair in DS:9DAE / DS:9DB0 for the check.
        self.ds[0x9dae] = first;
        self.ds[0x9db0] = second;
        let setup = fight::fun_20b5_2c06(&mut self.ds, &mut self.entities, true);
        if setup.mode == 0 {
            return false;
        }
        self.pending_fight = Some(setup);
        true
    }
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

    /// Runs whole rounds from the state at the start of the first recorded
    /// journey and checks every entity's bytes after every step against the
    /// original (order of steps and the shared scratch globals included).
    #[test]
    fn rounds_match_original_journey() {
        let Ok(text) = std::fs::read_to_string("../../tests/vectors/travel_step.json") else {
            eprintln!("SKIP: tests/vectors/travel_step.json absent");
            return;
        };
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let ds = hex(v["dgroup"].as_str().unwrap());
        let tiles = hex(v["tiles"].as_str().unwrap());
        let width = v["tile_width"].as_u64().unwrap() as u16;
        let calls = v["calls"].as_array().unwrap();
        let first_index = calls[0]["index"].as_u64().unwrap();
        // Only the first journey: stop when the clock jumps backwards (next run).
        let mut journey = Vec::new();
        let mut last = 0;
        for c in calls {
            let clock = c["clock"].as_u64().unwrap();
            if clock < last {
                break;
            }
            last = clock;
            journey.push(c);
        }
        let mut save = ds[DS_MAIN..DS_MAIN + SAVE_MAIN_LEN].to_vec();
        save.extend_from_slice(&ds[DS_TAIL..DS_TAIL + SAVE_TAIL_LEN]);
        let mut sim = MapSim::new(ds.clone(), &save, &tiles, width).unwrap();
        // Seed entities, clock and scratch from the first round.
        let per_round = journey
            .iter()
            .skip(1)
            .position(|c| c["index"].as_u64().unwrap() == first_index)
            .unwrap()
            + 1;
        sim.entities = journey[..per_round]
            .iter()
            .map(|c| MapEntity(hex(c["before"].as_str().unwrap()).try_into().unwrap()))
            .collect();
        sim.set_clock(journey[0]["clock"].as_u64().unwrap() as u32);
        let s = &journey[0]["scratch_before"];
        sim.scratch = TravelScratch {
            dir: s["dir"].as_u64().unwrap() as u8,
            best_tile_x: s["best_tile_x"].as_i64().unwrap() as i16,
            best_tile_y: s["best_tile_y"].as_i64().unwrap() as i16,
        };
        let rounds = journey.len() / per_round;
        for r in 0..rounds {
            let start = r * per_round;
            assert_eq!(
                u64::from(sim.clock()),
                journey[start]["clock"].as_u64().unwrap(),
                "round {r}: clock"
            );
            sim.round();
            for (k, c) in journey[start..start + per_round].iter().enumerate() {
                if c["end"].as_str().unwrap() != "return" {
                    continue;
                }
                assert_eq!(
                    sim.entities[k].0.as_slice(),
                    hex(c["after"].as_str().unwrap()).as_slice(),
                    "round {r}, entity {k}"
                );
            }
        }
        eprintln!("{rounds} rounds of {per_round} entities matched the original");
    }
}

#[cfg(test)]
mod real_data {
    use super::*;

    #[test]
    fn builds_from_real_episode() {
        let (Ok(exe), Ok(save)) = (
            std::fs::read("../../ghidra/EXCAL_UNPACKED.EXE"),
            std::fs::read("../../game/EPISODE1.DAT"),
        ) else {
            eprintln!("SKIP: game data absent");
            return;
        };
        let ds = dgroup_from_exe(&exe).expect("dgroup");
        assert!(TravelTables::from_dgroup(&ds).is_some(), "tables");
        let tiles = vec![0u8; 72 * 60];
        let sim = MapSim::new(ds, &save, &tiles, 72).expect("sim");
        eprintln!("entities: {}", sim.entities().len());
        assert!(!sim.entities().is_empty());
    }
}
