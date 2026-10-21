//! Combat step, ported from `FUN_20b5_1dff`.
//!
//! The battle scene runs a small animation script per combatant (army unit
//! or single-combat champion). Script steps that need game rules call
//! `FUN_20b5_1dff(op)`: the high byte of `op` is the opcode (`CS:1EC6`
//! table), the low byte its operand.
//!
//! | op | handler | rule |
//! |---:|---|---|
//! | 0 | `1ED8` | wait for the player's order / advance |
//! | 1 | `1F29` | random check against operand + attack |
//! | 2 | `1F43` | random wait timer |
//! | 3 | `1F84` | fixed timer, set attitude |
//! | 4 | `1FBC` | morale: stand, fall back or rout |
//! | 5 | `20FF` | flee or withdraw |
//! | 6 | `22F4` | attack: hit roll, wounds, conceding, death |
//! | 7 | `2769` | battle start (scene set-up; not ported here) |
//! | 8 | `2268` | magic, possibly absorbed |
//!
//! State lives in DGROUP (character records at `DS:2C5E + 0x38*id`, the
//! per-slot bytes at `DS:9C82`, timers) and in the acting and target
//! widgets (`+0x27` script state, `+0x30` timer, `+0x2E` target id, `+0x34`
//! slot). Calls into routines this function does not own are returned as
//! [`CombatEvent`]s.
//!
//! Two original bugs are reproduced: opcode 6 reads the uninitialised local
//! `[BP-0x10]` (passed in as `stale_bp10`), and opcode 5 calls
//! `FUN_26c5_041b` with only two arguments, so its delta is whatever the
//! previous call left on the stack.

#![forbid(unsafe_code)]

use crate::fight::fun_26c5_041b;

const CHAR_NAME: usize = 0x2c5e;
const CHAR_SIZE: usize = 0x38;
/// `DS:9C82`: one state byte per battle slot.
const SLOTS: usize = 0x9c82;
const CLOCK: usize = 0x7e3e;

pub type Widget = [u8; 0x36];

/// What the original does outside this function, in call order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CombatEvent {
    /// `FUN_20b5_0000(format, ...)`: a battle message (DGROUP format offset).
    Message(u16),
    /// `FUN_1316_183c(n)`: a sound effect.
    Sound(u16),
    /// `FUN_20b5_0140(id, slot)`: a unit was hit.
    UnitHit { id: u8, slot: u16 },
    /// `FUN_20b5_1057(id)`: a unit died.
    DeadUnit(u8),
    /// `FUN_1ba8_2611(stat, id, 1)`.
    StatChange { stat: u16, id: u8 },
    /// `FUN_15b8_11ac`: the dead mark next to a list entry.
    Mark { slot: u16 },
    /// `FUN_26c5_041b(a, b, delta)` called directly.
    Disposition { a: u8, b: u8, delta: i16 },
    /// `FUN_20b5_1705(widget, flag)` for the acting (`actor`) or target widget.
    Rout { actor: bool, flag: u16 },
    /// Opcode 7: the battle scene starts.
    BattleStart,
}

/// Inputs from the rest of the game.
pub trait CombatWorld {
    /// `FUN_15b8_17e0(n)`.
    fn random(&mut self, n: u16) -> u16;
    /// `FUN_20b5_1705(target, 1)` in opcode 5: true when the target routs.
    fn rout_check(&mut self) -> bool;
    /// `FUN_2d65_0a2a(id)`: location of the character's top leader.
    fn location_of(&self, ds: &[u8], id: u8) -> u8;
}

fn rec(id: u8) -> usize {
    CHAR_NAME + usize::from(id) * CHAR_SIZE
}

fn word(ds: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([ds[at], ds[at + 1]])
}

fn set_word(ds: &mut [u8], at: usize, v: u16) {
    ds[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

fn ww(w: &Widget, at: usize) -> u16 {
    u16::from_le_bytes([w[at], w[at + 1]])
}

fn set_timer(w: &mut Widget, v: u32) {
    w[0x30..0x34].copy_from_slice(&v.to_le_bytes());
}

fn slot_at(slot: u16) -> usize {
    SLOTS.wrapping_add(usize::from(slot))
}

fn clock(ds: &[u8]) -> u32 {
    u32::from(word(ds, CLOCK)) | u32::from(word(ds, CLOCK + 2)) << 16
}

/// `FUN_20b5_1dff(op)` for the acting widget (`DS:9A6E`) and its target
/// (the widget `FUN_15b8_16ca(0x200, 0, actor[+0x2E])` finds). Returns AL
/// and the events.
pub fn fun_20b5_1dff(
    ds: &mut [u8],
    actor: &mut Widget,
    mut target: Option<&mut Widget>,
    op: u16,
    stale_bp10: u16,
    world: &mut impl CombatWorld,
) -> (u8, Vec<CombatEvent>) {
    let mut ev = Vec::new();
    let aid = actor[0x0a];
    let ra = rec(aid);
    let skill = i16::from(ds[ra + 0x2d]);
    let derived = (i16::from(ds[ra + 0x2a]) - 10) >> 1;
    let attack = if skill <= derived { derived } else { skill };
    let slot = ww(actor, 0x34);
    let operand = op & 0xff;
    let opcode = op >> 8;
    if opcode > 8 {
        return (2, ev);
    }
    match opcode {
        0 => {
            if ds[0x01f2] == 0 || slot as i16 >= 0x14 {
                actor[0x27] = actor[0x27].wrapping_add(2);
                return (2, ev);
            }
            if word(ds, 0x7d3e) != 0 {
                set_word(ds, 0x7d3e, 0);
                actor[0x27] = actor[0x27].wrapping_add(1);
            } else if word(ds, 0x7d40) == 0 {
                actor[0x27] = 7;
                return (0, ev);
            }
            set_word(ds, 0x7d40, 0);
            ds[slot_at(slot)] = 1;
            (2, ev)
        }
        1 => {
            let r = world.random(100) as i16;
            if r >= (operand as i16).wrapping_add(attack) {
                actor[0x27] = actor[0x27].wrapping_add(1);
            }
            (2, ev)
        }
        2 => {
            let mut limit = 0x78u16.wrapping_sub((attack as u16).wrapping_mul(6));
            if ds[slot_at(slot)] & 2 != 0 {
                limit = limit.wrapping_add(0x168);
            }
            let r = world.random(limit);
            set_timer(actor, clock(ds).wrapping_add(u32::from(r)));
            (2, ev)
        }
        3 => {
            let t = clock(ds)
                .wrapping_add(0x32)
                .wrapping_sub(i32::from((attack as u16).wrapping_mul(2) as i16) as u32);
            set_timer(actor, t);
            actor[0x26] = (operand & 0x1f) as u8;
            (6, ev)
        }
        4 => {
            let kind = actor[0x0e];
            if matches!(kind, 2 | 3 | 7) && ds[ra + 0x2a] < 7 {
                ds[slot_at(slot)] = 0x20;
            }
            if matches!(kind, 2 | 3) {
                let rally = u32::from(word(ds, 0x5e41)) | u32::from(word(ds, 0x5e43)) << 16;
                if rally > clock(ds) {
                    actor[0x27] = 7;
                    ds[slot_at(slot)] = 2;
                }
            }
            if ds[ra + 0x2c] >= 2 && ds[slot_at(slot)] != 0x20 {
                return (2, ev);
            }
            if matches!(kind, 6 | 9 | 10 | 11) {
                return (2, ev);
            }
            if ds[ra + 0x2f] >= 6 && ds[slot_at(slot)] != 0x20 {
                ds[slot_at(slot)] = 2;
                return (2, ev);
            }
            if let Some(t) = target.as_deref_mut() {
                t[0x27] = 0;
                set_timer(t, 0);
            }
            ds[slot_at(slot)] = 0x10;
            actor[0x27] = 0x18;
            set_timer(actor, 0);
            (0, ev)
        }
        5 => {
            let tid = actor[0x2e];
            if target.is_none() {
                return (2, ev);
            }
            ev.push(CombatEvent::Rout { actor: false, flag: 1 });
            if world.rout_check() {
                if slot as i16 >= 0x14 {
                    return (2, ev);
                }
                ev.push(CombatEvent::Message(0x61b4));
                ds[slot_at(slot)] = 0;
                actor[0x28] = 0x14;
                actor[0x2e..0x30].fill(0);
                return (2, ev);
            }
            let kind = actor[0x0e];
            ev.push(CombatEvent::Message(if matches!(kind, 1 | 4 | 5 | 0x0c) { 0x6177 } else { 0x6193 }));
            if world.location_of(ds, aid) != 6 {
                set_word(ds, ra + 0x10, word(ds, ra + 0x10).wrapping_sub(2));
            }
            let mut stale = 0i16;
            ev.push(CombatEvent::Disposition { a: tid, b: aid, delta: 0 });
            if fun_26c5_041b(ds, tid, aid, 0) > 5 {
                ev.push(CombatEvent::Disposition { a: tid, b: aid, delta: -2 });
                fun_26c5_041b(ds, tid, aid, -2);
                stale = -2;
            }
            // Called with two arguments: the delta is the stack's leftover.
            ev.push(CombatEvent::Disposition { a: aid, b: tid, delta: stale });
            if fun_26c5_041b(ds, aid, tid, stale) > 5 {
                ev.push(CombatEvent::Disposition { a: aid, b: tid, delta: -2 });
                fun_26c5_041b(ds, aid, tid, -2);
            }
            actor[0x27] = actor[0x27].wrapping_add(1);
            (2, ev)
        }
        6 => attack_step(ds, actor, target, operand, attack, stale_bp10, world, ev),
        7 => {
            ev.push(CombatEvent::BattleStart);
            (2, ev)
        }
        _ => {
            let tid = actor[0x2e];
            let magic = i16::from(ds[ra + 0x2a]);
            let chance = (magic + 100 - i16::from(ds[rec(tid) + 0x24])) as u16;
            if magic > 1 {
                ds[ra + 0x2a] -= 1;
            }
            if world.random(100) < chance {
                actor[0x27] = actor[0x27].wrapping_add(1);
            } else {
                ev.push(CombatEvent::Message(0x61d1));
            }
            (2, ev)
        }
    }
}

/// Opcode 6 (`20b5:22F4`).
#[allow(clippy::too_many_arguments)]
fn attack_step(
    ds: &mut [u8],
    actor: &mut Widget,
    target: Option<&mut Widget>,
    operand: u16,
    attack: i16,
    stale_bp10: u16,
    world: &mut impl CombatWorld,
    mut ev: Vec<CombatEvent>,
) -> (u8, Vec<CombatEvent>) {
    let magic = operand & 0x80 != 0;
    let finish = |ev| (if magic { 2 } else { 0 }, ev);
    let aid = actor[0x0a];
    let tid = actor[0x2e];
    let Some(t) = target else {
        return finish(ev);
    };
    let rt = rec(tid);
    let tslot = ww(t, 0x34);
    let roll = (world.random(100) & 0xff) as i16;
    let mut chance = (operand & 0x7f) as i16 - i16::from(ds[rt + 0x2e]);
    if !magic {
        chance += attack - i16::from(ds[rt + 0x2d]);
    }
    let damage: u8 = if roll < chance {
        let grave = (chance >> 2) > roll;
        t[0x27] = if grave { 10 } else { 9 };
        ev.push(CombatEvent::Sound(if magic { 10 } else { 3 }));
        if grave { 2 } else { 1 }
    } else {
        if (chance << 1) > roll {
            t[0x27] = 8;
        }
        if !magic {
            actor[0x27] = actor[0x27].wrapping_add(1);
            ev.push(CombatEvent::Sound(2));
        }
        0
    };
    set_timer(t, 0);
    let flags = |ds: &[u8], id: u8| word(ds, rec(id) + 0x16);
    let reward = if magic { 0x25 } else { 0x2d };

    if ds[rt + 0x2c] > damage {
        ds[rt + 0x2c] -= damage;
        if word(ds, 0x9c56) & 4 != 0 && damage != 0 {
            // A contest ends at the first wound.
            ev.push(CombatEvent::UnitHit { id: tid, slot: tslot });
            ev.push(CombatEvent::Message(0x6209));
            t[0x27] = 0x0b;
            ds[slot_at(tslot)] = 8;
            let tw_id = ww(t, 0x0a) as u8;
            if flags(ds, tw_id) & 0x200 == 0 && flags(ds, aid) & 0x200 != 0 {
                ev.push(CombatEvent::StatChange { stat: 0x2f, id: aid });
            }
            let judge = stale_bp10 as u8;
            if ds[rec(judge) + 0x2f] >= 10 {
                ev.push(CombatEvent::Disposition { a: judge, b: tw_id, delta: 0 });
                if fun_26c5_041b(ds, judge, tw_id, 0) < 0x5f {
                    ev.push(CombatEvent::Disposition { a: judge, b: tw_id, delta: 1 });
                    fun_26c5_041b(ds, judge, tw_id, 1);
                }
                ev.push(CombatEvent::Disposition { a: tw_id, b: judge, delta: 0 });
                if fun_26c5_041b(ds, tw_id, judge, 0) < 0x5f {
                    ev.push(CombatEvent::Disposition { a: tw_id, b: judge, delta: 2 });
                    fun_26c5_041b(ds, tw_id, judge, 2);
                }
            }
            ev.push(CombatEvent::Rout { actor: true, flag: 0 });
            ev.push(CombatEvent::StatChange { stat: reward, id: aid });
            return finish(ev);
        }
        let index = if magic {
            usize::from(damage) + 0x0c
        } else {
            usize::from(world.random(4)) + usize::from(damage) * 4
        };
        ev.push(CombatEvent::Message(word(ds, 0x5dca + index * 4)));
        if damage != 0 {
            ev.push(CombatEvent::UnitHit { id: tid, slot: tslot });
        }
        return (2, ev);
    }

    // Killed.
    ds[rt + 0x2c] = 0;
    let f = flags(ds, tid) & 0xfff7;
    set_word(ds, rt + 0x16, f);
    ev.push(CombatEvent::Message(0x61fd));
    ev.push(CombatEvent::Mark { slot: tslot });
    t[0x27] = 0x0c;
    ds[slot_at(tslot)] = 4;
    ev.push(CombatEvent::Rout { actor: true, flag: word(ds, 0x9c56) & 1 });
    if ds[0x032d] == 0 && flags(ds, aid) & 0x200 != 0 {
        ev.push(CombatEvent::StatChange { stat: reward, id: aid });
    }
    ev.push(CombatEvent::DeadUnit(tid));
    finish(ev)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Replay {
        rng: Vec<u16>,
        rout: Vec<bool>,
    }

    impl CombatWorld for Replay {
        fn random(&mut self, _n: u16) -> u16 {
            assert!(!self.rng.is_empty(), "port drew a random number the original did not");
            self.rng.remove(0)
        }
        fn rout_check(&mut self) -> bool {
            assert!(!self.rout.is_empty(), "port asked for a rout check the original did not make");
            self.rout.remove(0)
        }
        fn location_of(&self, ds: &[u8], id: u8) -> u8 {
            crate::fight::fun_2d65_0a2a(ds, &[], id)
        }
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    fn apply(ds: &mut [u8], state: &serde_json::Value) {
        let chars = hex(state["chars"].as_str().unwrap());
        ds[CHAR_NAME..CHAR_NAME + chars.len()].copy_from_slice(&chars);
        for (addr, bytes) in state["globals"].as_object().unwrap() {
            let at = usize::from_str_radix(addr, 16).unwrap();
            let b = hex(bytes.as_str().unwrap());
            ds[at..at + b.len()].copy_from_slice(&b);
        }
    }

    fn widget(v: &serde_json::Value) -> Option<Widget> {
        v.as_str().map(|s| hex(s).try_into().unwrap())
    }

    /// The delegated calls the recorder sees, in the recorder's terms.
    fn subcalls(ev: &[CombatEvent]) -> Vec<String> {
        ev.iter()
            .filter_map(|e| match e {
                CombatEvent::UnitHit { id, slot } => Some(format!("remove_unit {id} {slot}")),
                CombatEvent::Rout { flag, .. } => Some(format!("rout {flag}")),
                CombatEvent::DeadUnit(id) => Some(format!("dead_unit {id}")),
                CombatEvent::StatChange { stat, id } => Some(format!("stat_change {stat} {id} 1")),
                CombatEvent::Disposition { a, b, delta } => {
                    Some(format!("disposition {a} {b} {}", *delta as u16))
                }
                CombatEvent::Mark { .. } => Some("mark".to_string()),
                _ => None,
            })
            .collect()
    }

    fn recorded(call: &serde_json::Value) -> Vec<String> {
        call["subcalls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                let s = s.as_array().unwrap();
                let name = s[0].as_str().unwrap();
                let n = |k: usize| s[k].as_u64().unwrap();
                match name {
                    "remove_unit" => format!("remove_unit {} {}", n(1) & 0xff, n(2)),
                    "rout" => format!("rout {}", n(3)),
                    "dead_unit" => format!("dead_unit {}", n(1) & 0xff),
                    "stat_change" => format!("stat_change {} {} {}", n(1), n(2) & 0xff, n(3)),
                    "disposition" => format!("disposition {} {} {}", n(1) & 0xff, n(2) & 0xff, n(3)),
                    _ => "mark".to_string(),
                }
            })
            .collect()
    }

    /// Replays combat steps recorded in the original by
    /// `tests/oracle/battle_vectors.py`.
    #[test]
    fn matches_original_combat_steps() {
        let Ok(text) = std::fs::read_to_string("../../tests/vectors/battle_step.json") else {
            eprintln!("SKIP: tests/vectors/battle_step.json absent");
            return;
        };
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let base = hex(v["dgroup"].as_str().unwrap());
        let mut by_op = [0usize; 9];
        for (n, call) in v["calls"].as_array().unwrap().iter().enumerate() {
            let op = call["op"].as_u64().unwrap() as u16;
            let mut ds = base.clone();
            apply(&mut ds, &call["before"]);
            let mut actor = widget(&call["before"]["actor"]).unwrap();
            let mut target = widget(&call["before"]["target"]);
            let mut world = Replay {
                rng: call["rng"].as_array().unwrap().iter().map(|r| r.as_u64().unwrap() as u16).collect(),
                rout: call["rout_ret"].as_array().unwrap().iter().map(|r| r.as_u64().unwrap() != 0).collect(),
            };
            let stale = call["stale_bp10"].as_u64().unwrap() as u16;
            let (ret, ev) = fun_20b5_1dff(&mut ds, &mut actor, target.as_mut(), op, stale, &mut world);
            let ctx = format!("call {n} op {:#06x}", op);
            assert_eq!(u64::from(ret), call["ret"].as_u64().unwrap() & 0xff, "{ctx}: return");
            assert!(world.rng.is_empty(), "{ctx}: unused random numbers {:?}", world.rng);
            let msgs: Vec<u64> = ev
                .iter()
                .filter_map(|e| if let CombatEvent::Message(m) = e { Some(u64::from(*m)) } else { None })
                .collect();
            let want: Vec<u64> = call["messages"].as_array().unwrap().iter().map(|m| m.as_u64().unwrap()).collect();
            assert_eq!(msgs, want, "{ctx}: messages");
            assert_eq!(subcalls(&ev), recorded(call), "{ctx}: delegated calls");

            // Delegated routines change state too; compare only what this
            // function alone is responsible for.
            let delegated = ev.iter().any(|e| {
                matches!(e, CombatEvent::Rout { .. } | CombatEvent::UnitHit { .. } | CombatEvent::DeadUnit(_)
                    | CombatEvent::StatChange { .. } | CombatEvent::Mark { .. })
            });
            let mut want_ds = base.clone();
            apply(&mut want_ds, &call["after"]);
            if !delegated {
                assert_eq!(&ds[CHAR_NAME..CHAR_NAME + 0x5d * 0x38 + 0x38],
                    &want_ds[CHAR_NAME..CHAR_NAME + 0x5d * 0x38 + 0x38], "{ctx}: records");
                assert_eq!(Some(actor), widget(&call["after"]["actor"]), "{ctx}: actor widget");
                assert_eq!(target, widget(&call["after"]["target"]), "{ctx}: target widget");
            }
            assert_eq!(&ds[SLOTS..SLOTS + 0x28], &want_ds[SLOTS..SLOTS + 0x28], "{ctx}: slot bytes");
            assert_eq!(&ds[0x7d3e..0x7d42], &want_ds[0x7d3e..0x7d42], "{ctx}: order flags");
            by_op[usize::from(op >> 8).min(8)] += 1;
        }
        eprintln!("combat steps matched by opcode: {by_op:?}");
    }
}
