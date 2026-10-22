//! Segment `1ba8` inventory leaves, ported against DGROUP.
//!
//! The instance table at `DS:4DD1` holds `0x55` four-byte records
//! `[definition, owner, count, flags]`. `flags & 0x18 == 0x18` marks an
//! instance carried by character `owner`; otherwise `owner` is a scene and
//! `flags & 7` the spot in it. Definition 1 is the gold besant: a carried
//! character's gold is the word at `+0x28` of the character record
//! (`DS:2C5E + id * 0x38`), not an instance.
//!
//! See `notes/SEGMENT_1BA8.md`. Verified by `tests/oracle/inventory_vectors.py`.

#![forbid(unsafe_code)]

/// `DS:4DD1`: item instances.
pub const INSTANCES: usize = 0x4DD1;
/// Number of instance records.
pub const INSTANCE_COUNT: usize = 0x55;
/// `DS:2C5E`: character records, `0x38` bytes each.
const CHARACTERS: usize = 0x2C5E;
/// `DS:488B`: item definitions, `0x1B` bytes each.
const DEFINITIONS: usize = 0x488B;
/// `DS:9C06`: definition of the last item moved.
const LAST_DEF: usize = 0x9C06;
/// `DS:9DF5`: the current scene (owner of ground items).
const SCENE: usize = 0x9DF5;
/// `DS:9DAB`: spot in the scene where dropped items land.
const SPOT: usize = 0x9DAB;
/// "You can't do that with a statue!"
pub const MSG_STATUE: u16 = 0x5187;
/// Character kind byte `+0x26` of a statue.
const KIND_STATUE: u8 = 0x16;

/// "%S %s %d %s point%c."
pub const MSG_STAT_CHANGE: u16 = 0x5713;
/// "%WUnable to tolerate the reproach of his comrades, %S forswears the Round Table."
pub const MSG_FORSWEAR: u16 = 0x5734;
/// "gains" / "loses".
const GAINS: u16 = 0x5728;
const LOSES: u16 = 0x572E;
/// Per-stat tables indexed by `stat - 0x24`: label far pointers, the random
/// bound and the threshold that limit gains.
const STAT_LABELS: usize = 0x500C;
const STAT_RANDOM: usize = 0x5040;
const STAT_FLOOR: usize = 0x504D;

/// One argument of a formatted message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgArg {
    /// `%S` of `FUN_2d65_08a4(id)`, the character's display name.
    Name(u8),
    /// `%s`/`%S` of the string at `DS:off`.
    Str(u16),
    /// `%d`.
    Int(i16),
    /// `%c` or `%W`: a raw word.
    Word(u16),
}

/// What the `1ba8` leaves need from the rest of the game.
pub trait RoomHost {
    /// `FUN_15b8_17e0(n)`.
    fn random(&mut self, n: u16) -> u16;
    /// `FUN_1772_000e`: a message box formatting the string at `DS:text`.
    fn message(&mut self, text: u16, args: &[MsgArg]);
    /// `FUN_15b8_1876(n)`.
    fn fun_15b8_1876(&mut self, n: u8);
    /// `FUN_1772_0578`.
    fn fun_1772_0578(&mut self);
    /// `FUN_20b5_1229(id)`: a character leaves (see `combat::fun_20b5_1229`).
    fn fun_20b5_1229(&mut self, id: u8);
}

fn character(id: u8) -> usize {
    CHARACTERS + usize::from(id) * 0x38
}

fn inst(i: usize) -> usize {
    INSTANCES + i * 4
}

fn word(ds: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([ds[at], ds[at + 1]])
}

fn set_word(ds: &mut [u8], at: usize, v: u16) {
    ds[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

/// `FUN_1ba8_0068`: true (after a message) when character `id` is a statue.
pub fn fun_1ba8_0068(ds: &[u8], id: u8, host: &mut dyn RoomHost) -> bool {
    let statue = ds[character(id) + 0x26] == KIND_STATUE;
    if statue {
        host.message(MSG_STATUE, &[]);
    }
    statue
}

/// `FUN_1ba8_0431`: move `count` of definition `def` from `from` to `to`.
///
/// Gold moves between the purses; other items move by rewriting the owner of
/// up to `count` carried instances, lowest slot first. `count == 0` moves
/// every match, as in the original (the counter wraps before it is tested).
pub fn fun_1ba8_0431(ds: &mut [u8], def: u8, from: u8, to: u8, count: u8) {
    ds[LAST_DEF] = def;
    if def == 1 {
        let n = u16::from(count);
        let f = character(from) + 0x28;
        set_word(ds, f, word(ds, f).wrapping_sub(n));
        let t = character(to) + 0x28;
        set_word(ds, t, word(ds, t).wrapping_add(n));
        return;
    }
    let mut left = count;
    for i in 0..INSTANCE_COUNT {
        let r = inst(i);
        if ds[r + 3] & 0x18 == 0x18 && ds[r + 1] == from && ds[r] == def {
            ds[r + 1] = to;
            left = left.wrapping_sub(1);
            if left == 0 {
                return;
            }
        }
    }
}

/// `FUN_1ba8_04fd`: how many of definition `def` character `owner` carries
/// (their gold for definition 1). Only flags exactly `0x18` count here.
#[must_use]
pub fn fun_1ba8_04fd(ds: &[u8], def: u8, owner: u8) -> u16 {
    if def == 1 {
        return word(ds, character(owner) + 0x28);
    }
    let mut n = 0u16;
    for i in 0..INSTANCE_COUNT {
        let r = inst(i);
        if ds[r] == def && ds[r + 3] == 0x18 && ds[r + 1] == owner {
            n = n.wrapping_add(1);
        }
    }
    n
}

/// `FUN_1ba8_2ad2`: first carried instance slot of `owner` matching `def`
/// (any definition when 0), flag bit 0 == `bit` and, unless `class` is 0,
/// definition word `+0x18 >> 12 == class`. 0 when none (or `def == 1`).
#[must_use]
pub fn fun_1ba8_2ad2(ds: &[u8], def: u8, owner: u8, bit: u8, class: u8) -> u8 {
    if def == 1 {
        return 0;
    }
    for i in 0..INSTANCE_COUNT {
        let r = inst(i);
        if def != 0 && ds[r] != def {
            continue;
        }
        if ds[r + 1] != owner || ds[r + 3] & 0x18 != 0x18 {
            continue;
        }
        if u16::from(ds[r + 3] & 1) != u16::from(bit) {
            continue;
        }
        if class == 0 {
            return i as u8;
        }
        let d = DEFINITIONS + usize::from(ds[r]) * 0x1B;
        if word(ds, d + 0x18) >> 12 == u16::from(class) {
            return i as u8;
        }
    }
    0
}

/// `FUN_1ba8_01ff`: character `owner` loses an item of definition `def`,
/// which falls to the ground in the current scene and spot.
///
/// Gold drops `random(21) + 5` coins, or `random(gold) + 1` when the purse
/// holds fewer than 25. They join a ground pile with room for them or take
/// the first free slot from 1; with no free slot the gold path falls
/// through to the item path, as in the original.
pub fn fun_1ba8_01ff(ds: &mut [u8], def: u8, owner: u8, host: &mut dyn RoomHost) {
    ds[LAST_DEF] = def;
    if def == 1 {
        let purse = character(owner) + 0x28;
        let mut amount = host.random(0x15).wrapping_add(5);
        if word(ds, purse) < 0x19 {
            amount = host.random(word(ds, purse)).wrapping_add(1);
        }
        set_word(ds, purse, word(ds, purse).wrapping_sub(amount));
        for i in 0..INSTANCE_COUNT {
            let r = inst(i);
            let f = ds[r + 3];
            if ds[r + 1] == ds[SCENE]
                && ds[r] == 1
                && f & 7 == ds[SPOT]
                && f & 0xE0 == 0
                && f & 0x18 != 0x10
                && (0xFF - i16::from(ds[r + 2])) >= amount as i16
            {
                ds[r + 2] = ds[r + 2].wrapping_add(amount as u8);
                return;
            }
        }
        for i in 1..INSTANCE_COUNT {
            let r = inst(i);
            if ds[r] == 0 {
                ds[r] = 1;
                ds[r + 1] = ds[SCENE];
                ds[r + 2] = amount as u8;
                ds[r + 3] = ds[SPOT];
                return;
            }
        }
    }
    for i in 0..INSTANCE_COUNT {
        let r = inst(i);
        if ds[r + 3] & 0x18 == 0x18 && ds[r + 1] == owner && ds[r] == def {
            ds[r + 1] = ds[SCENE];
            ds[r + 3] = ds[SPOT];
            return;
        }
    }
}

/// `FUN_1ba8_2611`: change stat byte `stat` (an offset in the character
/// record, `0x25..=0x30`) of character `id` by `delta`, clamped to `0..=200`.
///
/// Only characters with presence flags `& 0x8208 == 0x208` are affected. A
/// gain happens only when `random(bound) + floor >= old value`. Any change is
/// reported; a nobility loss (any stat in fact) leaving `+0x2F <= 5` makes
/// the knight forswear the Round Table.
pub fn fun_1ba8_2611(ds: &mut [u8], stat: u8, id: u8, delta: i16, host: &mut dyn RoomHost) {
    let rec = character(id);
    if word(ds, rec + 0x16) & 0x8208 != 0x208 {
        return;
    }
    let s = usize::from(stat.wrapping_add(0xDC));
    let at = rec + usize::from(stat);
    let old = ds[at];
    if delta > 0 {
        let roll = host.random(u16::from(ds[STAT_RANDOM + s]));
        if roll.wrapping_add(u16::from(ds[STAT_FLOOR + s])) < u16::from(old) {
            return;
        }
    }
    let new = (i16::from(old).wrapping_add(delta)).clamp(0, 200);
    ds[at] = new as u8;
    let change = new - i16::from(old);
    if change == 0 {
        return;
    }
    let plural = if change.wrapping_abs() > 1 { 0x73 } else { 0 };
    let label = word(ds, STAT_LABELS + s * 4);
    let verb = if change > 0 { GAINS } else { LOSES };
    host.message(
        MSG_STAT_CHANGE,
        &[
            MsgArg::Name(id),
            MsgArg::Str(verb),
            MsgArg::Int(change.wrapping_abs()),
            MsgArg::Str(label),
            MsgArg::Word(plural),
        ],
    );
    if change < 0 && ds[rec + 0x2F] <= 5 {
        host.fun_15b8_1876(5);
        host.fun_1772_0578();
        host.message(MSG_FORSWEAR, &[MsgArg::Word(0x104), MsgArg::Name(id)]);
        host.fun_20b5_1229(id);
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

    struct Replay {
        rng: Vec<u16>,
        next: usize,
        msgs: Vec<serde_json::Value>,
        subcalls: Vec<serde_json::Value>,
        ds: Vec<u8>,
    }

    fn asciiz(ds: &[u8], at: u16) -> String {
        let at = usize::from(at);
        ds[at..]
            .iter()
            .take_while(|&&b| b != 0)
            .map(|&b| b as char)
            .collect()
    }

    impl RoomHost for Replay {
        fn random(&mut self, _n: u16) -> u16 {
            self.next += 1;
            self.rng[self.next - 1]
        }
        fn message(&mut self, text: u16, args: &[MsgArg]) {
            let args: Vec<serde_json::Value> = args
                .iter()
                .map(|a| match *a {
                    MsgArg::Name(id) => crate::names::display_name(&self.ds, id).into(),
                    MsgArg::Str(off) => asciiz(&self.ds, off).into(),
                    MsgArg::Int(v) => v.into(),
                    MsgArg::Word(v) => v.into(),
                })
                .collect();
            self.msgs.push(serde_json::json!([text, args]));
        }
        fn fun_15b8_1876(&mut self, n: u8) {
            self.subcalls.push(serde_json::json!(["15b8_1876", n]));
        }
        fn fun_1772_0578(&mut self) {
            self.subcalls.push(serde_json::json!(["1772_0578"]));
        }
        fn fun_20b5_1229(&mut self, id: u8) {
            self.subcalls.push(serde_json::json!(["20b5_1229", id]));
        }
    }

    /// Bytes the host owns: the `FUN_15b8_17e0` seed at `DS:01FC`, and the
    /// ring of four `0x1E` name buffers (`DS:9D32`, index `DS:7ACD`) that
    /// `FUN_2d65_08a4` formats into, which `names::display_name` replaces.
    const HOST_OWNED: [std::ops::Range<usize>; 3] = [0x1FC..0x200, 0x7ACD..0x7ACE, 0x9D32..0x9DAA];

    /// Replays every call recorded from the original
    /// (`tests/oracle/inventory_vectors.py`): return value, random draws,
    /// messages and every DGROUP byte written.
    #[test]
    fn matches_original_inventory_leaves() {
        let Ok(text) = std::fs::read_to_string("../../tests/vectors/inventory.json") else {
            eprintln!("SKIP: tests/vectors/inventory.json absent");
            return;
        };
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let mut n = 0;
        for snap in v["snapshots"].as_array().unwrap() {
            let base = hex(snap["dgroup"].as_str().unwrap());
            for call in snap["calls"].as_array().unwrap() {
                let mut ds = base.clone();
                for p in call["patch"].as_array().unwrap() {
                    ds[p[0].as_u64().unwrap() as usize] = p[1].as_u64().unwrap() as u8;
                }
                let before = ds.clone();
                let w: Vec<u16> = call["args"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_u64().unwrap() as u16)
                    .collect();
                let a: Vec<u8> = w.iter().map(|&x| x as u8).collect();
                let rng = call["rng"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_u64().unwrap() as u16)
                    .collect();
                let mut host = Replay {
                    rng,
                    next: 0,
                    msgs: Vec::new(),
                    subcalls: Vec::new(),
                    ds: before.clone(),
                };
                let ax = call["ax"].as_u64().unwrap() as u16;
                let f = call["fn"].as_str().unwrap();
                let what = format!("{} {f} {a:?}", snap["snapshot"]);
                match f {
                    "0068" => assert_eq!(
                        fun_1ba8_0068(&ds, a[0], &mut host),
                        ax & 0xFF == 1,
                        "{what}"
                    ),
                    "0431" => fun_1ba8_0431(&mut ds, a[0], a[1], a[2], a[3]),
                    "04fd" => assert_eq!(fun_1ba8_04fd(&ds, a[0], a[1]), ax, "{what}"),
                    "2ad2" => assert_eq!(
                        fun_1ba8_2ad2(&ds, a[0], a[1], a[2], a[3]),
                        ax as u8,
                        "{what}"
                    ),
                    "01ff" => fun_1ba8_01ff(&mut ds, a[0], a[1], &mut host),
                    "2611" => fun_1ba8_2611(&mut ds, a[0], a[1], w[2] as i16, &mut host),
                    _ => panic!("unknown {f}"),
                }
                assert_eq!(host.next, host.rng.len(), "random draws {what}");
                assert_eq!(
                    &host.msgs,
                    call["msgs"].as_array().unwrap(),
                    "messages {what}"
                );
                assert_eq!(
                    &host.subcalls,
                    call["subcalls"].as_array().unwrap(),
                    "subcalls {what}"
                );
                let mut want = before.clone();
                for w in call["writes"].as_array().unwrap() {
                    want[w[0].as_u64().unwrap() as usize] = w[1].as_u64().unwrap() as u8;
                }
                for i in HOST_OWNED.iter().cloned().flatten() {
                    want[i] = before[i];
                }
                let diff: Vec<usize> = (0..ds.len()).filter(|&i| ds[i] != want[i]).collect();
                assert!(diff.is_empty(), "DGROUP differs at {diff:x?} {what}");
                n += 1;
            }
        }
        eprintln!("{n} inventory calls matched");
    }
}
