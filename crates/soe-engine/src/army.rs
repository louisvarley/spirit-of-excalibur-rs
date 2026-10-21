//! Army battle, ported from segment `2449` (`FUN_2449_134e` and helpers).
//!
//! An army battle is a strength contest shown on a small battlefield: one
//! token widget per unit (`FUN_2449_0aed`), moved by scripts 0x0D-0x14 on
//! the ported scene engine, with script handler 15 set to
//! `FUN_2449_12de` (the duel's handler 15 is the combat step instead).
//! `FUN_2449_134e` waits for the armies to meet (`FUN_2449_0255` keeps
//! each side's front and points the other side's tokens at it), then
//! resolves rounds until `FUN_2449_1021` decides a rout or a side is gone.
//!
//! State lives in DGROUP as in the original: the side lists `DS:9DC3` /
//! `DS:9DD7` and counts `DS:9DAC` / `DS:9DAD`, the slot states `DS:9C82`
//! (1 engage, 2 charging, 0x40/0x80 after a charge, 3/5 flanking, 4 out,
//! 6 retreating, 0x10 withdrawn to the walls), each slot's commander
//! `DS:9C5A` and battlefield place `DS:9CE1`, the place table `DS:64B8`
//! (x, y, taken, first script step), the strength totals `DS:9CC2..9CC9`,
//! the token images `DS:9CCA` and the UI flags `DS:9C58` (message wait),
//! `DS:9CB0` (order menu open), `DS:9CAE`, `DS:9CDA` (fight a round now),
//! `DS:9D09` / `DS:9D0B` (the two fronts) and `DS:9D0E` / `DS:9D0F`.
//!
//! Calls into routines that are not ported here (messages, the panel,
//! music, map moves, UI waits) are returned as [`ArmyEvent`]s in order.
//! The 134e loop itself is [`ArmyBattle`]; its round body is
//! [`fun_2449_134e_round`].

#![forbid(unsafe_code)]

use crate::combat::{self, CombatEvent};
use crate::script::{ScriptHost, ScriptScene, SpawnArgs, Widget};
use crate::text::Arg;

const CHAR: usize = 0x2c5e;
const CHAR_SIZE: usize = 0x38;
pub const LIST: usize = 0x9dc3;
pub const COUNT1: usize = 0x9dac;
pub const COUNT2: usize = 0x9dad;
pub const SLOTS: usize = 0x9c82;
const COMMANDERS: usize = 0x9c5a;
const PLACES: usize = 0x9ce1;
const PLACE_TABLE: usize = 0x64b8;
const IMAGES: usize = 0x9cca;
const MODE: usize = 0x9c56;
const CLOCK: usize = 0x7e3e;
const GAME_CLOCK: usize = 0x4fbf;
/// `DS:9C58`: the message wait the round loop sets before each pause.
pub const WAIT_MESSAGE: u16 = 0x9c58;
/// `DS:9CB0`: the order menu is open.
pub const WAIT_MENU: u16 = 0x9cb0;
/// `DS:9CAE`: waiting for "Do Round".
pub const WAIT_ROUND: u16 = 0x9cae;
/// `DS:031E`: the close-down wait at the end of the battle.
pub const WAIT_CLOSE: u16 = 0x031e;
/// First token image resource (`FUN_2449_0aed` loads 0x2EE..=0x2F1).
pub const TOKEN_IMAGES: u16 = 0x2ee;

/// What the original does outside the ported code, in call order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArmyEvent {
    /// `FUN_20b5_0000(format, ...)`: a line in the message stream.
    Message {
        format: u16,
        args: Vec<Arg>,
    },
    /// `FUN_2449_0053(format)`: a message box (an outcome).
    Notice(u16),
    /// `FUN_15b8_09f7(flag)`: run the UI until the DGROUP word is 0.
    Wait(u16),
    /// `FUN_15b8_11ac(x, y, code)`: the mark next to a panel row.
    Mark {
        x: u16,
        y: u16,
        code: u16,
    },
    /// `FUN_20b5_0140(id, slot)`: redraw a unit's panel row.
    UnitRow {
        id: u8,
        slot: u16,
    },
    /// `FUN_14b0_0bbf` + `FUN_14b0_03f5(.., "%d %S%c", ..)`: an army
    /// stack's strength redrawn in its panel row.
    Strength {
        slot: u16,
    },
    /// `FUN_1316_187b(name, number, flag)`.
    Music {
        name: u16,
        number: u16,
        flag: u16,
    },
    /// `FUN_1b06_071c(side1, side2)`: the armies move apart on the map
    /// (the first list word of each side).
    MapMove(u16, u16),
    /// `FUN_1772_000e` in a battle fought off screen: the losses box.
    Summary,
    /// `FUN_14b0_0bbf` from `FUN_2449_0a70`: the order column cleared.
    ClearOrders,
    /// `FUN_17f3_003a` / `FUN_17f3_0086`.
    MouseOff,
    MouseOn,
    /// Something `FUN_20b5_1229` raised.
    Combat(CombatEvent),
}

/// Inputs from the rest of the game.
pub trait ArmyWorld {
    /// `FUN_15b8_17e0(n)`.
    fn random(&mut self, n: u16) -> u16;
    /// `FUN_2449_0107`: "Your losses are heavy. Fall back to the walls?"
    /// (the answer `DS:9CDF`; 1 without a screen).
    fn fall_back(&mut self, ds: &[u8]) -> u16;
}

fn rec(id: u8) -> usize {
    CHAR + usize::from(id) * CHAR_SIZE
}

fn word(ds: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([ds[at], ds[at + 1]])
}

fn set_word(ds: &mut [u8], at: usize, v: u16) {
    ds[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

fn long(ds: &[u8], at: usize) -> u32 {
    u32::from(word(ds, at)) | u32::from(word(ds, at + 2)) << 16
}

fn set_long(ds: &mut [u8], at: usize, v: u32) {
    ds[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn ww(w: &Widget, at: usize) -> u16 {
    u16::from_le_bytes([w[at], w[at + 1]])
}

fn auto(ds: &[u8]) -> bool {
    word(ds, MODE) & 0x20 != 0
}

fn count(ds: &[u8], side: usize) -> usize {
    usize::from(if side == 0 { ds[COUNT1] } else { ds[COUNT2] })
}

/// Token id: `character << 8` plus the list slot (side two from 0x14).
fn token_id(ds: &[u8], slot: usize) -> u16 {
    (u16::from(ds[LIST + slot]) << 8).wrapping_add(slot as u16)
}

fn token(scene: &ScriptScene, ds: &[u8], slot: usize) -> Option<usize> {
    // FUN_15b8_16ca(0x200, 0xF5, id).
    scene.find(0x200, 0xf5, token_id(ds, slot))
}

fn place_frame(ds: &[u8], slot: usize) -> u8 {
    ds[PLACE_TABLE + usize::from(ds[PLACES + slot]) * 6 + 5]
}

fn text(ds: &[u8], at: usize) -> String {
    ds[at..]
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| char::from(b))
        .collect()
}

/// `FUN_2449_0faa`: current strength totals of both sides
/// (`DS:9CC2` side one, `DS:9CC4` side two).
pub fn fun_2449_0faa(ds: &mut [u8]) {
    let mut a: u16 = 0;
    for i in 0..usize::from(ds[COUNT1]) {
        a = a.wrapping_add(word(ds, rec(ds[LIST + i]) + 0x1e));
    }
    let mut b: u16 = 0;
    for i in 0x14..0x14 + usize::from(ds[COUNT2]) {
        b = b.wrapping_add(word(ds, rec(ds[LIST + i]) + 0x1e));
    }
    set_word(ds, 0x9cc2, a);
    set_word(ds, 0x9cc4, b);
}

/// `FUN_2449_0aed`: load the token images and place one token per unit.
/// Each unit's slot becomes 1 (engaged) and its commander the top of its
/// leader chain. Army stacks (flags 0x8000) are drawn by kind and size:
/// knights (kind 3) script 0x10, archers (2) 0x0F, soldiers (1) 0x0E,
/// characters 0x0D, foes +4; the image is 0..2 by strength (3 for
/// characters). Places come from `DS:64B8`; a token starts at its place's
/// script step + 1.
pub fn fun_2449_0aed(ds: &mut [u8], scene: &mut ScriptScene, host: &mut dyn ScriptHost) {
    ds[0x9d0e] = 1;
    for k in 0..4u16 {
        let h = host.load_sprite(TOKEN_IMAGES + k, 0);
        set_long(ds, IMAGES + 4 * usize::from(k), h);
    }
    for p in 0..0x1a {
        ds[PLACE_TABLE + p * 6 + 4] = 0;
    }
    for side in [0usize, 0x14] {
        for i in 0..count(ds, side) {
            let slot = i + side;
            ds[SLOTS + slot] = 1;
            let id = ds[LIST + slot];
            let mut top = id;
            while word(ds, rec(top) + 0x16) & 0x8000 != 0 && ds[rec(top) + 0x31] != 0 {
                top = ds[rec(top) + 0x31];
            }
            ds[COMMANDERS + slot] = top;
            let flags = word(ds, rec(id) + 0x16);
            let not_led = u8::from(flags & 0x200 != 0x200);
            let strength = word(ds, rec(id) + 0x1e) as i16;
            let size = |a: i16, b: i16| -> u8 {
                if strength < a {
                    0
                } else if strength < b {
                    1
                } else {
                    2
                }
            };
            let s = side as u8;
            let taken = |ds: &[u8], p: u8| ds[PLACE_TABLE + usize::from(p) * 6 + 4] != 0;
            let (mut script, image, mut p);
            match flags & 0x8007 {
                0x8003 => {
                    if side == 0 {
                        ds[0x9d0e] = 0;
                    }
                    script = 0x10;
                    image = size(0x19, 0x32);
                    p = if s != 0 { s + 5 } else { 0x12 };
                    let end = if s != 0 { s } else { 0x0c };
                    while taken(ds, p) && p > end {
                        p -= 1;
                    }
                }
                0x8002 | 0x8001 => {
                    if flags & 0x8007 == 0x8001 {
                        if side == 0 {
                            ds[0x9d0e] = 0;
                        }
                        script = 0x0e;
                        image = size(0x32, 0x64);
                    } else {
                        script = 0x0f;
                        image = size(0x19, 0x32);
                    }
                    p = if s != 0 { s } else { 9 };
                    let end = if s != 0 { s + 5 } else { 0x12 };
                    while taken(ds, p) && p < end {
                        p += 1;
                    }
                }
                _ => {
                    script = 0x0d;
                    image = 3;
                    p = s;
                    let end = if s != 0 { s + 5 } else { 8 };
                    while taken(ds, p) && p < end {
                        p += 1;
                    }
                }
            }
            let at = PLACE_TABLE + usize::from(p) * 6;
            let (x, y) = (word(ds, at), word(ds, at + 2));
            ds[at + 4] = 1;
            ds[PLACES + slot] = p;
            if side != 0 {
                script += 4;
            }
            let first = ds[at + 5].wrapping_add(1);
            let args = SpawnArgs {
                flags: u16::from(side != 0) | 0x248,
                res: 0,
                image: long(ds, IMAGES + 4 * usize::from(image)),
                callback: (if (s | not_led) != 0 { 0x077a } else { 0x0792 }, 0x2449),
                id: (u16::from(id) << 8).wrapping_add(slot as u16),
                variant: 0,
                script,
                depth: 0xf5,
                x,
                y,
                width: 0,
                height: 0,
            };
            let n = scene.fun_15b8_0461(&args, host);
            scene.widgets[n][0x27] = first;
        }
    }
}

/// `FUN_2449_01e5`: the slot can lead its side's front (engaged,
/// charging, after a charge or withdrawn).
fn fun_2449_01e5(ds: &[u8], slot: usize) -> bool {
    matches!(ds[SLOTS + slot], 1 | 2 | 0x10 | 0x40 | 0x80)
}

/// `FUN_2449_021e`: flankers (3, 5) of side one turn to retreat (6).
fn fun_2449_021e(ds: &mut [u8]) {
    for i in 0..usize::from(ds[COUNT1]) {
        if matches!(ds[SLOTS + i], 3 | 5) {
            ds[SLOTS + i] = 6;
        }
    }
}

/// `FUN_2449_0a70`: fight now: set `DS:9CDA`, release the waits, clear the
/// order column and time the next pause (`DS:0322` / `DS:0326` = clock +
/// 0xB4).
pub fn fun_2449_0a70(ds: &mut [u8], ev: &mut Vec<ArmyEvent>) {
    ds[0x9cda] = 1;
    set_word(ds, 0x9c58, 0);
    set_word(ds, 0x9cb0, 0);
    set_word(ds, 0x9cae, 0);
    ds[0x01f1] = 0;
    ev.push(ArmyEvent::ClearOrders);
    let t = long(ds, CLOCK).wrapping_add(0xb4);
    set_long(ds, 0x0322, t);
    set_long(ds, 0x0326, t);
}

/// `FUN_2449_0255`: each side's front is its foremost token (side one:
/// largest x among the units that can lead; side two: smallest x). When a
/// front moves, every token of the other side targets (`+0x2E`) the unit
/// at it, and side two's tokens (re)start advancing (slot 1, first script
/// step). The fronts are kept in `DS:9D09` / `DS:9D0B`; once they are
/// close (`9D0B - 9D09 <= 0x24`) a round may be fought, and a side-one
/// front behind x 0x8E starts one at once.
pub fn fun_2449_0255(ds: &mut [u8], scene: &mut ScriptScene, ev: &mut Vec<ArmyEvent>) {
    for side in [0usize, 0x14] {
        let mut front: u16 = if side == 0 { 0 } else { 0x140 };
        let mut lead: u16 = 0;
        let own = count(ds, side);
        let other = count(ds, 0x14 - side);
        for i in side..side + own {
            if let Some(t) = token(scene, ds, i) {
                let x = ww(&scene.widgets[t], 0x12);
                let better = if side == 0 {
                    fun_2449_01e5(ds, i) && x > front
                } else {
                    x < front
                };
                if better {
                    lead = token_id(ds, i);
                    front = x;
                }
            }
        }
        let at = 0x9d09 + 2 * usize::from(side != 0);
        if word(ds, at) != front {
            let first = 0x14 - side;
            for j in first..first + other {
                if let Some(t) = token(scene, ds, j) {
                    if side == 0 {
                        ds[SLOTS + j] = 1;
                        scene.widgets[t][0x27] = place_frame(ds, j).wrapping_add(1);
                    }
                    scene.widgets[t][0x2e..0x30].copy_from_slice(&lead.to_le_bytes());
                }
            }
        }
        set_word(ds, at, front);
    }
    if word(ds, 0x9d0b).wrapping_sub(word(ds, 0x9d09)) > 0x24 {
        ds[0x9cda] = 0;
    }
    if word(ds, 0x9d09) < 0x8e {
        fun_2449_0a70(ds, ev);
        fun_2449_021e(ds);
    }
}

/// `FUN_2449_12de`: script handler 15 in an army battle. Opcode 9 asks for
/// a round (`DS:9CDA`), 10 sets `DS:9D0F`, 12 brings withdrawn units
/// (0x10) back to engaged. Returns 2.
pub fn fun_2449_12de(ds: &mut [u8], arg: u16) -> u8 {
    match arg >> 8 {
        9 => ds[0x9cda] = 1,
        10 => ds[0x9d0f] = 1,
        0x0c => {
            for i in 0..usize::from(ds[COUNT1]) {
                if ds[SLOTS + i] == 0x10 {
                    ds[SLOTS + i] = 1;
                }
            }
        }
        _ => {}
    }
    2
}

/// `FUN_2449_0a3c` (the UI tick while the battle runs): every 0x78 clock
/// ticks (two seconds) release the message wait.
pub fn fun_2449_0a3c(ds: &mut [u8]) {
    let now = long(ds, CLOCK);
    if long(ds, 0x9cdb) <= now {
        set_word(ds, 0x9c58, 0);
        set_long(ds, 0x9cdb, now.wrapping_add(0x78));
    }
}

/// `FUN_2449_1021(s1, s2, castle1, castle2)`: has a side broken? 3: side
/// one is gone, 4: side two is gone, 1: side one routs, 2: side two
/// routs, 0: fight on. A side routs when under half (8/16, or 14/16 at a
/// castle... more precisely below `th`/16 of its starting strength
/// `DS:9CC6` / `DS:9CC8`) or when the other side's share exceeds its own
/// by more than 3/16 (2/16 at a castle); nothing routs while one side is
/// three times the other. Side one at its own castle is asked whether to
/// fall back (`FUN_2449_0107`); "no" fights on.
pub fn fun_2449_1021(
    ds: &mut [u8],
    s1: u16,
    s2: u16,
    c1: u16,
    c2: u16,
    world: &mut dyn ArmyWorld,
    ev: &mut Vec<ArmyEvent>,
) -> u8 {
    let defending = word(ds, MODE) & 8 != 0;
    let castle = c1 != 0 || c2 != 0;
    let share = |s: u16, total: u16| (s << 4).checked_div(total).unwrap_or(0xffff);
    let mut r: u8 = 0;
    'decided: {
        if s1 == 0 {
            ev.push(ArmyEvent::Notice(if defending { 0x662f } else { 0x6658 }));
            r = 3;
            break 'decided;
        }
        if s2 == 0 {
            ev.push(ArmyEvent::Notice(if defending { 0x667a } else { 0x669f }));
            r = 4;
            break 'decided;
        }
        if s1 / s2 >= 3 || s2 / s1 >= 3 {
            break 'decided;
        }
        let a = share(s1, word(ds, 0x9cc6));
        let b = share(s2, word(ds, 0x9cc8));
        let side_one_breaks = a < if castle { 0xe } else { 8 }
            || (b.wrapping_sub(a) as i16) > if castle { 2 } else { 3 };
        if side_one_breaks {
            r = 1;
            if defending {
                ev.push(ArmyEvent::Notice(if c1 != 0 { 0x66c6 } else { 0x66ee }));
            } else if c1 != 0 {
                if world.fall_back(ds) != 0 {
                    ev.push(ArmyEvent::Notice(0x670e));
                } else {
                    r = 0;
                }
            } else {
                ev.push(ArmyEvent::Notice(if c2 != 0 { 0x6737 } else { 0x675d }));
            }
            break 'decided;
        }
        if b >= if castle { 0xe } else { 0xb }
            && (a.wrapping_sub(b) as i16) <= if castle { 2 } else { 3 }
        {
            break 'decided;
        }
        let f = if defending {
            if c1 == 0 { 0x6786 } else { 0x6737 }
        } else if c1 != 0 {
            0x6737
        } else if c2 != 0 {
            0x67a6
        } else {
            0x67cb
        };
        ev.push(ArmyEvent::Notice(f));
        r = 2;
    }
    if r != 0 {
        if r & 1 != 0 {
            ev.push(ArmyEvent::Music {
                name: 0x67ee,
                number: 0xb,
                flag: 0,
            });
        } else {
            ev.push(ArmyEvent::Music {
                name: 0x67f6,
                number: 0xa,
                flag: 0,
            });
        }
        if auto(ds) {
            fun_2449_0faa(ds);
            ev.push(ArmyEvent::Summary);
        }
    }
    r
}

/// The `FUN_2449_134e` locals the round body reads and keeps.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RoundLocals {
    /// `[BP-0x56]`: rounds fought.
    pub rounds: u16,
    /// `[BP-0x7E]`: the last `FUN_2449_1021` result.
    pub result: u8,
    /// `[BP-0x70]` / `[BP-0x6E]`: each side's leader stands at a castle
    /// (location record word `+0x16` 0x258, 0x259, 0x25C or 0x25F).
    pub castle: [u16; 2],
    /// `[BP-0x7C]` / `[BP-0x7A]`, `[BP-0x78]` / `[BP-0x76]`: how far each
    /// side moves on the map when it routs (away from the other leader).
    pub dx: [u16; 2],
    pub dy: [u16; 2],
}

impl RoundLocals {
    /// The locals as `FUN_2449_134e` sets them up from the two leaders
    /// `DS:9DAE` / `DS:9DB0`.
    #[must_use]
    pub fn new(ds: &[u8]) -> Self {
        let pos = |id: u8| (word(ds, rec(id) + 0x0e), word(ds, rec(id) + 0x10));
        let (x0, y0) = pos(ds[0x9dae]);
        let (x1, y1) = pos(ds[0x9db0]);
        let castle = |x: u16, y: u16| {
            let loc = crate::encounter::location_at(ds, x as i16, y as i16);
            let kind = word(ds, 0x40b6 + usize::from(loc) * 0x19 + 0x16);
            u16::from(matches!(kind, 0x25c | 0x258 | 0x259 | 0x25f))
        };
        let dx = x1.wrapping_sub(x0);
        let dy = y1.wrapping_sub(y0);
        Self {
            rounds: 0,
            result: 0,
            castle: [castle(x0, y0), castle(x1, y1)],
            dx: [dx.wrapping_neg(), dx],
            dy: [dy.wrapping_neg(), dy],
        }
    }
}

/// How a round ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundEnd {
    /// Back to the order loop (`2449:15CB`).
    Continue,
    /// The battle is over (`2449:26D8`).
    Over,
}

fn wait_message(ds: &mut [u8], ev: &mut Vec<ArmyEvent>) {
    set_word(ds, 0x9c58, 1);
    ev.push(ArmyEvent::Wait(WAIT_MESSAGE));
    if word(ds, 0x9cb0) != 0 {
        fun_2449_1333(ds, ev);
    }
}

/// `FUN_2449_1333`: wait for the order menu to close.
fn fun_2449_1333(ds: &mut [u8], ev: &mut Vec<ArmyEvent>) {
    ds[0x01f1] = 0;
    ev.push(ArmyEvent::Wait(WAIT_MENU));
    ds[0x01f1] = 1;
}

fn name_arg(ds: &[u8], id: u8) -> Arg {
    Arg::Text(crate::names::display_name(ds, id))
}

fn unit_arg(ds: &[u8], id: u8) -> Arg {
    Arg::Text(text(ds, rec(id)))
}

/// "%s%s %Ss": "the" and "" for a commander's own unit, else the
/// commander's name and "'s".
fn owner_args(ds: &[u8], commander: u8, id: u8) -> [Arg; 2] {
    if commander == id {
        [Arg::Text(text(ds, 0x68e4)), Arg::Text(text(ds, 0x657d))]
    } else {
        [name_arg(ds, commander), Arg::Text(text(ds, 0x68e8))]
    }
}

fn sext(v: u16) -> u32 {
    i32::from(v as i16) as u32
}

/// The round body of `FUN_2449_134e` (`2449:1653` to `15CB` or `26D8`).
///
/// Side one's orders are applied (0x80 back to 1, 0x40 to 0x80, a charge
/// 2 to 0x40, a flank 3 to 5 and 5 to 1). Each side's power is the sum of
/// strength x attack (`+0x2D`, the 16-bit product sign-extended) of its
/// engaged units, plus 2 x attack for units of type 1 (`+0x1C`); a
/// charge counts 1.5 x strength, a flank's second round double. After the
/// first round (or when a side has no one engaged) `FUN_2449_1021` may end
/// it. Otherwise each side deals `(rand(8) + 12) x power / enemy strength`.
/// Characters take a wound per 0x80 of it whose `rand(0x80)` falls below
/// what remains and whose `rand(100)` beats their `+0x2E`; army stacks
/// lose `strength x damage / (defence + castle) >> 7` men (1.5 x after a
/// charge).
pub fn fun_2449_134e_round(
    ds: &mut [u8],
    scene: &mut ScriptScene,
    loc: &mut RoundLocals,
    world: &mut dyn ArmyWorld,
    ev: &mut Vec<ArmyEvent>,
) -> RoundEnd {
    let manual = !auto(ds);
    let mut strength = [0u16; 2];
    let mut power = [0u32; 2];
    let mut engaged = [false; 0x28];
    for i in 0..usize::from(ds[COUNT1]) {
        let id = ds[LIST + i];
        let commander = ds[COMMANDERS + i];
        let r = rec(id);
        if ds[SLOTS + i] == 0x80 {
            ds[SLOTS + i] = 1;
        }
        if ds[SLOTS + i] == 0x40 {
            ds[SLOTS + i] = 0x80;
        }
        let s = word(ds, r + 0x1e);
        let atk = u16::from(ds[r + 0x2d]);
        if matches!(ds[SLOTS + i], 1 | 0x80) {
            engaged[i] = true;
            strength[0] = strength[0].wrapping_add(s);
            power[0] = power[0].wrapping_add(sext(s.wrapping_mul(atk)));
            if word(ds, r + 0x1c) == 1 {
                power[0] = power[0].wrapping_add(sext(atk * 2));
            }
        }
        if ds[SLOTS + i] == 2 {
            engaged[i] = true;
            ds[SLOTS + i] = 0x40;
            strength[0] = strength[0].wrapping_add(s);
            let charge = s.wrapping_add(((s as i16) >> 1) as u16);
            power[0] = power[0].wrapping_add(sext(charge.wrapping_mul(atk)));
            if manual {
                if s as i16 > 1 {
                    ev.push(ArmyEvent::Message {
                        format: 0x6862,
                        args: vec![name_arg(ds, commander), unit_arg(ds, id)],
                    });
                } else {
                    ev.push(ArmyEvent::Message {
                        format: 0x687e,
                        args: vec![name_arg(ds, id)],
                    });
                }
                wait_message(ds, ev);
            }
        }
        if ds[SLOTS + i] == 5 {
            engaged[i] = true;
            ds[SLOTS + i] = 1;
            strength[0] = strength[0].wrapping_add(s);
            power[0] = power[0].wrapping_add(sext(s.wrapping_mul(atk).wrapping_mul(2)));
            if manual {
                if s as i16 > 1 {
                    ev.push(ArmyEvent::Message {
                        format: 0x689f,
                        args: vec![name_arg(ds, commander), unit_arg(ds, id)],
                    });
                }
                wait_message(ds, ev);
            }
        }
        if ds[SLOTS + i] == 3 {
            ds[SLOTS + i] = 5;
        }
    }
    for i in 0..usize::from(ds[COUNT2]) {
        let slot = i + 0x14;
        if ds[SLOTS + slot] == 1 {
            engaged[slot] = true;
            let r = rec(ds[LIST + slot]);
            let s = word(ds, r + 0x1e);
            let atk = u16::from(ds[r + 0x2d]);
            strength[1] = strength[1].wrapping_add(s);
            power[1] = power[1].wrapping_add(sext(s.wrapping_mul(atk)));
            if word(ds, r + 0x1c) == 1 {
                power[1] = power[1].wrapping_add(sext(atk * 2));
            }
        }
    }
    loc.rounds = loc.rounds.wrapping_add(1);
    if loc.rounds > 1 || strength[0] == 0 || strength[1] == 0 {
        loc.result = fun_2449_1021(
            ds,
            strength[0],
            strength[1],
            loc.castle[0],
            loc.castle[1],
            world,
            ev,
        );
    }
    if loc.result != 0 {
        if loc.result < 3 {
            let now = (long(ds, GAME_CLOCK) >> 8) as u16;
            for side in [0usize, 0x14] {
                let s = usize::from(side != 0);
                for i in 0..count(ds, side) {
                    let slot = i + side;
                    if ds[SLOTS + slot] == 4 {
                        continue;
                    }
                    let r = rec(ds[LIST + slot]);
                    let [c0, c1] = loc.castle.map(|c| c != 0);
                    let moves = (side == 0 && c1)
                        || (side != 0 && c0)
                        || (side == 0 && loc.result == 1 && !c0)
                        || (side != 0 && loc.result == 2 && !c1);
                    if moves {
                        set_word(ds, r + 0x0e, word(ds, r + 0x0e).wrapping_add(loc.dx[s]));
                        set_word(ds, r + 0x10, word(ds, r + 0x10).wrapping_add(loc.dy[s]));
                        set_word(ds, r + 0x1a, now.wrapping_add(0x3c));
                    } else if (side == 0 && c0) || (side != 0 && c1) {
                        set_word(ds, r + 0x1a, now.wrapping_add(0x46));
                    }
                }
            }
            ev.push(ArmyEvent::MapMove(word(ds, LIST), word(ds, LIST + 0x14)));
        }
        return RoundEnd::Over;
    }
    // Damage dealt to each side: [to side one, to side two].
    let to_two = (u32::from(world.random(8)) + 0xc)
        .wrapping_mul(power[0])
        .checked_div(u32::from(strength[1]))
        .unwrap_or(0);
    let to_one = (u32::from(world.random(8)) + 0xc)
        .wrapping_mul(power[1])
        .checked_div(u32::from(strength[0]))
        .unwrap_or(0);
    let damage = [to_one, to_two];
    for side in [0usize, 0x14] {
        let s = usize::from(side != 0);
        let b = u16::from(ds[0x9c50 + s]);
        for i in 0..count(ds, side) {
            let slot = i + side;
            if !engaged[slot] {
                continue;
            }
            let id = ds[LIST + slot];
            let commander = ds[COMMANDERS + slot];
            let r = rec(id);
            let row_y = b
                .wrapping_mul(i as u16)
                .wrapping_add(word(ds, 0x9c48))
                .wrapping_add(10);
            let code = b << 12 | 100;
            if word(ds, r + 0x16) & 0x8000 == 0 {
                // A character: wounds.
                let mut wounded = false;
                let mut left = damage[s] as i32;
                while left > 0 {
                    let roll = u32::from(world.random(0x80));
                    if left as u32 > 0xffff || roll < left as u32 {
                        let roll = world.random(100);
                        if roll > u16::from(ds[r + 0x2e]) {
                            wounded = true;
                            ds[r + 0x2c] = ds[r + 0x2c].wrapping_sub(1);
                            if ds[r + 0x2c] == 0 {
                                ds[SLOTS + slot] = 4;
                                if manual {
                                    ev.push(ArmyEvent::Message {
                                        format: 0x68bb,
                                        args: vec![name_arg(ds, id)],
                                    });
                                    let x = word(ds, if side == 0 { 0x9c42 } else { 0x9c46 });
                                    ev.push(ArmyEvent::Mark { x, y: row_y, code });
                                    set_word(ds, 0x9c58, 1);
                                    ev.push(ArmyEvent::Wait(WAIT_MESSAGE));
                                }
                                if let Some(t) = token(scene, ds, slot) {
                                    scene.widgets[t][0x27] = 0x0b;
                                }
                                wounded = false;
                                let mut cev = Vec::new();
                                let ended = combat::fun_20b5_1229(ds, id, &mut cev);
                                ev.extend(cev.into_iter().map(ArmyEvent::Combat));
                                if ended != 0 {
                                    ev.push(ArmyEvent::Music {
                                        name: 0x67f6,
                                        number: 0xa,
                                        flag: 0,
                                    });
                                    return RoundEnd::Over;
                                }
                                break;
                            }
                        }
                    }
                    left = left.wrapping_sub(0x80);
                }
                if wounded && manual {
                    let feminine = word(ds, r + 0x16) & 0x800 != 0;
                    let name = name_arg(ds, id);
                    let pronoun = Arg::Text(text(
                        ds,
                        usize::from(word(ds, 0x79e0 + 4 * usize::from(feminine))),
                    ));
                    let k = usize::from(world.random(8)) + 4;
                    let format = word(ds, 0x5dca + 4 * k);
                    ev.push(ArmyEvent::Message {
                        format,
                        args: vec![name, pronoun],
                    });
                    ev.push(ArmyEvent::UnitRow {
                        id,
                        slot: slot as u16,
                    });
                    wait_message(ds, ev);
                }
            } else {
                // An army stack: casualties.
                let defence = u32::from(ds[r + 0x2b]).wrapping_add(u32::from(loc.castle[s]));
                let men = sext(word(ds, r + 0x1e));
                let mut lost = men
                    .wrapping_mul(damage[s])
                    .checked_div(defence)
                    .unwrap_or(0)
                    >> 7;
                if ds[SLOTS + slot] == 0x80 {
                    lost = lost.wrapping_add(lost >> 1);
                }
                if lost == 0 {
                    continue;
                }
                if men > lost {
                    set_word(ds, r + 0x1e, word(ds, r + 0x1e).wrapping_sub(lost as u16));
                    if manual {
                        let [owner, sep] = owner_args(ds, commander, id);
                        if lost >= 2 {
                            ev.push(ArmyEvent::Message {
                                format: 0x68c9,
                                args: vec![Arg::Number(lost), owner, sep, unit_arg(ds, id)],
                            });
                        } else {
                            ev.push(ArmyEvent::Message {
                                format: 0x68eb,
                                args: vec![owner, sep, unit_arg(ds, id)],
                            });
                        }
                        ev.push(ArmyEvent::MouseOff);
                        ev.push(ArmyEvent::Strength { slot: slot as u16 });
                    }
                    // Off screen (DS:9C56 bit 0x20) the token keeps its image.
                    if let Some(t) = token(scene, ds, slot).filter(|_| manual) {
                        let left = word(ds, r + 0x1e) as i16;
                        let small = long(ds, IMAGES);
                        let middle = long(ds, IMAGES + 4);
                        let image = match word(ds, r + 0x16) & 7 {
                            3 | 2 if left < 0x19 => Some(small),
                            3 | 2 if left < 0x32 => Some(middle),
                            1 if left < 0x32 => Some(small),
                            1 if left < 0x64 => Some(middle),
                            _ => None,
                        };
                        if let Some(h) = image {
                            scene.widgets[t][2..6].copy_from_slice(&h.to_le_bytes());
                        }
                    }
                } else {
                    if manual {
                        let [owner, sep] = owner_args(ds, commander, id);
                        ev.push(ArmyEvent::Message {
                            format: 0x690e,
                            args: vec![owner, sep, unit_arg(ds, id)],
                        });
                        let x = if side == 0 {
                            word(ds, 0x9c42).wrapping_add(if word(ds, MODE) & 8 != 0 {
                                0
                            } else {
                                5
                            })
                        } else {
                            word(ds, 0x9c46)
                        };
                        ev.push(ArmyEvent::Mark { x, y: row_y, code });
                    }
                    ds[SLOTS + slot] = 4;
                    set_word(ds, r + 0x16, word(ds, r + 0x16) & 0xfff7);
                    set_word(ds, r + 0x1e, 0);
                    ds[r + 0x31] = 0;
                    set_word(ds, r + 0x0e, 0);
                    set_word(ds, r + 0x10, 0);
                    if let Some(t) = token(scene, ds, slot) {
                        scene.widgets[t][0x27] = 0x0b;
                    }
                }
                if manual {
                    ev.push(ArmyEvent::MouseOn);
                    wait_message(ds, ev);
                }
            }
        }
    }
    fun_2449_0faa(ds);
    for i in 0..usize::from(ds[COUNT1]) {
        if ds[SLOTS + i] == 5 {
            if let Some(t) = token(scene, ds, i) {
                scene.widgets[t][0x27] = if (ww(&scene.widgets[t], 0x10) as i16) < 0x82 {
                    0x10
                } else {
                    0x11
                };
            }
        }
    }
    if manual {
        let lost = word(ds, 0x9cc6).wrapping_sub(word(ds, 0x9cc2));
        let format = if word(ds, MODE) & 8 != 0 {
            0x692d
        } else {
            0x695d
        };
        ev.push(ArmyEvent::Message {
            format,
            args: vec![
                Arg::Number(u32::from(lost)),
                Arg::Number(u32::from(word(ds, 0x9cc6))),
            ],
        });
    }
    RoundEnd::Continue
}

pub mod battle;
pub use battle::{ArmyBattle, ArmyOrder, ArmySprite};

#[cfg(test)]
mod tests;
