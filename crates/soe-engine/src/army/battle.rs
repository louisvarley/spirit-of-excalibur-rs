//! The `FUN_2449_134e` loop on the ported scene engine.
//!
//! The original runs the battle as straight-line code that waits in the UI
//! loop (`FUN_15b8_09f7`) for DGROUP flags: the message wait `DS:9C58`,
//! released every two seconds by the tick `FUN_2449_0a3c`; the order menu
//! `DS:9CB0`; "Do Round" `DS:9CAE`; the close `DS:031E`. While it waits
//! the scene runs: the tokens walk by their scripts and handler 15
//! (`FUN_2449_12de`) asks for rounds. [`ArmyBattle`] keeps the same waits
//! as phases and advances one scene frame per clock tick.
//!
//! One approximation: a round body runs in one go (the ported
//! [`fun_2449_134e_round`]); its message waits are then replayed, so the
//! messages appear two seconds apart as in the original, but the strength
//! changes of a whole round are applied when it starts, and scene frames
//! (and their random draws) do not interleave with its pauses.

use std::collections::HashMap;

use super::*;
use crate::resources::ResourceManager;
use crate::script::{ScriptEvent, ScriptWorld};
use crate::text::format_message;

/// `DS:7E3E` runs at 60 ticks per second.
pub const CLOCK_HZ: u32 = 60;

/// The order buttons of the army battle panel (`FUN_20b5_085b`, bit 0x10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArmyOrder {
    /// `FUN_2449_0420`.
    Charge,
    /// `FUN_2449_04f2`.
    Engage,
    /// `FUN_2449_056b`.
    Flank,
    /// `FUN_2449_063a`.
    Retreat,
    /// `FUN_2449_0764`.
    DoRound,
    /// `FUN_2449_0ade`: let the battle be fought without orders.
    Ignore,
}

impl ArmyOrder {
    pub const ALL: [Self; 6] = [Self::Charge, Self::Engage, Self::Flank, Self::Retreat, Self::DoRound, Self::Ignore];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Charge => "Charge",
            Self::Engage => "Engage",
            Self::Flank => "Flank",
            Self::Retreat => "Retreat",
            Self::DoRound => "Do Round",
            Self::Ignore => "Ignore",
        }
    }
}

/// A token to draw: frame `frame` of the sprite bank `resource` at its
/// screen position (tokens use absolute 320x200 coordinates; the field
/// image sits at (0x90, 0x50)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArmySprite {
    pub resource: u16,
    pub frame: u8,
    pub x: i16,
    pub y: i16,
    pub mirrored: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// `2449:15CB`.
    Top,
    /// After the message wait of the order loop (`15E9`).
    AfterPause,
    /// After the order menu wait (`15F7`).
    AfterMenu,
    /// Waiting for "Do Round" before a round.
    BeforeRound,
    /// Replaying a round's events; then back to the top or to the end.
    Replay(RoundEnd),
    /// `26D8`: waiting for the close.
    Closing,
    Done,
}

struct Host<'a> {
    ds: Vec<u8>,
    resources: &'a mut ResourceManager,
    sheets: HashMap<u32, Vec<u8>>,
    rng: u32,
    reads: u32,
}

impl Host<'_> {
    fn clock(&self) -> u32 {
        long(&self.ds, CLOCK)
    }
}

fn next_random(rng: &mut u32, n: u16) -> u16 {
    // Stand-in for the timer-seeded FUN_15b8_17e0 (seed DS:01FC).
    *rng = rng.wrapping_mul(0x015a_4e35).wrapping_add(1);
    if n == 0 { 0 } else { ((*rng >> 16) % u32::from(n)) as u16 }
}

impl ScriptWorld for Host<'_> {
    fn random(&mut self, n: u16) -> u16 {
        next_random(&mut self.rng, n)
    }
}

impl ScriptHost for Host<'_> {
    fn clock(&mut self) -> u32 {
        // As in the duel: the timer keeps counting while a script loops.
        self.reads += 1;
        if self.reads % 4096 == 0 {
            let c = Host::clock(self).wrapping_add(1);
            set_long(&mut self.ds, CLOCK, c);
        }
        Host::clock(self)
    }
    fn program(&self, script: u8) -> Vec<u16> {
        let off = usize::from(word(&self.ds, 0x1e2a + 4 * usize::from(script)));
        if off == 0 {
            return Vec::new();
        }
        self.ds[off..(off + 0x200).min(self.ds.len())]
            .chunks(2)
            .map(|c| u16::from_le_bytes([c[0], *c.get(1).unwrap_or(&0)]))
            .collect()
    }
    fn sheet(&self, image: u32) -> Vec<u8> {
        self.sheets.get(&image).cloned().unwrap_or_default()
    }
    fn load_image(&mut self, _old: u32, res: u16) -> u32 {
        self.load_sprite(res, 0)
    }
    fn load_sprite(&mut self, res: u16, _variant: u16) -> u32 {
        let h = u32::from(res);
        if !self.sheets.contains_key(&h) {
            let file = self.resources.find_file(res).map(str::to_owned);
            let Some(data) = file.and_then(|f| self.resources.record(&f, res).ok().map(<[u8]>::to_vec)) else {
                return 0;
            };
            let mut sheet = vec![0, 0];
            sheet.extend_from_slice(&data);
            self.sheets.insert(h, sheet);
        }
        h
    }
    fn static_image(&mut self, _res: u16) -> (u32, u16, u16) {
        (0, 0, 0)
    }
    fn retain_image(&mut self, _image: u32) {}
    fn release_image(&mut self, _flags: u16, _image: u32) {}
    fn event(&mut self, _event: ScriptEvent) {}
    fn hook(&mut self, handler: u16, arg: u16, _scene: &mut ScriptScene) -> u8 {
        if handler == 15 {
            fun_2449_12de(&mut self.ds, arg)
        } else {
            2
        }
    }
}

struct World<'a> {
    rng: &'a mut u32,
}

impl ArmyWorld for World<'_> {
    fn random(&mut self, n: u16) -> u16 {
        next_random(self.rng, n)
    }
    fn fall_back(&mut self, _ds: &[u8]) -> u16 {
        // The Yes/No box is not shown yet; the original's default answer
        // when fought off screen.
        1
    }
}

/// An army battle between the side lists `DS:9DC3` and `DS:9DD7`.
pub struct ArmyBattle {
    pub scene: ScriptScene,
    ds: Vec<u8>,
    sheets: HashMap<u32, Vec<u8>>,
    rng: u32,
    phase: Phase,
    locals: RoundLocals,
    /// Round events waiting to be replayed, oldest first.
    queue: Vec<ArmyEvent>,
    /// The message stream, oldest first.
    pub messages: Vec<String>,
    /// Message boxes (outcomes), oldest first.
    pub notices: Vec<String>,
    /// The field image behind the tokens (`FUN_20b5_085b`: 0x26D or 0x26C).
    pub field: u16,
    /// The slot whose order menu is open (`DS:9CAA` low byte).
    pub selected: Option<usize>,
    /// The field image's resource record.
    field_image: Option<Vec<u8>>,
}

impl ArmyBattle {
    /// Set the battle up as `FUN_2449_134e` does, on DGROUP `ds` (side
    /// lists, counts, leaders `DS:9DAE` / `DS:9DB0` and mode `DS:9C56`
    /// already set, as `FUN_20b5_2a30` leaves them). `terrain_class` is
    /// `DS:2B5C[terrain]` under side one's leader.
    pub fn start(ds: &[u8], resources: &mut ResourceManager, terrain_class: u8, seed: u32) -> Self {
        let mut ds = ds.to_vec();
        ds[0x5e38] = 0;
        set_word(&mut ds, 0x9cb0, 0);
        set_word(&mut ds, 0x9cae, 0);
        ds[0x9d0f] = 0;
        set_word(&mut ds, 0x9cb2, 0);
        set_word(&mut ds, 0x9d09, 0);
        set_word(&mut ds, 0x9d0b, 0x140);
        ds[0x9dc0] = 0;
        ds[0x01ef] = 0;
        ds[0x01f0] = 0;
        ds[0x01f1] = 1;
        let mut host = Host { ds, resources, sheets: HashMap::new(), rng: seed, reads: 0 };
        let mut scene = ScriptScene { clip: [0, 0, 200, 320], ..ScriptScene::default() };
        let mut backdrop = [0u8; 0x36];
        backdrop[0x0f] = 0xff;
        scene.widgets.push(backdrop);
        let mut ds = std::mem::take(&mut host.ds);
        fun_2449_0aed(&mut ds, &mut scene, &mut host);
        let Host { sheets, rng, .. } = host;
        let manual = !auto(&ds);
        if manual {
            // FUN_20b5_085b draws the panel; the tick FUN_2449_0a3c runs.
            ds[0x01f4] = 0;
        }
        let locals = RoundLocals::new(&ds);
        let mut messages = Vec::new();
        if manual {
            messages.push(text(&ds, 0x5e46));
            let t = long(&ds, CLOCK).wrapping_add(0xb4);
            set_long(&mut ds, 0x9cdb, t);
        }
        ds[0x9cda] = u8::from(!manual);
        let total = |ds: &[u8], side: usize| {
            (0..count(ds, side)).fold(0u16, |a, i| a.wrapping_add(word(ds, rec(ds[LIST + side + i]) + 0x1e)))
        };
        let (t1, t2) = (total(&ds, 0), total(&ds, 0x14));
        set_word(&mut ds, 0x9cc6, t1);
        set_word(&mut ds, 0x9cc8, t2);
        fun_2449_0faa(&mut ds);
        let field = if matches!(terrain_class.wrapping_sub(1), 0 | 5 | 8 | 11) { 0x26d } else { 0x26c };
        let field_image = resources
            .find_file(field)
            .map(str::to_owned)
            .and_then(|f| resources.record(&f, field).ok().map(<[u8]>::to_vec));
        Self {
            scene,
            ds,
            sheets,
            rng,
            phase: Phase::Top,
            locals,
            queue: Vec::new(),
            messages,
            notices: Vec::new(),
            field,
            selected: None,
            field_image,
        }
    }

    /// The field image's resource record (drawn at (0x90, 0x50)).
    #[must_use]
    pub fn field_image(&self) -> Option<&[u8]> {
        self.field_image.as_deref()
    }

    #[must_use]
    pub fn ds(&self) -> &[u8] {
        &self.ds
    }

    /// The battle has closed.
    #[must_use]
    pub fn finished(&self) -> bool {
        self.phase == Phase::Done
    }

    /// `FUN_2449_1021`'s last result: 1/3 side one lost, 2/4 side two.
    #[must_use]
    pub const fn result(&self) -> u8 {
        self.locals.result
    }

    /// The sprite bank of a token image.
    #[must_use]
    pub fn bank(&self, resource: u16) -> Option<&[u8]> {
        self.sheets.get(&u32::from(resource)).map(|s| &s[2..])
    }

    /// Side `side` (0 or 1): (character id, strength, slot state).
    #[must_use]
    pub fn units(&self, side: usize) -> Vec<(u8, u16, u8)> {
        let base = side * 0x14;
        (0..count(&self.ds, base))
            .map(|i| {
                let id = self.ds[LIST + base + i];
                (id, word(&self.ds, rec(id) + 0x1e), self.ds[SLOTS + base + i])
            })
            .collect()
    }

    /// The orders open to the selected unit (`DS:5E30[order] >> kind`).
    #[must_use]
    pub fn order_allowed(&self, order: ArmyOrder) -> bool {
        let Some(slot) = self.selected else { return matches!(order, ArmyOrder::DoRound | ArmyOrder::Ignore) };
        let k = match order {
            ArmyOrder::Charge => 0,
            ArmyOrder::Engage => 1,
            ArmyOrder::Flank => 4,
            ArmyOrder::Retreat => 5,
            _ => return true,
        };
        (self.ds[0x5e30 + k] >> self.kind(slot)) & 1 != 0
    }

    fn kind(&self, slot: usize) -> u8 {
        let f = word(&self.ds, rec(self.ds[LIST + slot]) + 0x16);
        if f & 0x8000 == 0 { 4 } else { (f & 7) as u8 }
    }

    fn say(&mut self, format: u16, args: &[Arg]) {
        let line = format_message(&text(&self.ds, usize::from(format)), args, |_| None);
        self.messages.push(line);
    }

    /// `FUN_2449_0792`: click on a side-one unit row or token.
    pub fn select(&mut self, slot: usize) {
        if slot >= count(&self.ds, 0) {
            return;
        }
        let id = self.ds[LIST + slot];
        if word(&self.ds, rec(id) + 0x16) & 8 == 0 {
            let name = text(&self.ds, rec(id));
            self.say(0x65d6, &[Arg::Text(name)]);
            return;
        }
        set_word(&mut self.ds, 0x9cb2, 1);
        match self.ds[SLOTS + slot] {
            4 | 6 => {
                // FUN_2449_077a: "You cannot command that force."
                let line = format_message(&text(&self.ds, 0x65d2), &[Arg::Text(text(&self.ds, 0x6560))], |_| None);
                self.notices.push(line);
                return;
            }
            0x40 | 0x80 => {
                let line = format_message(&text(&self.ds, 0x65e2), &[Arg::Text(text(&self.ds, 0x6560))], |_| None);
                self.notices.push(line);
                return;
            }
            _ => {}
        }
        let tid = token_id(&self.ds, slot);
        set_word(&mut self.ds, 0x9caa, tid);
        if token(&self.scene, &self.ds, slot).is_none() {
            return;
        }
        self.selected = Some(slot);
        set_word(&mut self.ds, 0x9cb0, 1);
        set_word(&mut self.ds, 0x9c58, 0);
        self.ds[0x01f1] = 0;
        if word(&self.ds, rec(id) + 0x16) & 0x8000 == 0 {
            self.say(0x65f5, &[Arg::Text(text(&self.ds, rec(id)))]);
        } else {
            let n = word(&self.ds, rec(id) + 0x1e);
            self.say(0x660e, &[Arg::Number(u32::from(n)), Arg::Text(text(&self.ds, rec(id)))]);
        }
    }

    /// An order button (`FUN_2449_0420`, `04f2`, `056b`, `063a`, `0764`,
    /// `0ade`).
    pub fn order(&mut self, order: ArmyOrder) {
        match order {
            ArmyOrder::DoRound => {
                set_word(&mut self.ds, 0x9cae, 0);
                set_word(&mut self.ds, 0x9cb0, 0);
                self.ds[0x01f1] = 1;
                self.selected = None;
                return;
            }
            ArmyOrder::Ignore => {
                let m = word(&self.ds, MODE) | 0x20;
                set_word(&mut self.ds, MODE, m);
                let mut ev = Vec::new();
                fun_2449_0a70(&mut self.ds, &mut ev);
                self.selected = None;
                return;
            }
            _ => {}
        }
        set_word(&mut self.ds, 0x9cb0, 0);
        let Some(slot) = self.selected.take() else { return };
        if word(&self.ds, 0x9cb2) == 0 || self.ds[SLOTS + slot] == 6 {
            return;
        }
        let allowed = |s: &Self, k: usize| (s.ds[0x5e30 + k] >> s.kind(slot)) & 1 != 0;
        let t = token(&self.scene, &self.ds, slot);
        let first = place_frame(&self.ds, slot);
        match order {
            ArmyOrder::Charge => {
                if !allowed(self, 0) {
                    self.notices.push(text(&self.ds, 0x65b4));
                } else if let Some(t) = t {
                    if ww(&self.scene.widgets[t], 0x12) <= word(&self.ds, 0x9d0b) {
                        self.ds[SLOTS + slot] = 2;
                        self.scene.widgets[t][0x27] = first.wrapping_add(4);
                    }
                }
            }
            ArmyOrder::Engage => {
                self.ds[SLOTS + slot] = 1;
                if let Some(t) = t {
                    self.scene.widgets[t][0x27] = first.wrapping_add(1);
                }
            }
            ArmyOrder::Flank => {
                if !allowed(self, 4) {
                    self.notices.push(text(&self.ds, 0x65b4));
                } else {
                    self.ds[SLOTS + slot] = 3;
                    if let Some(t) = t {
                        let w = &mut self.scene.widgets[t];
                        w[0x27] = if (ww(w, 0x10) as i16) < 0x82 { 0x0c } else { 0x0d };
                        if ww(w, 0x12) as i16 > 0xdb {
                            w[0x27] += 2;
                        }
                    }
                }
            }
            ArmyOrder::Retreat => {
                if let Some(t) = t {
                    let r = rec(self.ds[LIST + slot]);
                    if (ww(&self.scene.widgets[t], 0x12) as i16) < 0xba {
                        self.scene.widgets[t][0x27] = 10;
                        self.ds[SLOTS + slot] = 6;
                        let s = word(&self.ds, r + 0x1e);
                        set_word(&mut self.ds, r + 0x1e, s.wrapping_sub(((s as i16) >> 3) as u16));
                    } else {
                        self.scene.widgets[t][0x27] = first.wrapping_add(7);
                        self.ds[SLOTS + slot] = 0x10;
                    }
                }
            }
            _ => {}
        }
    }

    fn waiting(&self, flag: u16) -> bool {
        word(&self.ds, usize::from(flag)) != 0
    }

    /// Run the 134e program until it waits.
    fn run(&mut self) {
        for _ in 0..64 {
            let manual = !auto(&self.ds);
            match self.phase {
                Phase::Top => {
                    if manual {
                        let mut ev = Vec::new();
                        fun_2449_0255(&mut self.ds, &mut self.scene, &mut ev);
                        set_word(&mut self.ds, 0x9c58, 1);
                        self.phase = Phase::AfterPause;
                        return;
                    }
                    self.round();
                }
                Phase::AfterPause => {
                    if self.waiting(WAIT_MESSAGE) {
                        return;
                    }
                    if self.waiting(WAIT_MENU) {
                        self.ds[0x01f1] = 0;
                    }
                    self.phase = Phase::AfterMenu;
                }
                Phase::AfterMenu => {
                    if self.waiting(WAIT_MENU) {
                        return;
                    }
                    self.ds[0x01f1] = 1;
                    if self.ds[0x9d0e] != 0 && self.ds[0x9d0f] != 0 {
                        self.ds[0x9cda] = 1;
                    }
                    if self.ds[0x9cda] == 0 {
                        self.phase = Phase::Top;
                        continue;
                    }
                    let c = usize::from(self.ds[0x5e38]);
                    if manual && word(&self.ds, MODE) & 8 == 0 && c != 0 && !matches!(self.ds[0x9c81 + c], 4 | 0x10) {
                        self.ds[0x01f1] = 0;
                        set_word(&mut self.ds, 0x9cae, 1);
                        self.phase = Phase::BeforeRound;
                        return;
                    }
                    self.round();
                }
                Phase::BeforeRound => {
                    if self.waiting(WAIT_ROUND) {
                        return;
                    }
                    self.round();
                }
                Phase::Replay(end) => {
                    if self.waiting(WAIT_MESSAGE) || self.waiting(WAIT_MENU) {
                        return;
                    }
                    if self.queue.is_empty() {
                        if end == RoundEnd::Continue {
                            self.phase = Phase::Top;
                        } else {
                            self.close();
                        }
                        continue;
                    }
                    match self.queue.remove(0) {
                        ArmyEvent::Message { format, args } => self.say(format, &args),
                        ArmyEvent::Notice(f) => {
                            let line = text(&self.ds, usize::from(f));
                            self.notices.push(line.clone());
                            self.messages.push(line);
                        }
                        ArmyEvent::Wait(f) => set_word(&mut self.ds, usize::from(f), 1),
                        _ => {}
                    }
                }
                Phase::Closing => {
                    if long(&self.ds, CLOCK) >= long(&self.ds, 0x0322) || auto(&self.ds) {
                        self.ds[0x9dc0] = 1;
                        self.ds[0x01ef] = 1;
                        self.ds[0x01f0] = 1;
                        self.phase = Phase::Done;
                    }
                    return;
                }
                Phase::Done => return,
            }
        }
    }

    fn round(&mut self) {
        let mut ev = Vec::new();
        let mut world = World { rng: &mut self.rng };
        let end = fun_2449_134e_round(&mut self.ds, &mut self.scene, &mut self.locals, &mut world, &mut ev);
        // The waits ran (at once) inside the round; release them and let
        // the replay set them again one by one.
        set_word(&mut self.ds, 0x9c58, 0);
        self.queue = ev;
        self.phase = Phase::Replay(end);
    }

    /// `2449:26D8`.
    fn close(&mut self) {
        self.ds[0x01f1] = 0;
        self.ds[0x01f4] = 1;
        if !auto(&self.ds) {
            let t = long(&self.ds, CLOCK).wrapping_add(0xb4);
            set_long(&mut self.ds, 0x0322, t);
            set_long(&mut self.ds, 0x0326, t);
        }
        self.phase = Phase::Closing;
    }

    /// Advance by `ticks` clock ticks (60 per second), one scene frame
    /// each.
    pub fn advance(&mut self, ticks: u32, resources: &mut ResourceManager) {
        for _ in 0..ticks {
            if self.finished() {
                return;
            }
            let mut host = Host {
                ds: std::mem::take(&mut self.ds),
                resources,
                sheets: std::mem::take(&mut self.sheets),
                rng: self.rng,
                reads: 0,
            };
            let c = host.clock().wrapping_add(1);
            set_long(&mut host.ds, CLOCK, c);
            if !auto(&host.ds) {
                self.scene.fun_18cb_0002(&mut host);
                if host.ds[0x01f4] == 0 {
                    fun_2449_0a3c(&mut host.ds);
                }
            }
            self.ds = host.ds;
            self.sheets = host.sheets;
            self.rng = host.rng;
            self.run();
        }
    }

    /// Tokens to draw, back to front.
    #[must_use]
    pub fn sprites(&self) -> Vec<ArmySprite> {
        self.scene
            .widgets
            .iter()
            .rev()
            .filter(|w| ww(w, 0) & 0x248 == 0x248 && w[0x26] & 0x80 == 0)
            .map(|w| ArmySprite {
                resource: u32::from_le_bytes([w[2], w[3], w[4], w[5]]) as u16,
                frame: w[0x26],
                x: ww(w, 0x12) as i16,
                y: ww(w, 0x10) as i16,
                mirrored: w[0] & 1 != 0,
            })
            .collect()
    }
}
