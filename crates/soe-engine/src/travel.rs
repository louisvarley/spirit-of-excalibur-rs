//! Regional map movement.
//!
//! `fun_1b06_0007` is the per-frame step of one map entity (the party, an
//! army, a wandering NPC). The UI framework (`15b8`) calls it for every map
//! widget with the widget's index into the far-pointer table at `DS:969F`.
//!
//! An entity moves one pixel at a time towards a waypoint while it has
//! movement budget. A straight step costs `0x6400 / speed` and a diagonal one
//! `0x7D00 / speed`, where `speed` is `DS:2A5C[terrain]`. When the waypoint is
//! reached it picks a new one: 16 probe directions are scored by
//! `(1000 * best_distance / distance) * speed`, the winner maps to one of 8
//! neighbour tiles, and those are rescored by `speed * weight`, skipping the
//! last six tiles visited. Arrival at the target (or at a followed entity)
//! raises the arrival callback (`DS:0672`).
//!
//! The function works on the entity's raw bytes so it can be compared with
//! the original byte for byte; offsets are named below.

#![forbid(unsafe_code)]

/// Bytes of a map entity covered by the travel step.
pub const ENTITY_BYTES: usize = 0x36;

// Entity field offsets.
const ID: usize = 0x0a;
const MOVED: usize = 0x0e;
const Y: usize = 0x10;
const X: usize = 0x12;
const TARGET_Y: usize = 0x1c;
const TARGET_X: usize = 0x1e;
const WAY_Y: usize = 0x20;
const WAY_X: usize = 0x22;
const CARRY: usize = 0x24;
const SPEED: usize = 0x26;
const FOLLOW: usize = 0x27;
const HIST_Y: usize = 0x28;
const HIST_X: usize = 0x2e;
const WAKE: usize = 0x34;

/// A map entity's raw bytes (`ENTITY_BYTES` from its far pointer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapEntity(pub [u8; ENTITY_BYTES]);

impl MapEntity {
    fn w(&self, at: usize) -> u16 {
        u16::from_le_bytes([self.0[at], self.0[at + 1]])
    }
    fn set_w(&mut self, at: usize, v: u16) {
        self.0[at..at + 2].copy_from_slice(&v.to_le_bytes());
    }
    #[must_use]
    pub fn x(&self) -> i16 {
        self.w(X) as i16
    }
    #[must_use]
    pub fn y(&self) -> i16 {
        self.w(Y) as i16
    }
    #[must_use]
    pub fn target(&self) -> (i16, i16) {
        (self.w(TARGET_X) as i16, self.w(TARGET_Y) as i16)
    }
    #[must_use]
    pub fn id(&self) -> u16 {
        self.w(ID)
    }
}

/// Constant DGROUP tables read by the step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TravelTables {
    /// `DS:0682`: weight of the i-th neighbour tried.
    pub weight8: [u8; 8],
    /// `DS:068A` / `DS:069A`: pixel offset of neighbour direction 0..8.
    pub step8_x: [i16; 8],
    pub step8_y: [i16; 8],
    /// Low bytes of the words at `DS:06AA`: probe direction -> neighbour.
    pub probe_to8: [u8; 16],
    /// `DS:06CA` / `DS:06EA`: pixel offset of probe direction 0..16.
    pub probe_x: [i16; 16],
    pub probe_y: [i16; 16],
    /// `DS:2A5C`: speed per terrain type (index 0xFF = off map).
    pub terrain_speed: [u8; 256],
    /// `DS:4FC4`: shift applied to the entity speed nibble.
    pub speed_shift: u8,
}

impl TravelTables {
    /// Read the tables from a DGROUP image (offset 0 = `DS:0000`).
    #[must_use]
    pub fn from_dgroup(ds: &[u8]) -> Option<Self> {
        let w = |at: usize| -> Option<i16> {
            Some(i16::from_le_bytes([*ds.get(at)?, *ds.get(at + 1)?]))
        };
        let mut t = Self {
            weight8: ds.get(0x682..0x68a)?.try_into().ok()?,
            step8_x: [0; 8],
            step8_y: [0; 8],
            probe_to8: [0; 16],
            probe_x: [0; 16],
            probe_y: [0; 16],
            terrain_speed: ds.get(0x2a5c..0x2b5c)?.try_into().ok()?,
            speed_shift: *ds.get(0x4fc4)?,
        };
        for i in 0..8 {
            t.step8_x[i] = w(0x68a + i * 2)?;
            t.step8_y[i] = w(0x69a + i * 2)?;
        }
        for i in 0..16 {
            t.probe_to8[i] = *ds.get(0x6aa + i * 2)?;
            t.probe_x[i] = w(0x6ca + i * 2)?;
            t.probe_y[i] = w(0x6ea + i * 2)?;
        }
        Some(t)
    }
}

/// Globals the step reads before writing (`DS:9B25`, `DS:9A7C`, `DS:9A7E`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TravelScratch {
    pub dir: u8,
    pub best_tile_x: i16,
    pub best_tile_y: i16,
}

/// What the original would have called back.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StepEvents {
    /// `CALLF [DS:0676]` with the entity id (`+0x0A`): the wake timer fired.
    pub woke: Option<u16>,
    /// `CALLF [DS:0672]` with the widget index: the entity arrived.
    pub arrived: bool,
}

/// The parts of the world the step reads.
pub trait TravelWorld {
    /// `FUN_15b8_1b1b`: terrain type at pixel `(x, y)`, `0xFF` off map.
    fn terrain(&self, x: i16, y: i16) -> u8;
    /// `FUN_15b8_16ca(0, 0xD3, id)`: position of the followed entity, or
    /// `None` when there is no such entity.
    fn follow(&self, id: u8) -> Option<(i16, i16)>;
}

/// `FUN_15b8_1b1b` over a tile grid: 16-pixel tiles, `x` in `0..=0x480`,
/// `y` in `0..=0x3C0`, row width `width`.
#[must_use]
pub fn terrain_at(tiles: &[u8], width: u16, x: i16, y: i16) -> u8 {
    if x < 0 || y < 0 || x > 0x480 || y > 0x3c0 {
        return 0xff;
    }
    let at = usize::from((y >> 4) as u16) * usize::from(width) + usize::from((x >> 4) as u16);
    tiles.get(at).copied().unwrap_or(0xff)
}

/// 8086 `CWD; XOR AX,DX; SUB AX,DX`.
fn abs16(v: i16) -> u16 {
    v.wrapping_abs() as u16
}

/// `MUL word` keeping only AX.
fn sq16(v: u16) -> u16 {
    v.wrapping_mul(v)
}

/// `FUN_1b06_0007`: one movement step for the entity at widget `index`.
///
/// `clock` is the 32-bit game clock at `DS:4FBF`; `wake_callback` is whether
/// `DS:0676` is set. Divide-by-zero cases (terrain speed 0, or a wrapped probe
/// distance of 0) trap on the original; here they count as an unaffordable
/// step and a zero score respectively.
pub fn fun_1b06_0007(
    entity: &mut MapEntity,
    tables: &TravelTables,
    scratch: &mut TravelScratch,
    world: &impl TravelWorld,
    clock: u32,
    wake_callback: bool,
) -> StepEvents {
    let mut events = StepEvents::default();
    let e = entity;
    e.0[MOVED] = 0;
    if e.w(TARGET_X) == 0 {
        return events;
    }
    if clock >> 8 < u32::from(e.w(WAKE)) {
        return events;
    }
    if e.w(WAKE) != 0 && wake_callback {
        events.woke = Some(e.w(ID));
        e.set_w(WAKE, 0);
    }
    let follow = if e.0[FOLLOW] != 0 { world.follow(e.0[FOLLOW]) } else { None };
    let mut budget = (u16::from(e.0[SPEED] & 0xf) << (tables.speed_shift & 0x1f)).wrapping_add(e.w(CARRY));

    loop {
        // 1b06:00c6
        let x = e.w(X);
        let y = e.w(Y);
        if x == e.w(TARGET_X) && y == e.w(TARGET_Y) {
            let chase = match follow {
                Some((fx, fy)) if e.0[FOLLOW] != 0 && fx != 0 && !(fx as u16 == x && fy as u16 == y) => {
                    Some((fx, fy))
                }
                _ => None,
            };
            match chase {
                Some((fx, fy)) => {
                    e.set_w(TARGET_X, fx as u16);
                    e.set_w(TARGET_Y, fy as u16);
                }
                None => {
                    e.set_w(TARGET_X, 0);
                    events.arrived = true;
                    return events;
                }
            }
        }

        // 1b06:0159
        let mut a = abs16((x as i16).wrapping_sub(e.w(WAY_X) as i16));
        let mut b = abs16((y as i16).wrapping_sub(e.w(WAY_Y) as i16));
        if a == 0 && b == 0 {
            a = abs16((x as i16).wrapping_sub(e.w(TARGET_X) as i16));
            b = abs16((y as i16).wrapping_sub(e.w(TARGET_Y) as i16));
            if a < 0x20 && b < 0x20 {
                e.set_w(WAY_X, e.w(TARGET_X));
                e.set_w(WAY_Y, e.w(TARGET_Y));
            } else {
                choose_waypoint(e, tables, scratch, world);
                a = abs16((e.w(X) as i16).wrapping_sub(e.w(WAY_X) as i16));
                b = abs16((e.w(Y) as i16).wrapping_sub(e.w(WAY_Y) as i16));
            }
        }

        // 1b06:05eb
        let x = e.w(X) as i16;
        let y = e.w(Y) as i16;
        let sx: i16 = if (a << 1) > b {
            if (e.w(WAY_X) as i16) < x { -1 } else { 1 }
        } else {
            0
        };
        let sy: i16 = if (b << 1) >= a {
            if (e.w(WAY_Y) as i16) < y { -1 } else { 1 }
        } else {
            0
        };
        let base: u16 = if sx != 0 && sy != 0 { 0x7d00 } else { 0x6400 };
        let speed = u16::from(tables.terrain_speed[usize::from(world.terrain(x, y))]);
        let cost = base.checked_div(speed).unwrap_or(u16::MAX);
        if cost > budget {
            e.set_w(CARRY, budget);
            return events;
        }
        budget -= cost;
        e.set_w(X, x.wrapping_add(sx) as u16);
        e.set_w(Y, y.wrapping_add(sy) as u16);
        e.0[MOVED] = 1;
    }
}

/// `1b06:01cd..05e7`: pick the next waypoint and push it onto the history.
fn choose_waypoint(
    e: &mut MapEntity,
    t: &TravelTables,
    scratch: &mut TravelScratch,
    world: &impl TravelWorld,
) {
    let x = e.w(X) as i16;
    let y = e.w(Y) as i16;
    let tx = e.w(TARGET_X) as i16;
    let ty = e.w(TARGET_Y) as i16;

    // Probe 16 directions: squared tile distance to the target (low word,
    // +1) and the terrain speed there.
    let mut dist = [0u16; 16];
    let mut speed = [0u8; 16];
    let mut min: u16 = 0xffff;
    for i in 0..16 {
        let px = x.wrapping_add(t.probe_x[i]);
        let py = y.wrapping_add(t.probe_y[i]);
        let a = (abs16(px.wrapping_sub(tx)) as i16) >> 4;
        let b = (abs16(py.wrapping_sub(ty)) as i16) >> 4;
        speed[i] = t.terrain_speed[usize::from(world.terrain(px, py))];
        dist[i] = sq16(a as u16).wrapping_add(sq16(b as u16)).wrapping_add(1);
        if dist[i] < min {
            min = dist[i];
        }
    }
    let scaled = u32::from(min).wrapping_mul(1000);
    let mut best: u32 = 0;
    for i in 0..16 {
        let q = scaled.checked_div(u32::from(dist[i])).unwrap_or(0);
        let score = q.wrapping_mul(u32::from(speed[i]));
        if score > best {
            best = score;
            scratch.dir = i as u8;
        }
    }
    scratch.dir = t.probe_to8[usize::from(scratch.dir & 0x0f)];
    let start = scratch.dir;

    // Rescore the 8 neighbours starting at `start`.
    let mut best: u32 = 0;
    let mut best_dir = start;
    let hist_x = &e.0[HIST_X..HIST_X + 6];
    let hist_y = &e.0[HIST_Y..HIST_Y + 6];
    for i in 0..8u8 {
        let d = start.wrapping_add(i) & 7;
        let px = x.wrapping_add(t.step8_x[usize::from(d)]);
        let py = y.wrapping_add(t.step8_y[usize::from(d)]);
        let product = u16::from(t.terrain_speed[usize::from(world.terrain(px, py))])
            .wrapping_mul(u16::from(t.weight8[usize::from(i)]));
        // MUL then CWD: the high word is the sign of AX, not the product's.
        let mut score = i32::from(product as i16) as u32;
        let tile_x = px >> 4;
        let tile_y = py >> 4;
        for k in 0..6 {
            if i16::from(hist_x[k]) == tile_x && i16::from(hist_y[k]) == tile_y {
                score = 0;
            }
        }
        let take = if score > best {
            true
        } else if score == best {
            let target_tx = tx >> 4;
            let target_ty = ty >> 4;
            let held = sq16(abs16(scratch.best_tile_x.wrapping_sub(target_tx)))
                .wrapping_add(sq16(abs16(scratch.best_tile_y.wrapping_sub(target_ty))));
            let here = sq16(abs16(tile_x.wrapping_sub(target_tx)))
                .wrapping_add(sq16(abs16(tile_y.wrapping_sub(target_ty))));
            here < held
        } else {
            false
        };
        if take {
            best = score;
            best_dir = d;
            scratch.best_tile_x = tile_x;
            scratch.best_tile_y = tile_y;
        }
    }

    let wx = ((x as u16) & 0xfff0).wrapping_add(t.step8_x[usize::from(best_dir)] as u16);
    let wy = ((y as u16) & 0xfff0).wrapping_add(t.step8_y[usize::from(best_dir)] as u16);
    e.set_w(WAY_X, wx);
    e.set_w(WAY_Y, wy);
    for k in (1..6).rev() {
        e.0[HIST_X + k] = e.0[HIST_X + k - 1];
        e.0[HIST_Y + k] = e.0[HIST_Y + k - 1];
    }
    e.0[HIST_X] = ((wx as i16) >> 4) as u8;
    e.0[HIST_Y] = ((wy as i16) >> 4) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Grid<'a> {
        tiles: &'a [u8],
        width: u16,
        follow: Option<(i16, i16)>,
    }

    impl TravelWorld for Grid<'_> {
        fn terrain(&self, x: i16, y: i16) -> u8 {
            terrain_at(self.tiles, self.width, x, y)
        }
        fn follow(&self, _id: u8) -> Option<(i16, i16)> {
            self.follow
        }
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    fn scratch(v: &serde_json::Value) -> TravelScratch {
        TravelScratch {
            dir: v["dir"].as_u64().unwrap() as u8,
            best_tile_x: v["best_tile_x"].as_i64().unwrap() as i16,
            best_tile_y: v["best_tile_y"].as_i64().unwrap() as i16,
        }
    }

    /// Replays every call recorded from the original by
    /// `tests/oracle/travel_vectors.py`.
    #[test]
    fn matches_original_travel_steps() {
        let Ok(text) = std::fs::read_to_string("../../tests/vectors/travel_step.json") else {
            eprintln!("SKIP: tests/vectors/travel_step.json absent");
            return;
        };
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let dgroup = hex(v["dgroup"].as_str().unwrap());
        let tables = TravelTables::from_dgroup(&dgroup).unwrap();
        let tiles = hex(v["tiles"].as_str().unwrap());
        let width = v["tile_width"].as_u64().unwrap() as u16;
        let calls = v["calls"].as_array().unwrap();
        let mut checked = 0;
        for (n, call) in calls.iter().enumerate() {
            let mut entity = MapEntity(hex(call["before"].as_str().unwrap()).try_into().unwrap());
            let mut s = scratch(&call["scratch_before"]);
            let follow = call["follow"]
                .as_array()
                .map(|p| (p[0].as_i64().unwrap() as i16, p[1].as_i64().unwrap() as i16));
            let world = Grid { tiles: &tiles, width, follow };
            let ev = fun_1b06_0007(
                &mut entity,
                &tables,
                &mut s,
                &world,
                call["clock"].as_u64().unwrap() as u32,
                call["wake_callback"].as_bool().unwrap(),
            );
            let after = hex(call["after"].as_str().unwrap());
            assert_eq!(entity.0.as_slice(), after.as_slice(), "call {n}: entity bytes");
            match call["end"].as_str().unwrap() {
                "arrive" => assert!(ev.arrived, "call {n}: expected arrival"),
                "wake" => assert!(ev.woke.is_some(), "call {n}: expected wake"),
                _ => {
                    assert!(!ev.arrived && ev.woke.is_none(), "call {n}: unexpected callback");
                    assert_eq!(s, scratch(&call["scratch_after"]), "call {n}: scratch");
                }
            }
            checked += 1;
        }
        assert!(checked > 0);
        eprintln!("{checked} original travel steps matched");
    }
}
