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
    /// `FUN_1ba8_2611(stat, id, delta)`.
    StatChange { stat: u16, id: u8, delta: u16 },
    /// `FUN_15b8_11ac`: the dead mark next to a list entry.
    Mark { slot: u16 },
    /// `FUN_26c5_041b(a, b, delta)` called directly.
    Disposition { a: u8, b: u8, delta: i16 },
    /// `FUN_20b5_1705(widget, flag)` for the acting (`actor`) or target widget.
    Rout { actor: bool, flag: u16 },
    /// Opcode 7: the battle scene starts.
    BattleStart,
    /// `FUN_1316_187b(name, number, 0)`: start a music piece (name is a
    /// DGROUP string offset).
    Music { name: u16, number: u16 },
    /// A battle UI call (`FUN_1772_0623`, `FUN_15b8_1876(2)`,
    /// `FUN_1772_072b`), by offset.
    Ui(u16),
    /// `FUN_26c5_000b(a, b)`: a story event raised by a death.
    StoryEvent { a: u8, b: u16 },
    /// `FUN_1ba8_01ff(item, owner)`: a dead unit dropped an item.
    ItemDropped { item: u8, owner: u8 },
    /// `FUN_1b06_06b5` / `FUN_1b06_071c` on the map, with their arguments.
    MapEvent { call: u16, args: [u16; 3] },
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

/// `FUN_20b5_1229`: bookkeeping for dead character `id`. Followers
/// (`+0x31`) pass to the first of them, who takes the dead leader's
/// position (and, with the same `0x700` allegiance, its target, `+0x18` and
/// `+0x32..+0x37`); anyone targeting it (`+0x18`) stops. A dead leader's
/// map position becomes its top leader's (`FUN_2d65_09a2`) or 0 when still
/// standing. Flag 8 (in the field), strength `+0x1E` and the leader link
/// are cleared. The first matching death event in the table at `DS:46AB`
/// (6-byte entries) is raised (`FUN_26c5_000b`) and may end the battle.
pub fn fun_20b5_1229(ds: &mut [u8], id: u8, ev: &mut Vec<CombatEvent>) -> u8 {
    let r = rec(id);
    let mut heir = ds[r + 0x31];
    for l in 1..0x5du8 {
        let rl = rec(l);
        if ds[rl + 0x18] == id {
            ds[rl + 0x18] = 0;
        }
        if ds[rl + 0x31] == id {
            if heir == 0 {
                heir = l;
                ds[rl + 0x31] = 0;
                let pos = [word(ds, r + 0x0e), word(ds, r + 0x10)];
                set_word(ds, rl + 0x0e, pos[0]);
                set_word(ds, rl + 0x10, pos[1]);
                if word(ds, rl + 0x16) & 0x700 == word(ds, r + 0x16) & 0x700 {
                    let target = [word(ds, r + 0x12), word(ds, r + 0x14)];
                    set_word(ds, rl + 0x12, target[0]);
                    set_word(ds, rl + 0x14, target[1]);
                    ds[rl + 0x18] = ds[r + 0x18];
                    for k in 0..6 {
                        ds[rl + 0x32 + k] = ds[r + 0x32 + k];
                    }
                }
            } else {
                ds[rl + 0x31] = heir;
            }
        }
    }
    if ds[r + 0x2c] == 0 {
        if heir != 0 {
            // FUN_2d65_09a2: top of the leader chain.
            let mut top = id;
            while ds[rec(top) + 0x31] != 0 {
                top = ds[rec(top) + 0x31];
            }
            let pos = [word(ds, rec(top) + 0x0e), word(ds, rec(top) + 0x10)];
            set_word(ds, r + 0x0e, pos[0]);
            set_word(ds, r + 0x10, pos[1]);
        }
    } else {
        set_word(ds, r + 0x0e, 0);
        set_word(ds, r + 0x10, 0);
    }
    let f = word(ds, r + 0x16) & 0xfff7;
    set_word(ds, r + 0x16, f);
    set_word(ds, r + 0x1e, 0);
    ds[r + 0x31] = 0;
    if ds[0x9cad] == 0 {
        for l in 0..0x50usize {
            let e = 0x46ab + l * 6;
            let kind = word(ds, e + 2);
            if ds[e] == id && kind & 0x0f == 0 && kind & 0x8000 != 0 {
                ds[0x9cad] = l as u8;
                ev.push(CombatEvent::StoryEvent { a: ds[e + 1], b: word(ds, e + 4) });
                if kind & 0x80 != 0 {
                    ds[0x9cac] = 1;
                }
                return 1;
            }
        }
    }
    0
}

/// `FUN_20b5_1705(widget, flag)`: a unit left the fight (it won, or its
/// opponent fled). With `flag`, look on the other side for a new opponent:
/// units waiting (slot state 0x40) first, then idle ones (0). Kind 1, 4, 5
/// and 0x0C units are sent off (state 0x10, step 0x18); a unit either side
/// dislikes (`FUN_26c5_041b` < 0x28) is engaged (returns 1); on the foe
/// side the rest stand aside. If someone there is still fighting (state &
/// 3), the unit waits (step 0x14 for the party, 5 for foes). Otherwise the
/// battle is over: music, a 3-second close (`DS:0322`), and every unit's
/// outcome by its slot state (`CS:1A03` table): fighters stand down, the
/// dead get [`fun_20b5_1229`], the wounded and fled get stat changes and
/// map moves. `stale_bp1` is the uninitialised `[BP-1]` the end path reads
/// when `flag` is 0.
pub fn fun_20b5_1705(
    scene: &mut crate::script::ScriptScene,
    at: usize,
    flag: u8,
    stale_bp1: u8,
    ds: &mut [u8],
    world: &mut impl CombatWorld,
) -> (u8, Vec<CombatEvent>) {
    let mut ev = Vec::new();
    let aid = scene.widgets[at][0x0a];
    let aslot = ww(&scene.widgets[at], 0x34);
    let mut other_side = stale_bp1;
    let set = |w: &mut Widget, step: u8, target: Option<u16>| {
        w[0x27] = step;
        set_timer(w, 0);
        if let Some(t) = target {
            w[0x2e..0x30].copy_from_slice(&t.to_le_bytes());
        }
    };
    if flag != 0 {
        let count;
        if aslot < 0x14 {
            count = ds[0x9dad];
            other_side = 0x14;
        } else {
            count = ds[0x9dac];
            other_side = 0;
        }
        for want in [0x40u8, 0] {
            for c in 0..count {
                let s = usize::from(other_side) + usize::from(c);
                if ds[SLOTS + s] != want {
                    continue;
                }
                let uid = ds[0x9dc3 + s];
                let Some(w) = scene.find(0x200, 0, u16::from(uid)) else { continue };
                let kind = ds[rec(uid) + 0x26];
                if matches!(kind, 1 | 4 | 5 | 0x0c) {
                    ds[SLOTS + s] = 0x10;
                    set(&mut scene.widgets[w], 0x18, Some(u16::from(aid)));
                    continue;
                }
                if fun_26c5_041b(ds, aid, uid, 0) < 0x28 || fun_26c5_041b(ds, uid, aid, 0) < 0x28 {
                    ds[slot_at(aslot)] = 1;
                    set(&mut scene.widgets[at], 7, Some(u16::from(uid)));
                    ds[SLOTS + s] = 1;
                    set(&mut scene.widgets[w], 6, Some(u16::from(aid)));
                    return (1, ev);
                }
                if other_side != 0 {
                    ds[SLOTS + s] = 0x10;
                    set(&mut scene.widgets[w], 0x18, None);
                }
            }
        }
        for c in 0..count {
            if ds[SLOTS + usize::from(other_side) + usize::from(c)] & 3 != 0 {
                ds[slot_at(aslot)] = 0;
                let step = if other_side != 0 { 0x14 } else { 5 };
                set(&mut scene.widgets[at], step, Some(0));
                return (0, ev);
            }
        }
    }
    ds[0x9cac] = 1;
    set_word(ds, 0x9c58, 0);
    let close = clock(ds).wrapping_add(0xb4);
    ds[0x0322..0x0326].copy_from_slice(&close.to_le_bytes());
    ds[0x0326..0x032a].copy_from_slice(&close.to_le_bytes());
    if other_side != 0 {
        scene.widgets[at][0x27] = 0x14;
        ev.push(CombatEvent::Music { name: 0x6168, number: 10 });
    } else {
        scene.widgets[at][0x27] = 0;
        ev.push(CombatEvent::Music { name: 0x616f, number: 0x0b });
    }
    let reward = |ev: &mut Vec<CombatEvent>, stat: u16, id: u8, delta: u16| {
        ev.push(CombatEvent::Ui(0x0623));
        ev.push(CombatEvent::StatChange { stat, id, delta });
        ev.push(CombatEvent::Ui(0x1876));
        ev.push(CombatEvent::Ui(0x072b));
    };
    for side in [0u8, 0x14] {
        let count = if side == 0 { ds[0x9dac] } else { ds[0x9dad] };
        for c in 0..count {
            let s = usize::from(side) + usize::from(c);
            let uid = ds[0x9dc3 + s];
            let r = rec(uid);
            match ds[SLOTS + s] {
                st @ (0 | 1 | 2 | 0x40) => {
                    if matches!(st, 1 | 2) {
                        if let Some(w) = scene.find(0x200, 0, u16::from(uid)) {
                            scene.widgets[w][0x27] = if side == 0 { 0x14 } else { 0 };
                        }
                    }
                    if word(ds, r + 0x16) & 0x8200 == 0x200 {
                        ds[0x9cac] = 0;
                    }
                }
                4 => {
                    fun_20b5_1229(ds, uid, &mut ev);
                }
                8 => {
                    if word(ds, r + 0x16) & 0x200 == 0 {
                        reward(&mut ev, 0x2f, uid, 1);
                    }
                }
                0x10 | 0x20 => {
                    let other = ds[0x9dc3 + usize::from(0x14 - side)];
                    let d = fun_26c5_041b(ds, uid, other, 0);
                    if d < 0x32 && side == other_side {
                        let f = word(ds, r + 0x16) & 0x8fff;
                        set_word(ds, r + 0x16, f);
                        if ds[r + 0x31] == 0 {
                            if word(ds, r + 0x16) & 0x400 != 0 {
                                let x = word(ds, r + 0x0e);
                                set_word(ds, r + 0x12, x);
                                set_word(ds, r + 0x14, 0);
                            } else if world.location_of(ds, uid) == 6 && word(ds, r + 0x16) & 0x200 != 0 {
                                let extra = if uid <= 0x10 { 0x6000 } else { 0x1000 };
                                let f = (word(ds, r + 0x16) & 0x8fff) | extra;
                                set_word(ds, r + 0x16, f);
                                set_word(ds, r + 0x12, 0);
                                set_word(ds, r + 0x14, 0);
                                ev.push(CombatEvent::MapEvent {
                                    call: 0x071c,
                                    args: [u16::from(d) << 8 | u16::from(uid), u16::from(other), 0],
                                });
                            } else {
                                let dx = world.random(100).wrapping_sub(0x32) as i16;
                                let x = word(ds, r + 0x0e).wrapping_add(dx as u16);
                                set_word(ds, r + 0x12, x);
                                let dy = if world.random(2) != 0 {
                                    0x32 - dx.wrapping_abs()
                                } else {
                                    dx.wrapping_abs() - 0x32
                                };
                                let y = (dy as u16).wrapping_add(word(ds, r + 0x10));
                                set_word(ds, r + 0x14, y);
                                ev.push(CombatEvent::MapEvent {
                                    call: 0x06b5,
                                    args: [u16::from(d) << 8 | u16::from(uid), u16::from(other), 0x14],
                                });
                            }
                        }
                        if ds[r + 0x26] == 0 {
                            reward(&mut ev, 0x2f, uid, 0xfffe);
                        }
                    } else {
                        if let Some(w) = scene.find(0x200, 0, u16::from(uid)) {
                            set(&mut scene.widgets[w], 0x15, Some(u16::from(other)));
                        }
                        if side == other_side && ds[0x032d] == 0 {
                            reward(&mut ev, 0x2f, other, 1);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    ds[0x01ee] = 1;
    (0, ev)
}

/// A scene host that also owns DGROUP, for [`fun_20b5_1057`].
pub trait DropHost: crate::script::ScriptHost {
    fn dgroup(&mut self) -> &mut [u8];
}

/// `FUN_20b5_1057(id)`: a dead unit drops what it carries. Only when drops
/// are on (`DS:9DF6`), the unit's hit points (`+0x2C`) are 0 and it still
/// has a widget. Each item (`DS:4DD1`, 4 bytes: item, owner, -, state)
/// owned by it with state bits `0x18` and no `0xF000` item flags
/// (`DS:488B + 0x1B*item`, `+0x18`) drops with a 40% chance: it leaves the
/// inventory (`FUN_1ba8_01ff`, returned as [`CombatEvent::ItemDropped`])
/// and lies on the field as a static image (resource `+0x12`) at the
/// unit's x plus `rand(20)`, standing on the ground line (`+0x15` plus
/// `DS:79F7[DS:9DAB]`; 10 higher for resources 0x328, 0x333 and 0x338).
pub fn fun_20b5_1057(scene: &mut crate::script::ScriptScene, id: u8, host: &mut dyn DropHost) -> Vec<CombatEvent> {
    let mut ev = Vec::new();
    {
        let ds = host.dgroup();
        if ds[0x9df6] == 0 || ds[rec(id) + 0x2c] != 0 {
            return ev;
        }
    }
    let Some(w) = scene.find(0x200, 0, u16::from(id)) else {
        return ev;
    };
    let x = ww(&scene.widgets[w], 0x12);
    for i in 0..0x55usize {
        let (item, owner, state) = {
            let ds = host.dgroup();
            (ds[0x4dd1 + i * 4], ds[0x4dd1 + i * 4 + 1], ds[0x4dd1 + i * 4 + 3])
        };
        let def = 0x488b + usize::from(item) * 0x1b;
        if state & 0x18 != 0x18 || owner != id || word(host.dgroup(), def + 0x18) & 0xf000 != 0 {
            continue;
        }
        if host.random(100) >= 0x28 {
            continue;
        }
        ev.push(CombatEvent::ItemDropped { item, owner: id });
        let res = word(host.dgroup(), def + 0x12);
        // FUN_14b0_0122 / FUN_14b0_02c4: the image height, then release it.
        let (_, _, height) = host.static_image(res);
        let ground = {
            let ds = host.dgroup();
            ds[def + 0x15].wrapping_add(ds[0x79f7 + usize::from(ds[0x9dab])])
        };
        let depth = if matches!(res, 0x328 | 0x333 | 0x338) { ground.wrapping_sub(10) } else { ground };
        let y = u16::from(ground).wrapping_sub(height);
        let dx = host.random(0x14);
        let args = crate::script::SpawnArgs {
            flags: 0x20,
            res,
            id: res,
            script: res as u8,
            depth,
            x: dx.wrapping_add(x),
            y,
            ..Default::default()
        };
        let at = scene.fun_15b8_0461(&args, host);
        let f = ww(&scene.widgets[at], 0) | 2;
        scene.widgets[at][0..2].copy_from_slice(&f.to_le_bytes());
    }
    ev
}

/// Opcode 7 of `FUN_20b5_1dff` (`20b5:2769`): the battle starts. Once per
/// battle (`DS:5E37` != 1, then incremented) it advances the actor, shows
/// the battle panel (`FUN_15b8_09f7(0x31E)`, `FUN_20b5_085b`: returned as
/// [`CombatEvent::BattleStart`]) and message `0x621D`, clears the slot
/// flags (`DS:9C82` party, `DS:9C96` foes), and sends the two champions
/// (slots `DS:9C52`/`9C53`, ids `DS:9C4E`/`9C4F`) at each other: script
/// step 6, timer 0, `+2E` = the other. In an army fight (`DS:9C56` bit 0)
/// whose meeting pair includes a champion, the remaining living units
/// (character flag 8) are paired off in list order. If party slot 0 is
/// still free, its unit goes to step 0x14.
fn fun_20b5_2769(scene: &mut crate::script::ScriptScene, ds: &mut [u8]) -> (u8, Vec<CombatEvent>) {
    let mut ev = Vec::new();
    if ds[0x5e37] == 1 {
        return (2, ev);
    }
    ds[0x5e37] = ds[0x5e37].wrapping_add(1);
    let cur = scene.current as usize;
    scene.widgets[cur][0x27] = scene.widgets[cur][0x27].wrapping_add(1);
    ev.push(CombatEvent::BattleStart);
    ev.push(CombatEvent::Message(0x621d));
    for i in 0..usize::from(ds[0x9dac]) {
        ds[SLOTS + i] = 0;
    }
    for i in 0..usize::from(ds[0x9dad]) {
        ds[0x9c96 + i] = 0;
    }
    let alive = |ds: &[u8], id: u8| word(ds, rec(id) + 0x16) & 8 != 0;
    let send = |scene: &mut crate::script::ScriptScene, at: usize, step: u8, target: Option<u8>| {
        let w = &mut scene.widgets[at];
        w[0x27] = step;
        set_timer(w, 0);
        if let Some(t) = target {
            w[0x2e..0x30].copy_from_slice(&u16::from(t).to_le_bytes());
        }
    };
    for (slot, other) in [(ds[0x9c52], ds[0x9c4f]), (ds[0x9c53], ds[0x9c4e])] {
        ds[SLOTS + usize::from(slot)] = 1;
        let id = ds[0x9dc3 + usize::from(slot)];
        if let Some(at) = scene.find(0x200, 0, u16::from(id)) {
            send(scene, at, 6, Some(other));
        }
    }
    if word(ds, 0x9c56) & 1 != 0 && (ds[0x9dae] == ds[0x9c4e] || ds[0x9db0] == ds[0x9c4f]) {
        let (mut i, mut j) = (0usize, 0usize);
        while usize::from(ds[0x9dac]) > i && usize::from(ds[0x9dad]) > j {
            if ds[SLOTS + i] != 0 || ds[0x9c96 + j] != 0 {
                i += 1;
                continue;
            }
            ds[SLOTS + i] = 1;
            let a = ds[0x9dc3 + i];
            let Some(at) = scene.find(0x200, 0, u16::from(a)) else { continue };
            if !alive(ds, a) {
                continue;
            }
            let b = ds[0x9dd7 + j];
            send(scene, at, 6, Some(b));
            ds[0x9c96 + j] = 1;
            let Some(bt) = scene.find(0x200, 0, u16::from(b)) else { continue };
            if !alive(ds, b) {
                continue;
            }
            send(scene, bt, 6, Some(a));
            i += 1;
            j += 1;
        }
    }
    if ds[SLOTS] == 0 {
        let a = ds[0x9dc3];
        if let Some(at) = scene.find(0x200, 0, u16::from(a)) {
            if alive(ds, a) {
                send(scene, at, 0x14, None);
            }
        }
    }
    (0, ev)
}

/// Script handler 15 (installed in `DS:0389` at run time): run
/// [`fun_20b5_1dff`] for the scene's current widget against the widget
/// `FUN_15b8_16ca(0x200, 0, actor[+0x2E])` finds. A widget targeting itself
/// is run without a target (the original would alias the two; not seen).
/// When the step decides the actor leaves the fight (a kill, a concession),
/// [`fun_20b5_1705`] follows. Opcode 5 still asks `world.rout_check()` for
/// the target.
pub fn script_hook(
    scene: &mut crate::script::ScriptScene,
    op: u16,
    ds: &mut [u8],
    stale_bp10: u16,
    world: &mut impl CombatWorld,
) -> (u8, Vec<CombatEvent>) {
    let cur = scene.current as usize;
    if op >> 8 == 7 {
        return fun_20b5_2769(scene, ds);
    }
    let tid = ww(&scene.widgets[cur], 0x2e);
    let target = scene.find(0x200, 0, tid).filter(|&t| t != cur);
    let (ret, events) = match target {
        Some(t) if t < cur => {
            let (lo, hi) = scene.widgets.split_at_mut(cur);
            fun_20b5_1dff(ds, &mut hi[0], Some(&mut lo[t]), op, stale_bp10, world)
        }
        Some(t) => {
            let (lo, hi) = scene.widgets.split_at_mut(t);
            fun_20b5_1dff(ds, &mut lo[cur], Some(&mut hi[0]), op, stale_bp10, world)
        }
        None => fun_20b5_1dff(ds, &mut scene.widgets[cur], None, op, stale_bp10, world),
    };
    // FUN_20b5_1705(actor, flag) runs inside the step; nothing the step does
    // after it depends on what it changes, so it runs here, in event order.
    // With flag 0 it reads an uninitialised [BP-1] (the side the battle is
    // decided for); the actor's opposite side is used.
    let mut out = Vec::with_capacity(events.len());
    for e in events {
        let rout = matches!(e, CombatEvent::Rout { actor: true, .. });
        let flag = if let CombatEvent::Rout { flag, .. } = e { flag as u8 } else { 0 };
        out.push(e);
        if rout {
            let stale = if ww(&scene.widgets[cur], 0x34) < 0x14 { 0x14 } else { 0 };
            let (_, more) = fun_20b5_1705(scene, cur, flag, stale, ds, world);
            out.extend(more);
        }
    }
    (ret, out)
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
                ev.push(CombatEvent::StatChange { stat: 0x2f, id: aid, delta: 1 });
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
            ev.push(CombatEvent::StatChange { stat: reward, id: aid, delta: 1 });
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
        ev.push(CombatEvent::StatChange { stat: reward, id: aid, delta: 1 });
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
                CombatEvent::StatChange { stat, id, delta } => Some(format!("stat_change {stat} {id} {delta}")),
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

    /// Replays FUN_20b5_1705 calls (with FUN_20b5_1229 inside) recorded in
    /// the original by `tests/oracle/rout_vectors.py`.
    #[test]
    fn matches_original_rout_calls() {
        let Ok(text) = std::fs::read_to_string("../../tests/vectors/rout_calls.json") else {
            eprintln!("SKIP: tests/vectors/rout_calls.json absent");
            return;
        };
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let mut done = 0;
        for (n, call) in v["calls"].as_array().unwrap().iter().enumerate() {
            let state = |k: &str| -> (Vec<u8>, Vec<Widget>, i16) {
                let st = &call[k];
                let widgets = st["widgets"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|w| hex(w.as_str().unwrap()).try_into().unwrap())
                    .collect();
                (hex(st["ds"].as_str().unwrap()), widgets, st["current"].as_u64().unwrap() as i16)
            };
            let (mut ds, widgets, current) = state("before");
            let (want_ds, want_widgets, _) = state("after");
            let mut scene = crate::script::ScriptScene { widgets, current, ..Default::default() };
            let mut world = Replay {
                rng: call["rng"].as_array().unwrap().iter().map(|r| r.as_u64().unwrap() as u16).collect(),
                rout: Vec::new(),
            };
            let at = call["widget"].as_u64().unwrap() as usize;
            let flag = call["flag"].as_u64().unwrap() as u8;
            let stale = call["stale_bp1"].as_u64().unwrap() as u8;
            let (ret, ev) = fun_20b5_1705(&mut scene, at, flag, stale, &mut ds, &mut world);
            let ctx = format!("call {n}");
            assert_eq!(u64::from(ret), call["ret"].as_u64().unwrap(), "{ctx}: return");
            assert!(world.rng.is_empty(), "{ctx}: unused random numbers");
            for (i, (a, b)) in scene.widgets.iter().zip(&want_widgets).enumerate() {
                assert_eq!(a.as_slice(), b.as_slice(), "{ctx}: widget {i}");
            }
            // Compare what 1705/1229 write. Calls they hand off (music, the
            // story event FUN_26c5_000b, UI) and the clock interrupt change
            // other bytes during the call.
            let units: Vec<u8> = (0..usize::from(ds[0x9dac]))
                .map(|i| ds[0x9dc3 + i])
                .chain((0..usize::from(ds[0x9dad])).map(|i| ds[0x9dd7 + i]))
                .collect();
            let mut owned: Vec<usize> = Vec::new();
            for g in [0x9cac, 0x9cad, 0x9c58, 0x9c59, 0x01ee] {
                owned.push(g);
            }
            owned.extend(0x0322..0x032a);
            owned.extend(SLOTS..SLOTS + 0x28);
            // Characters: the units, their followers and anyone targeting
            // them (the story event may change others).
            let involved = |id: u8| {
                let r = rec(id);
                units.contains(&id) || units.contains(&ds[r + 0x31]) || units.contains(&ds[r + 0x18])
            };
            for id in (1..0x5du8).filter(|&id| involved(id)) {
                let r = rec(id);
                for k in [0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x18, 0x1e, 0x1f, 0x31] {
                    owned.push(r + k);
                }
                owned.extend(r + 0x32..r + 0x38);
                if units.contains(&id) {
                    owned.extend([r + 0x16, r + 0x17]);
                }
            }
            for &i in &owned {
                assert_eq!(ds[i], want_ds[i], "{ctx}: DS:{i:04X}");
            }
            let got: Vec<String> = ev
                .iter()
                .filter_map(|e| match e {
                    CombatEvent::Music { name, number } => Some(format!("music {name} {} {number} 0", 0x2578)),
                    CombatEvent::StatChange { stat, id, delta } => Some(format!("stat_change {stat} {id} {delta}")),
                    CombatEvent::Ui(0x0623) => Some("ui_0623".into()),
                    CombatEvent::Ui(0x072b) => Some("ui_072b".into()),
                    CombatEvent::Ui(0x1876) => Some("ui_1876 2".into()),
                    CombatEvent::StoryEvent { a, b } => Some(format!("story_event {a} {b}")),
                    CombatEvent::MapEvent { call: 0x071c, args } => Some(format!("map_071c {} {}", args[0], args[1])),
                    CombatEvent::MapEvent { call: _, args } => Some(format!("map_06b5 {} {} {}", args[0], args[1], args[2])),
                    _ => None,
                })
                .collect();
            let want: Vec<String> = call["subcalls"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| {
                    let s = s.as_array().unwrap();
                    let mut out = s[0].as_str().unwrap().to_string();
                    for a in &s[1..] {
                        out += &format!(" {}", a.as_u64().unwrap());
                    }
                    out
                })
                .collect();
            assert_eq!(got, want, "{ctx}: outside calls");
            done += 1;
        }
        eprintln!("rout calls matched: {done}");
        assert!(done > 0);
    }
}
