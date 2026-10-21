//! Confrontations on the map: who fights whom and how.
//!
//! `fun_20b5_2c06` is the fight check the encounter callback calls first
//! (and `FUN_2d65_2048` elsewhere). It gathers both sides around the two
//! characters in `DS:9DAE` / `DS:9DB0`, picks the most hostile pair, and
//! decides the confrontation: a challenge to a fight to the death, an
//! ambush, an attempted arrest, an army sighting or a contest. The result
//! is the mode word `DS:9C56` that `FUN_20b5_2a30` turns into a battle, plus
//! the message and buttons the original shows first.
//!
//! Helpers ported here: `FUN_2a6e_01e7` (a character's group, walking the
//! leader tree), `FUN_2a6e_06a8` (groups around a map position),
//! `FUN_26c5_041b` (disposition 0..99 between two characters),
//! `FUN_2d65_0f58` (entities -> character records) and `FUN_2d65_0a2a`
//! (location of a character's top leader).

#![forbid(unsafe_code)]

use crate::encounter::location_at;
use crate::travel::MapEntity;

const CHAR_NAME: usize = 0x2c5e;
const CHAR_SIZE: usize = 0x38;
const LAST: u8 = 0x5c;
const RELATIONS: usize = 0x4f25;
const DISPOSITION: usize = 0x69a6;

/// `DS:9DC3` / `DS:9DAC`: side one; `DS:9DD7` / `DS:9DAD`: side two.
pub const LIST1: usize = 0x9dc3;
pub const LIST2: usize = 0x9dd7;
pub const COUNT1: usize = 0x9dac;
pub const COUNT2: usize = 0x9dad;

fn name(id: u8) -> usize {
    CHAR_NAME + usize::from(id) * CHAR_SIZE
}

fn word(ds: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([ds[at], ds[at + 1]])
}

fn flags(ds: &[u8], id: u8) -> u16 {
    word(ds, name(id) + 0x16)
}

fn leader(ds: &[u8], id: u8) -> u8 {
    ds[name(id) + 0x31]
}

fn kind(ds: &[u8], id: u8) -> u8 {
    ds[name(id) + 0x26]
}

/// Kinds that never fight (`1`, `4`, `5`, `0x0C`).
fn non_combatant(k: u8) -> bool {
    matches!(k, 1 | 4 | 5 | 0x0c)
}

fn has_subordinates(ds: &[u8], id: u8) -> bool {
    (1..=LAST).any(|d| leader(ds, d) == id)
}

/// `FUN_2a6e_01e7`: add `root` and its command tree to the list at
/// `list` (count byte at `count`, at most `max`), filtered by `must` /
/// `must_not` flag masks and never `exclude`. Returns the OR of the flags
/// of everyone added.
pub fn fun_2a6e_01e7(
    ds: &mut [u8],
    root: u8,
    list: usize,
    count: usize,
    max: u8,
    mut must: u16,
    mut must_not: u16,
    exclude: u16,
) -> u16 {
    if ds[count] >= max {
        return 0;
    }
    let matches = |ds: &[u8], c: u8, must: u16, must_not: u16| {
        let f = flags(ds, c);
        f & must == must && f & must_not == 0 && u16::from(c) != exclude
    };
    let mut acc = 0u16;
    let mut stack: Vec<u8> = Vec::new();
    let mut root = root;
    macro_rules! add {
        ($c:expr) => {{
            let n = ds[count];
            ds[count] = n.wrapping_add(1);
            ds[list + usize::from(n)] = $c;
            acc |= flags(ds, $c);
            if ds[count] >= max {
                return acc;
            }
        }};
    }
    if matches(ds, root, must, must_not)
        && (flags(ds, root) & 0x08 != 0 || word(ds, name(root) + 0x1e) == 0)
    {
        let n = ds[count];
        ds[count] = n.wrapping_add(1);
        ds[list + usize::from(n)] = root;
        acc = flags(ds, root);
        if ds[count] >= max {
            return acc;
        }
        must &= 0xf8ff;
        must_not &= 0xf8ff;
    }
    'restart: loop {
        // 2a6e:02e3 / 0408: leaves first, those with 0x8000, then the rest.
        for want_8000 in [true, false] {
            for c in 1..=LAST {
                if leader(ds, c) == root
                    && !has_subordinates(ds, c)
                    && (flags(ds, c) & 0x8000 != 0) == want_8000
                    && matches(ds, c, must, must_not)
                {
                    add!(c);
                }
            }
        }
        // 2a6e:052d: descend into the first subordinate with subordinates.
        let mut c = 1u8;
        while c <= LAST {
            if leader(ds, c) == root && has_subordinates(ds, c) {
                stack.push(root);
                root = c;
                if matches(ds, c, must, must_not) {
                    add!(c);
                }
                continue 'restart;
            }
            if c == LAST {
                if let Some(parent) = stack.pop() {
                    c = root;
                    root = parent;
                }
            }
            c = c.wrapping_add(1);
        }
        let n = ds[count];
        ds[list + usize::from(n)] = 0;
        return acc;
    }
}

/// `FUN_2a6e_06a8`: gather the groups of leaderless characters within one
/// tile of `(x, y)` whose side (`flags >> 12 & 7`) matches `side` (7 = any,
/// bit 7 clear also accepts side 7).
pub fn fun_2a6e_06a8(
    ds: &mut [u8],
    x: i16,
    y: i16,
    side: u8,
    list: usize,
    count: usize,
    max: u8,
    must: u16,
    must_not: u16,
    exclude: u16,
) -> u16 {
    ds[count] = 0;
    ds[list] = 0;
    let mut acc = 0u16;
    let mut c = 1u8;
    while c < 0x5d && ds[count] < max {
        let n = name(c);
        let dx = (word(ds, n + 0x0e) as i16).wrapping_sub(x).wrapping_abs();
        let dy = (word(ds, n + 0x10) as i16).wrapping_sub(y).wrapping_abs();
        let f_side = flags(ds, c) & 0x7000;
        if dx < 2
            && dy < 2
            && leader(ds, c) == 0
            && (side == 7
                || (f_side == 0x7000 && side & 0x80 == 0)
                || (f_side >> 12) == u16::from(side & 7))
        {
            acc |= fun_2a6e_01e7(ds, c, list, count, max, must, must_not, exclude);
        }
        c += 1;
    }
    acc
}

/// `FUN_26c5_041b(a, b, delta)`: how `a` regards `b`, 0 (hostile) ..= 99.
/// Uses the 5x5 table at `DS:69A6` by flag bits 8..10, the two characters'
/// `+0x2F` / `+0x30` bytes, and the 50-entry relation table at `DS:4F25`
/// (`delta` is added to it, creating an entry when non-zero).
pub fn fun_26c5_041b(ds: &mut [u8], a: u8, b: u8, delta: i16) -> u8 {
    let fa = usize::from((flags(ds, a) as i16 >> 8) as u16 & 7);
    let fb = usize::from((flags(ds, b) as i16 >> 8) as u16 & 7);
    let base = i16::from(ds[DISPOSITION + fa * 5 + fb]);
    let (na, nb) = (name(a), name(b));
    let mut score = if ds[na + 0x2f] < 7 {
        base + 10 - i16::from(ds[nb + 0x2f])
    } else {
        i16::from(ds[nb + 0x2f]) + base - 10
    };
    score = if ds[na + 0x30] < 7 {
        score + 10 - i16::from(ds[nb + 0x30])
    } else {
        i16::from(ds[nb + 0x30]) + score - 10
    };
    let mut delta = delta;
    for k in 0..0x32 {
        let at = RELATIONS + k * 3;
        if ds[at] == a && ds[at + 1] == b {
            delta += i16::from(ds[at + 2]);
            delta = delta.clamp(0, 0xff);
            ds[at + 2] = delta as u8;
            score += i16::from(ds[at + 2]) - 100;
            delta = 0;
            break;
        }
    }
    if delta != 0 {
        for k in 0..0x32 {
            let at = RELATIONS + k * 3;
            let slot = ds[at];
            if slot == 0
                || word(ds, name(slot) + 0x1e) == 0
                || word(ds, name(ds[at + 1]) + 0x1e) == 0
            {
                ds[at] = a;
                ds[at + 1] = b;
                delta = (delta + 100).clamp(0, 0xff);
                ds[at + 2] = delta as u8;
                break;
            }
        }
    }
    (score + delta).clamp(0, 99) as u8
}

/// `FUN_2d65_0f58`: copy every map entity back into its character record.
pub fn fun_2d65_0f58(ds: &mut [u8], entities: &[MapEntity]) {
    for e in entities {
        let n = name(e.id() as u8);
        let (tx, ty) = e.target();
        for (at, v) in [(0x0e, e.x()), (0x10, e.y()), (0x12, tx), (0x14, ty)] {
            ds[n + at..n + at + 2].copy_from_slice(&(v as u16).to_le_bytes());
        }
        if flags(ds, e.id() as u8) & 0x100 == 0 {
            ds[n + 0x18] = e.0[0x27];
        }
        ds[n + 0x1a..n + 0x1c].copy_from_slice(&e.0[0x34..0x36]);
    }
}

/// `FUN_2d65_0a2a`: location of `id`'s top leader (`FUN_2d65_09a2`).
#[must_use]
pub fn fun_2d65_0a2a(ds: &[u8], entities: &[MapEntity], id: u8) -> u8 {
    let mut top = id;
    while leader(ds, top) != 0 {
        top = leader(ds, top);
    }
    match entities.iter().find(|e| e.id() == u16::from(top)) {
        Some(e) if ds[0x9df6] == 0 => location_at(ds, e.x(), e.y()),
        _ => location_at(ds, word(ds, name(top) + 0x0e) as i16, word(ds, name(top) + 0x10) as i16),
    }
}

/// A button the confrontation window offers, with the mode it selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FightButton {
    Accept(u16),
    Decline,
    ExecuteFoe,
    ArmyAttack,
    GoThere(u16),
    IgnoreIt,
}

/// What `FUN_20b5_2c06` set up before waiting for the player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FightSetup {
    /// `DS:9C56`; 0 means no confrontation (the function returns 0).
    pub mode: u16,
    /// `DS:9C4E` / `DS:9C4F`: the most hostile pair.
    pub first: u8,
    pub second: u8,
    /// `DS:9C54` / `DS:9C55`: attacker and defender.
    pub attacker: u8,
    pub defender: u8,
    /// Lowest disposition found (`local_c`, 99 when none).
    pub hostility: u8,
    /// DGROUP offset of the message format (`%S` = display names of
    /// first, second, first).
    pub message: u16,
    pub buttons: Vec<FightButton>,
    /// The window waits for a button (`FUN_15b8_09f7`) instead of a timed
    /// pause.
    pub modal: bool,
}

/// `FUN_20b5_2c06` up to the message window: gather sides, pick the pair
/// and decide the confrontation. `map` is `DS:9DC0 == 1`.
pub fn fun_20b5_2c06(ds: &mut [u8], entities: &mut [MapEntity], map: bool) -> FightSetup {
    let mut t: u16 = 0; // local_10
    let mut e: u16 = 0; // local_e
    let mut k: u16; // local_14 (keeps 0x32 from the clearing loop)
    ds[0x9c55] = 0;
    ds[0x9c54] = 0;
    let mut modal = false;
    let mut mode: u16 = 0;
    let mut a = ds[0x9dae];
    let mut b = ds[0x9db0];
    if map {
        if (flags(ds, b) & 0x200 != 0 && flags(ds, a) & 0x200 == 0)
            || (flags(ds, a) & 0x400 != 0 && flags(ds, b) & 0x400 == 0)
        {
            std::mem::swap(&mut a, &mut b);
        }
        ds[COUNT1] = 0;
        ds[COUNT2] = 0;
        ds[LIST1..LIST1 + 0x32].fill(0);
        k = 0x32;
        fun_2d65_0f58(ds, entities);
        let pos = |ds: &[u8], c: u8| (word(ds, name(c) + 0x0e) as i16, word(ds, name(c) + 0x10) as i16);
        if flags(ds, a) & 0x200 != 0 && flags(ds, b) & 0x200 == 0 {
            let (x, y) = pos(ds, a);
            k = fun_2a6e_06a8(ds, x, y, 7, LIST1, COUNT1, 0x14, 0x208, 0, u16::from(b));
            t = k;
        }
        if flags(ds, b) & 0x400 != 0 {
            let (x, y) = pos(ds, b);
            e = fun_2a6e_06a8(ds, x, y, 7, LIST2, COUNT2, 0x14, 0x408, 0, u16::from(a));
        }
        if (t | e) & 0x8000 == 0 || t == 0 {
            ds[COUNT1] = 0;
            if flags(ds, a) & 0x700 == 0 || flags(ds, b) & 0x400 != 0 {
                let (x, y) = pos(ds, a);
                t = fun_2a6e_06a8(ds, x, y, 0, LIST1, COUNT1, 0x14, 8, 0x500, u16::from(b));
            } else {
                t = fun_2a6e_01e7(ds, a, LIST1, COUNT1, 0x14, 8, 0, u16::from(b));
            }
        }
        if kind(ds, b) == 0x0b && k != 0 && ds[COUNT1] == 0 {
            let (x, y) = pos(ds, a);
            t = fun_2a6e_06a8(ds, x, y, 7, LIST1, COUNT1, 0x14, 0x208, 0, u16::from(b));
        }
        if e & 0x8000 == 0 || e == 0 {
            ds[COUNT2] = 0;
            e = fun_2a6e_01e7(ds, b, LIST2, COUNT2, 0x14, 8, 0, u16::from(a));
        }
    }
    ds[0x9dae] = ds[LIST1];
    ds[0x9db0] = ds[LIST2];
    let a = ds[0x9dae];
    let b = ds[0x9db0];

    // Move non-combatants to the end of side one.
    let count1 = usize::from(ds[COUNT1]);
    let fighters = (0..count1).filter(|&i| !non_combatant(kind(ds, ds[LIST1 + i]))).count();
    for i in 0..fighters {
        while non_combatant(kind(ds, ds[LIST1 + i])) {
            let x = ds[LIST1 + i];
            for j in i..count1.saturating_sub(1) {
                ds[LIST1 + j] = ds[LIST1 + j + 1];
            }
            ds[LIST1 + count1 - 1] = x;
        }
    }
    // FUN_15b8_16ca(0x200, 0, id)->+0x34 = i: no map entity carries 0x200.

    // The most hostile fighting pair.
    let mut hostility: u8 = 99;
    for i in 0..count1 {
        let p = ds[LIST1 + i];
        if non_combatant(kind(ds, p)) {
            continue;
        }
        for j in 0..usize::from(ds[COUNT2]) {
            let q = ds[LIST2 + j];
            let d1 = fun_26c5_041b(ds, q, p, 0);
            let d2 = fun_26c5_041b(ds, p, q, 0);
            if !non_combatant(kind(ds, q)) && (d1 < hostility || d2 < hostility) {
                hostility = d1.min(d2);
                ds[0x9c52] = i as u8;
                ds[0x9c53] = (j as u8).wrapping_add(0x14);
                ds[0x9c4e] = p;
                ds[0x9c4f] = q;
            }
        }
    }
    let (p, q) = (ds[0x9c4e], ds[0x9c4f]);
    let mut message = 0u16;
    let mut buttons = Vec::new();

    'decided: {
        if hostility < 0x1f && map {
            let (tt, ee) = (t & 0x8000 != 0, e & 0x8000 != 0);
            if !(tt && ee) {
                if tt || ee {
                    if !tt || flags(ds, a) & 0x8200 != 0x200 {
                        if flags(ds, a) & 0x8000 == 0 {
                            if flags(ds, b) & 0x8000 == 0 {
                                if !ee {
                                    message = 0x6248;
                                    ds[0x9c54] = p;
                                    ds[0x9c55] = q;
                                    mode = 0x1182;
                                } else {
                                    message = 0x639a;
                                    ds[0x9c55] = p;
                                    ds[0x9c54] = q;
                                    mode = 0x1282;
                                }
                                break 'decided;
                            }
                            message = 0x6367;
                        } else {
                            message = 0x633e;
                        }
                        mode = 0x30;
                        ds[0x9c55] = p;
                        ds[0x9c54] = q;
                    } else {
                        mode = 0x30;
                        ds[0x9c54] = p;
                        ds[0x9c55] = q;
                        if !matches!(kind(ds, q), 6 | 10 | 11) {
                            message = 0x6248;
                            buttons = vec![FightButton::Accept(0x181), FightButton::Decline, FightButton::ExecuteFoe];
                            modal = true;
                        } else {
                            message = 0x6303;
                        }
                    }
                }
            } else if flags(ds, a) & 0x200 == 0 || a == p || a == q {
                mode = 0x10;
                message = if flags(ds, p) & 0x8000 == 0 { 0x62c0 } else { 0x6293 };
                if ds[0x4fc5] & 4 == 0 {
                    buttons = vec![FightButton::GoThere(0x10), FightButton::IgnoreIt];
                    modal = true;
                }
            } else {
                mode = 0x10;
                message = 0x6248;
                buttons = vec![FightButton::Accept(0x382), FightButton::Decline, FightButton::ArmyAttack];
                modal = true;
            }
        }
    }
    // 20b5:3514
    if mode == 0 && t & 0x8000 == 0 && e & 0x8000 == 0 {
        if hostility < 0x15 {
            mode = 1;
            message = 0x63c4;
            if map
                && ds[0x4fc5] & 4 == 0
                && fun_2d65_0a2a(ds, entities, p) != 6
                && fun_2d65_0a2a(ds, entities, q) != 6
            {
                buttons = vec![FightButton::GoThere(1), FightButton::IgnoreIt];
                modal = true;
            }
        } else {
            let home = |c: u8| matches!(word(ds, name(c) + 0x22), 700 | 0x2c3);
            if home(p) && home(q) && hostility <= 0x31 {
                mode = 4;
                message = 0x63f3;
                if hostility < 0x1f {
                    mode = 2;
                    message = 0x6248;
                }
                buttons = vec![FightButton::Accept(mode), FightButton::Decline];
                modal = true;
            }
        }
    }
    ds[0x9c56..0x9c58].copy_from_slice(&mode.to_le_bytes());
    FightSetup {
        mode,
        first: p,
        second: q,
        attacker: ds[0x9c54],
        defender: ds[0x9c55],
        hostility,
        message,
        buttons,
        modal,
    }
}

/// The rest of `FUN_20b5_2c06` once the player has chosen (`mode` is the
/// value of the chosen button, or the setup's mode when no button was
/// offered): the follow-up messages (DGROUP format offsets) and the final
/// mode passed to `FUN_20b5_2a30`, which starts the battle.
pub fn fun_20b5_2c06_resolve(ds: &mut [u8], setup: &FightSetup, mode: u16) -> (Vec<u16>, u16) {
    let mut mode = mode;
    let mut messages = Vec::new();
    if mode != 0 {
        if (flags(ds, setup.first) ^ flags(ds, setup.second)) & 0x200 == 0 {
            if mode & 0x20 == 0 {
                messages.push(if flags(ds, setup.first) & 0x200 == 0 { 0x6450 } else { 0x6428 });
            }
            mode |= 8;
        }
        if mode & 0x1000 != 0 {
            if ds[name(ds[0x9c54]) + 0x2f] < 6 {
                mode = 0x70;
            } else {
                messages.push(0x6478);
            }
        }
        if mode & 0x40 != 0 {
            messages.push(0x6492);
        }
    }
    ds[0x9c56..0x9c58].copy_from_slice(&mode.to_le_bytes());
    (messages, mode)
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

    fn label(b: FightButton) -> (&'static str, Option<u16>) {
        match b {
            FightButton::Accept(v) => ("Accept", Some(v)),
            FightButton::Decline => ("Decline", Some(0)),
            FightButton::ExecuteFoe => ("Execute Foe", Some(0x70)),
            FightButton::ArmyAttack => ("Army Attack", Some(0x10)),
            FightButton::GoThere(v) => ("Go There", Some(v)),
            FightButton::IgnoreIt => ("Ignore It", Some(0x30)),
        }
    }

    /// Replays calls of the original made by `tests/oracle/fight_vectors.py`.
    #[test]
    fn matches_original_fight_checks() {
        let Ok(text) = std::fs::read_to_string("../../tests/vectors/fight.json") else {
            eprintln!("SKIP: tests/vectors/fight.json absent");
            return;
        };
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let base = hex(v["dgroup"].as_str().unwrap());
        let mut confrontations = 0;
        for (n, call) in v["calls"].as_array().unwrap().iter().enumerate() {
            let pair = &call["pair"];
            let mut ds = base.clone();
            overlay(&mut ds, &call["before"]["regions"]);
            let mut ents = entities(&call["before"]["entities"]);
            let setup = fun_20b5_2c06(&mut ds, &mut ents, true);
            let (follow, mode) = fun_20b5_2c06_resolve(&mut ds, &setup, setup.mode);

            assert_eq!(u64::from(setup.mode != 0), call["ret"].as_u64().unwrap(), "call {n} {pair}: return");
            let buttons: Vec<(String, u64)> = call["buttons"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| (b[0].as_str().unwrap().to_string(), b[1].as_u64().unwrap()))
                .collect();
            let mine: Vec<(String, u64)> = setup
                .buttons
                .iter()
                .map(|&b| {
                    let (l, v) = label(b);
                    (l.to_string(), u64::from(v.unwrap()))
                })
                .collect();
            assert_eq!(mine, buttons, "call {n} {pair}: buttons");
            let mut messages = Vec::new();
            if setup.mode != 0 {
                messages.push(u64::from(setup.message));
                messages.extend(follow.iter().map(|&m| u64::from(m)));
            }
            let want: Vec<u64> = call["messages"].as_array().unwrap().iter().map(|m| m.as_u64().unwrap()).collect();
            assert_eq!(messages, want, "call {n} {pair}: messages");
            if setup.mode != 0 {
                assert_eq!(Some(u64::from(mode)), call["battle"].as_u64(), "call {n} {pair}: battle mode");
                confrontations += 1;
            }
            let mut want_ds = base.clone();
            overlay(&mut want_ds, &call["after"]["regions"]);
            for (addr, bytes) in call["after"]["regions"].as_object().unwrap() {
                let at = usize::from_str_radix(addr, 16).unwrap();
                let len = bytes.as_str().unwrap().len() / 2;
                assert_eq!(&ds[at..at + len], &want_ds[at..at + len], "call {n} {pair}: DS:{addr}");
            }
            assert_eq!(ents, entities(&call["after"]["entities"]), "call {n} {pair}: entities");
        }
        eprintln!(
            "{} original fight checks matched ({confrontations} confrontations)",
            v["calls"].as_array().unwrap().len()
        );
    }
}
