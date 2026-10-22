//! Single combat on the original scene engine.
//!
//! The original animates a duel in the field scene: `FUN_2d65_13e2` spawns
//! the two champions as script widgets (script 0), the scene loop
//! `FUN_18cb_0002` runs them, and script handler 15 calls the combat step
//! `FUN_20b5_1dff`. This module drives those ported pieces
//! ([`ScriptScene`], [`combat::script_hook`]) over a DGROUP image and lists
//! what to draw. It knows nothing about pixels: widgets are drawn by the
//! presenter from their sprite resource and frame.
//!
//! The field scene is built by the ported room builder
//! ([`room_builder::fun_2b79_0003`]), whose room base places the
//! champions. Not ported here, so approximated: the battle panel and the "click to start" wait of opcode 7, the
//! target rout check of combat opcode 5 (never routs), the stale
//! `[BP-0x10]` word of the combat step (0), and the arguments of the battle
//! message calls (the format offset is ported; names are filled in as the
//! target for wounds, conceding and death, the actor otherwise, with the
//! second `%S` as that person's pronoun).

#![forbid(unsafe_code)]

use crate::combat::{self, CombatEvent, CombatWorld};
use crate::resources::ResourceManager;
use crate::room::{Cut, SceneImages};
use crate::room_builder::{self, RoomBuild, RoomHost};
use crate::script::{ScriptEvent, ScriptHost, ScriptScene, ScriptWorld, Widget};
use crate::text::{format_message, Arg};

/// The scene clock `DS:7E3E` runs at 60 ticks per second (script waits are
/// `seconds * 0x3C`).
pub const CLOCK_HZ: u32 = 60;
const CLOCK: usize = 0x7e3e;
const CHAR: usize = 0x2c5e;
const CHAR_SIZE: usize = 0x38;

fn word(ds: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([ds[at], ds[at + 1]])
}

fn c_string(ds: &[u8], at: usize) -> String {
    ds[at..].iter().take_while(|&&b| b != 0).map(|&b| char::from(b)).collect()
}

pub use room_builder::scene_record;

/// One widget to draw, back to front.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sprite {
    /// `ANI.RES` frame bank, or a still image when `still`.
    pub resource: u16,
    pub still: bool,
    /// The image handle (see [`SceneImages::image`]).
    pub image: u32,
    pub frame: u8,
    pub x: i16,
    pub y: i16,
    pub mirrored: bool,
    /// Colour remap (widget `+0x0C`; 0 = none), see [`Duel::remap_table`].
    pub remap: u16,
}

struct Host<'a> {
    ds: Vec<u8>,
    resources: &'a mut ResourceManager,
    images: SceneImages,
    rng: u32,
    messages: Vec<String>,
    sounds: Vec<u16>,
    /// Clock reads since the frame began.
    reads: u32,
    /// A champion died, conceded or left the fight.
    over: bool,
}

impl Host<'_> {
    fn clock_value(&self) -> u32 {
        u32::from(word(&self.ds, CLOCK)) | u32::from(word(&self.ds, CLOCK + 2)) << 16
    }

    fn next_random(rng: &mut u32, n: u16) -> u16 {
        // Stand-in for the timer-seeded FUN_15b8_17e0 (seed DS:01FC).
        *rng = rng.wrapping_mul(0x015a_4e35).wrapping_add(1);
        if n == 0 { 0 } else { ((*rng >> 16) % u32::from(n)) as u16 }
    }

    fn name(&self, id: u8) -> String {
        crate::names::display_name(&self.ds, id)
    }

    fn pronoun(&self, id: u8) -> String {
        let feminine = word(&self.ds, CHAR + usize::from(id) * CHAR_SIZE + 0x16) & 0x800 != 0;
        // DS:79E0: "he" / "she", capitalised as the original shows it
        // ("Sir Constantine fails to parry. He is wounded.").
        let mut p = c_string(&self.ds, usize::from(word(&self.ds, 0x79e0 + 4 * usize::from(feminine))));
        if let Some(first) = p.get_mut(..1) {
            first.make_ascii_uppercase();
        }
        p
    }

    fn message(&mut self, format: u16, actor: u8, target: u8) {
        let subject = match format {
            0x61b4 | 0x6177 | 0x6193 => actor,
            _ => target,
        };
        let args = [Arg::Text(self.name(subject)), Arg::Text(self.pronoun(subject))];
        let text = format_message(&c_string(&self.ds, usize::from(format)), &args, |_| None);
        self.messages.push(text);
    }
}

impl ScriptWorld for Host<'_> {
    fn random(&mut self, n: u16) -> u16 {
        Self::next_random(&mut self.rng, n)
    }
}

struct Combat<'a> {
    rng: &'a mut u32,
}

impl CombatWorld for Combat<'_> {
    fn random(&mut self, n: u16) -> u16 {
        Host::next_random(self.rng, n)
    }
    fn rout_check(&mut self) -> bool {
        false
    }
    fn location_of(&self, ds: &[u8], id: u8) -> u8 {
        crate::fight::fun_2d65_0a2a(ds, &[], id)
    }
}

impl combat::DropHost for Host<'_> {
    fn dgroup(&mut self) -> &mut [u8] {
        &mut self.ds
    }
}

impl RoomHost for Host<'_> {
    fn cut_image(&mut self, _src: u32, res: u16, sx: u16, sy: u16, width: u16, height: u16) -> u32 {
        self.images.cut(Cut { res, sx, sy, width, height })
    }
    fn free_image(&mut self, _image: u32) {}
}

impl ScriptHost for Host<'_> {
    fn clock(&mut self) -> u32 {
        // In the original the timer interrupt keeps counting while a script
        // loops without waiting, so FUN_18cb_066a's 60-tick deadline ends
        // the loop. Model that as one tick per 4096 clock reads.
        self.reads += 1;
        if self.reads % 4096 == 0 {
            let clock = self.clock_value().wrapping_add(1);
            self.ds[CLOCK..CLOCK + 4].copy_from_slice(&clock.to_le_bytes());
        }
        self.clock_value()
    }
    fn program(&self, script: u8) -> Vec<u16> {
        // DS:1E2A far pointers; the scripts live in DGROUP.
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
        self.images.sheet(image)
    }
    fn load_image(&mut self, _old: u32, res: u16) -> u32 {
        self.load_sprite(res, 0)
    }
    fn load_sprite(&mut self, res: u16, _variant: u16) -> u32 {
        self.images.load_sprite(res, self.resources)
    }
    fn static_image(&mut self, res: u16) -> (u32, u16, u16) {
        self.images.static_image(res, self.resources)
    }
    fn retain_image(&mut self, _image: u32) {}
    fn release_image(&mut self, _flags: u16, _image: u32) {}
    fn event(&mut self, event: ScriptEvent) {
        if let ScriptEvent::Sound(n) = event {
            self.sounds.push(n);
        }
    }
    fn hook(&mut self, handler: u16, arg: u16, scene: &mut ScriptScene) -> u8 {
        if handler != 15 {
            // Talk (FUN_26c5_071a) is not part of a duel.
            return 2;
        }
        let actor = scene.widgets[scene.current as usize];
        let (aid, tid) = (actor[0x0a], actor[0x2e]);
        let mut world = Combat { rng: &mut self.rng };
        let (ret, mut events) = combat::script_hook(scene, arg, &mut self.ds, 0, &mut world);
        let dead: Vec<u8> = events
            .iter()
            .filter_map(|e| if let CombatEvent::DeadUnit(id) = e { Some(*id) } else { None })
            .collect();
        for id in dead {
            events.extend(combat::fun_20b5_1057(scene, id, self));
        }
        for e in events {
            match e {
                CombatEvent::Message(f) => self.message(f, aid, tid),
                CombatEvent::Sound(n) => self.sounds.push(n),
                _ => {}
            }
        }
        ret
    }
}

/// A single combat between two characters (1-based ids).
pub struct Duel {
    pub scene: ScriptScene,
    ds: Vec<u8>,
    pub images: SceneImages,
    /// What the room builder drew for the field scene.
    pub build: RoomBuild,
    rng: u32,
    /// `SCENES.RES` record of the field scene behind the fight.
    pub scene_id: u16,
    pub champions: [u8; 2],
    /// Battle messages, oldest first.
    pub messages: Vec<String>,
    pub sounds: Vec<u16>,
    /// Clock ticks not yet run (the original runs a frame per loop pass).
    pending: u32,
    over: bool,
}

impl Duel {
    /// Set up the field scene for `party` against `foe` on DGROUP `ds`
    /// (the live map state): battle lists `DS:9DC3` / `DS:9DD7`, field
    /// type `field` (`DS:9DF5`), automatic orders (`DS:01F2 = 0`).
    /// `scenes` reads a `SCENES.RES` record (see [`scene_record`] for the
    /// one this field uses).
    pub fn start(
        ds: &[u8],
        resources: &mut ResourceManager,
        scenes: impl FnOnce(u16) -> Option<Vec<u8>>,
        party: u8,
        foe: u8,
        field: u8,
        seed: u32,
    ) -> Self {
        let mut ds = ds.to_vec();
        for at in 0x9dc3..0x9dc3 + 0x28 {
            ds[at] = 0;
        }
        ds[0x9dc3] = party;
        ds[0x9dc3 + 0x14] = foe;
        ds[0x9dac] = 1;
        ds[0x9dad] = 1;
        ds[0x9dab] = 0;
        ds[0x9df5] = field;
        ds[0x01f2] = 0;
        // Champions and their list slots (DS:9C4E/9C4F, DS:9C52/9C53), as
        // the fight check leaves them; single combat (DS:9C56 = 1); the
        // battle-start counter DS:5E37 as recorded in the original duel.
        ds[0x9c4e] = party;
        ds[0x9c4f] = foe;
        ds[0x9c52] = 0;
        ds[0x9c53] = 0x14;
        ds[0x9c56..0x9c58].copy_from_slice(&1u16.to_le_bytes());
        ds[0x9dae] = party;
        ds[0x9db0] = foe;
        ds[0x5e37] = 2;
        ds[0x01ee] = 0;
        // In a field scene (DS:9DF6 = 1 in the recorded duel; it enables
        // FUN_20b5_1057's drops).
        ds[0x9df6] = 1;
        ds[0x01f5] = 0;
        let scene_id = scene_record(&ds, field, 0);
        let mut host = Host { ds, resources, images: SceneImages::default(), rng: seed, messages: Vec::new(), sounds: Vec::new(), reads: 0, over: false };
        let mut scene = ScriptScene { clip: [0, 0, 200, 320], ..ScriptScene::default() };
        // Widget 0 is never run by the scene loop; the room builder's
        // backdrop normally fills it.
        let mut backdrop = [0u8; 0x36];
        backdrop[0x0f] = 0xff;
        scene.widgets.push(backdrop);
        // FUN_2d65_13e2: FUN_2b79_0ea5 (DS:9DC1 = found), then
        // FUN_2b79_0003(2) and the characters on its room base.
        let mut build = RoomBuild::default();
        if let Some(record) = scenes(scene_id).filter(|r| r.len() >= 2) {
            room_builder::fun_2b79_0ea5(&mut host.ds, scene_id, &record, true);
            host.ds[0x9dc1] = 1;
            let mut ds = std::mem::take(&mut host.ds);
            build = room_builder::fun_2b79_0003(&mut scene, &mut ds, &record, 2, &mut host);
            host.ds = ds;
        }
        let snapshot = host.ds.clone();
        scene.fun_2d65_13e2_characters(&snapshot, build.base as u8, 0, &mut host);
        let Host { ds, images, rng, messages, sounds, .. } = host;
        Self { scene, ds, images, build, rng, scene_id, champions: [party, foe], messages, sounds, pending: 0, over: false }
    }

    /// DGROUP after the fight so far (hit points live in the character
    /// records).
    #[must_use]
    pub fn ds(&self) -> &[u8] {
        &self.ds
    }

    /// The 32-entry colour remap for a sprite's `remap` value: the scene
    /// draw passes widget `+0x0C` to the blitter `FUN_14b0_0bbf`, which
    /// hands the driver the far pointer `DS:29A4[remap]`; MCGA.DVR `drv_0`
    /// (flag 2) then draws every non-zero source pixel `p` as `table[p]`.
    /// 0 means no remap (the blitter clears flag 2).
    #[must_use]
    pub fn remap_table(&self, remap: u16) -> Option<[u8; 32]> {
        if remap == 0 {
            return None;
        }
        let at = 0x29a4 + 4 * usize::from(remap);
        let off = usize::from(word(&self.ds, at));
        self.ds.get(off..off + 32).and_then(|t| t.try_into().ok())
    }

    /// The resource record of a still image the scene loaded.
    #[must_use]
    pub fn still(&self, resource: u16) -> Option<&[u8]> {
        self.images.still(resource)
    }

    /// The `ANI.RES` frame bank of a sprite resource the scene loaded.
    #[must_use]
    pub fn bank(&self, resource: u16) -> Option<&[u8]> {
        self.images.bank(resource)
    }

    /// Current hit points of a champion (`name + 0x2C`).
    #[must_use]
    pub fn hit_points(&self, id: u8) -> u8 {
        self.ds[CHAR + usize::from(id) * CHAR_SIZE + 0x2c]
    }

    /// The fight is over: `FUN_20b5_1705` ended the battle and its
    /// three-second close has passed.
    #[must_use]
    pub const fn finished(&self) -> bool {
        self.over
    }

    /// Advance by `ticks` clock ticks (60 per second), one scene frame per
    /// tick. Returns the redraw rects of the last frame.
    pub fn advance(&mut self, ticks: u32, resources: &mut ResourceManager) -> Vec<[i16; 4]> {
        self.pending += ticks;
        let mut host = Host {
            ds: std::mem::take(&mut self.ds),
            resources,
            images: std::mem::take(&mut self.images),
            rng: self.rng,
            messages: Vec::new(),
            sounds: Vec::new(),
            reads: 0,
            over: self.over,
        };
        let mut dirty = Vec::new();
        while self.pending > 0 && !host.over {
            self.pending -= 1;
            let clock = host.clock_value().wrapping_add(1);
            host.ds[CLOCK..CLOCK + 4].copy_from_slice(&clock.to_le_bytes());
            host.reads = 0;
            dirty = self.scene.fun_18cb_0002(&mut host);
            // FUN_20b5_1705 ended the battle (DS:01EE); the battle loop
            // closes once the clock passes DS:0322 (three seconds later).
            let close = u32::from(word(&host.ds, 0x0322)) | u32::from(word(&host.ds, 0x0324)) << 16;
            if host.ds[0x01ee] != 0 && host.clock_value() >= close {
                host.over = true;
            }
        }
        self.ds = host.ds;
        self.images = host.images;
        self.rng = host.rng;
        self.over = host.over;
        if self.over {
            self.pending = 0;
        }
        self.messages.extend(host.messages);
        self.sounds.extend(host.sounds);
        dirty
    }

    /// Widgets to draw, back to front (the original draws the list from
    /// its end to its start). Hidden frames (bit 7) and parked widgets are
    /// left out.
    #[must_use]
    pub fn sprites(&self) -> Vec<Sprite> {
        self.scene
            .widgets
            .iter()
            .rev()
            .filter_map(|w: &Widget| {
                let res = u32::from_le_bytes([w[2], w[3], w[4], w[5]]);
                let x = i16::from_le_bytes([w[0x12], w[0x13]]);
                if res == 0 || w[0x26] & 0x80 != 0 || x >= 0x270f {
                    return None;
                }
                Some(Sprite {
                    resource: res as u16,
                    still: res & crate::room::STILL != 0,
                    image: res,
                    frame: w[0x26],
                    x,
                    y: i16::from_le_bytes([w[0x10], w[0x11]]),
                    mirrored: w[0] & 1 != 0,
                    remap: u16::from_le_bytes([w[0x0c], w[0x0d]]),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Session;
    use std::path::Path;

    /// Constantine (1) against Lupinus (62) on field type 73, as recorded
    /// in the original: both champions spawn and the fight runs to an end.
    #[test]
    fn duel_runs_to_an_end() {
        let game = Path::new("../../game");
        let exe = Path::new("../../ghidra/EXCAL_UNPACKED.EXE");
        if !game.join("EPISODE1.DAT").exists() || !exe.exists() {
            eprintln!("SKIP: game data absent");
            return;
        }
        let mut session = Session::load(game, exe, 1).unwrap();
        let mut ds = session.dgroup().unwrap().to_vec();
        // Lay the save over DGROUP as the game does.
        let save = session.state.to_bytes();
        ds[0x2c5c..0x2c5c + 0x2360].copy_from_slice(&save[..0x2360]);
        // Give both champions an item that can drop (state bits 0x18, an
        // item with an image and no 0xF000 flags), to exercise
        // FUN_20b5_1057 whoever loses.
        let item = (0..0x55usize)
            .map(|i| ds[0x4dd1 + i * 4])
            .find(|&it| {
                let def = 0x488b + usize::from(it) * 0x1b;
                u16::from_le_bytes([ds[def + 0x18], ds[def + 0x19]]) & 0xf000 == 0
                    && u16::from_le_bytes([ds[def + 0x12], ds[def + 0x13]]) != 0
            })
            .expect("a droppable item");
        for (slot, owner) in [(0usize, 1u8), (1, 62)] {
            let e = 0x4dd1 + slot * 4;
            ds[e] = item;
            ds[e + 1] = owner;
            ds[e + 3] = 0x18;
        }
        let record = session.scene_record(907).ok();
        let mut duel = Duel::start(&ds, session.resources_mut(), |_| record, 1, 62, 73, 2);
        assert_eq!(duel.scene_id, 907);
        let champions = |duel: &Duel| duel.scene.widgets.iter().filter(|w| w[0] & 0x40 != 0 && w[0x0f] < 0xc8 && (w[0x0a] == 1 || w[0x0a] == 62)).count();
        assert_eq!(champions(&duel), 2, "two champions");
        // Scene 907's army soldiers (0x429..=0x42F) stay out of a duel.
        assert!(duel.build.background.iter().all(|s| !(0x429..=0x42f).contains(&s.res)));
        let field_stills = duel.sprites().iter().filter(|s| s.still).count();
        let mut seconds = 0;
        while !duel.finished() && seconds < 300 {
            duel.advance(CLOCK_HZ, session.resources_mut());
            seconds += 1;
        }
        for m in &duel.messages {
            eprintln!("{m}");
        }
        eprintln!("after {seconds}s: hp {} / {}", duel.hit_points(1), duel.hit_points(62));
        assert!(!duel.messages.is_empty(), "the champions exchanged blows");
        assert!(duel.finished(), "the duel ended");
        let stills = duel.sprites().iter().filter(|s| s.still).count() - field_stills;
        eprintln!("dropped items on the field: {stills}");
        assert_eq!(stills, 1, "seed 2: the loser drops its item");
        for s in duel.sprites().iter().filter(|s| s.still) {
            assert!(duel.still(s.resource).is_some(), "dropped item image loaded");
        }
    }
}
