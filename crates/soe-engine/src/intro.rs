//! The start-up intro: the lake, the sword rising, lightning and the title
//! (`FUN_2c74_0006`, called with 1 from `FUN_2d65_2cd9` at `2d65:2e3c`).
//!
//! The original is straight-line code with timed waits; here it is a state
//! machine that stops at every wait and resumes when the game clock
//! (`DS:7E3E`, one count per 18.2 Hz tick) reaches the deadline. Drawing
//! uses three surfaces as the original does: the screen (`DS:962C`), the
//! back buffer (`DS:9680`) and a 320x200 work buffer from `FUN_14b0_0060`.
//!
//! Callees ported here: the wait `FUN_2c74_0b31` (palette cycling of colours
//! 11..15 while `DS:9D30` is set, key/click skip into `DS:9DBB`), the
//! ripple strip `FUN_2c74_0add`, the rectangle rules of the blitter
//! `FUN_14b0_0bbf`, the palette writers `FUN_1316_1302`/`FUN_1316_1169`,
//! and the MCGA.DVR fades behind `FUN_1316_1111` (`drv_8`) and
//! `FUN_1316_1143` (`drv_7` to black). The argument-0 branch (the ending's
//! "As %S takes the eldritch blade" text) is not ported.

#![forbid(unsafe_code)]

use std::collections::HashMap;

use soe_assets::Palette;
use soe_video::{IndexedFrame, LOGICAL_HEIGHT, LOGICAL_WIDTH, PlanarImage};

/// `DS:73AC`: x of the splash frames in the second phase.
const SPLASH_X: [u8; 11] = [101, 94, 91, 88, 88, 85, 86, 88, 91, 94, 101];
/// `DS:73B7`: y of the splash frames.
const SPLASH_Y: [u8; 11] = [16, 9, 6, 3, 3, 1, 1, 3, 6, 9, 16];
/// `DS:73C2`: lightning frame (1-based, 0 = none) per step of the third phase.
const BOLT: [u8; 20] = [1, 2, 1, 2, 3, 4, 3, 0, 1, 2, 3, 4, 3, 2, 1, 0, 1, 2, 1, 0];

/// TITLE.RES ids the intro loads with `FUN_14b0_0122`.
pub const LAKE: u16 = 0x78;
pub const BLADE: u16 = 0x76;
pub const TITLE: u16 = 0x77;
/// Splash frames 100..=106 (played 100..106, 103..100).
pub const SPLASH: u16 = 100;
/// Ripple strip 0x6B..=0x6E (`DS:9D20`).
pub const RIPPLE: u16 = 0x6b;
/// Hand frames 0x6F..=0x72.
pub const HAND: u16 = 0x6f;
/// Water around the rising blade, 0x73/0x74.
pub const WAKE: u16 = 0x73;
/// Lightning 0x7B..=0x7E.
pub const BOLT_IMAGE: u16 = 0x7b;

/// Every image id the intro needs.
#[must_use]
pub fn image_ids() -> Vec<u16> {
    let mut ids = vec![LAKE, BLADE, TITLE, WAKE, WAKE + 1];
    ids.extend(SPLASH..SPLASH + 7);
    for i in 0..4 {
        ids.extend([RIPPLE + i, HAND + i, BOLT_IMAGE + i]);
    }
    ids
}

/// Sound requests made by the intro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntroEvent {
    /// `FUN_1316_187b(name, 1, 0)`.
    Music(&'static str),
    /// `FUN_1316_181d(level)` then `FUN_1316_183c(sound)`.
    Sound { level: u8, sound: u16 },
    /// The intro has returned (after the fade to black).
    Finished,
}

/// A 320x200 indexed surface (screen, back buffer, work buffer).
#[derive(Clone)]
struct Surface {
    pixels: Vec<u8>,
}

impl Surface {
    fn new() -> Self {
        Self {
            pixels: vec![0; usize::from(LOGICAL_WIDTH) * usize::from(LOGICAL_HEIGHT)],
        }
    }
}

#[derive(Clone, Copy)]
enum Src {
    Image(u16),
    Work,
    Back,
}

#[derive(Clone, Copy)]
enum Dst {
    Screen,
    Back,
    Work,
}

/// Rectangle as `FUN_14b0_0bbf` keeps it: top, left, bottom, right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rect {
    top: i16,
    left: i16,
    bottom: i16,
    right: i16,
}

/// `FUN_30ec_0006`: intersect; true when not empty.
fn clip(a: Rect, bounds: Rect) -> (Rect, bool) {
    let r = Rect {
        top: a.top.max(bounds.top),
        left: a.left.max(bounds.left),
        bottom: a.bottom.min(bounds.bottom),
        right: a.right.min(bounds.right),
    };
    (r, r.top < r.bottom && r.left < r.right)
}

/// Widen a rectangle to whole 8-pixel columns as `14b0:0bbf` does for
/// copies that are not fills or transparent sprites: the left edge is
/// rounded down and `right + (left & 7) + 7` rounded down.
fn align(r: Rect) -> Rect {
    let low = r.left & 7;
    Rect {
        left: r.left & !7,
        right: r.right.wrapping_add(low).wrapping_add(7) & !7,
        ..r
    }
}

/// The source and destination rectangles `FUN_14b0_0bbf` hands the driver
/// for `(sx, sy, w, h)` -> `(dx, dy)`, or `None` when nothing is drawn.
/// `src_size`/`dst_size` are (width, height).
#[allow(clippy::too_many_arguments)]
fn blit_rects(
    src_size: (i16, i16),
    sx: i16,
    sy: i16,
    dst_size: (i16, i16),
    dx: i16,
    dy: i16,
    w: i16,
    h: i16,
    flags: u16,
) -> Option<(Rect, Rect)> {
    let mut src = Rect { top: sy, left: sx, bottom: sy.wrapping_add(h), right: sx.wrapping_add(w) };
    let mut dst = Rect { top: dy, left: dx, bottom: dy.wrapping_add(h), right: dx.wrapping_add(w) };
    // Bit 0x100 (source header byte 0 bit 7) is never set for these images.
    if flags & 0x10 == 0 && flags & 0x20 == 0 && flags & 0x40 == 0 {
        src = align(src);
        dst = align(dst);
    }
    let off_y = src.top.wrapping_sub(dst.top);
    let off_x = src.left.wrapping_sub(dst.left);
    if flags & 4 != 0 {
        let (r, ok) = clip(src, Rect { top: 0, left: 0, bottom: src_size.1, right: src_size.0 });
        if !ok {
            return None;
        }
        src = r;
    }
    let (r, ok) = clip(dst, Rect { top: 0, left: 0, bottom: dst_size.1, right: dst_size.0 });
    if !ok {
        return None;
    }
    let shifted = Rect {
        top: r.top.wrapping_add(off_y),
        left: r.left.wrapping_add(off_x),
        bottom: r.bottom.wrapping_add(off_y),
        right: r.right.wrapping_add(off_x),
    };
    let (src, ok) = clip(src, shifted);
    if !ok {
        return None;
    }
    let dst = Rect {
        top: src.top.wrapping_sub(off_y),
        left: src.left.wrapping_sub(off_x),
        bottom: src.bottom.wrapping_sub(off_y),
        right: src.right.wrapping_sub(off_x),
    };
    Some((src, dst))
}

/// 4-bit palette component to the DAC, as MCGA.DVR's table at `CS:0900`.
fn dac_from_nibble(v: u8) -> u8 {
    ((u16::from(v & 15) * 63 + 7) / 15) as u8
}

/// 4-bit component to the DAC as `FUN_1316_1169` computes it for VGA
/// (`(v + 1) * 4 + (v >> 2) - 4`).
fn dac_direct(v: u8) -> u8 {
    v.wrapping_add(1).wrapping_mul(4).wrapping_add(v >> 2).wrapping_sub(4)
}

/// A palette fade in progress (MCGA.DVR `09c3`): one step per vertical
/// retrace, at most 0x40 steps, each component moving 2 towards the target.
#[derive(Clone)]
struct Fade {
    target: [u8; 96],
    steps_left: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pc {
    Start,
    /// After the fade-in: `FUN_2c74_0b31(0x1E0)`.
    Hold,
    Rise(i16),
    Ripples1(u8),
    Splash(i16),
    Ripples2(u8),
    StartStorm,
    Storm(i16),
    /// The `i == 7` flash: the fade back is done, wait out the step.
    StormFlashed(i16),
    Linger(i16),
    Sink(i16),
    Final,
    FinalWait,
    FadeOut,
    Done,
}

/// What the machine is blocked on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Block {
    None,
    /// `FUN_2c74_0b31` until the clock reaches the deadline.
    Wait(u32),
    /// A synchronous driver fade, then continue at `pc`.
    Fade,
}

/// The intro player.
pub struct Intro {
    images: HashMap<u16, PlanarImage>,
    screen: Surface,
    back: Surface,
    work: Surface,
    /// The palette table at `DS:1FD9` (4-bit RGB), rotated by the cycler.
    table: [u8; 96],
    /// DAC contents, 6-bit.
    dac: [u8; 96],
    fade: Option<Fade>,
    /// Game clock `DS:7E3E`.
    clock: u32,
    /// `DS:9D1C`: next palette-cycle time.
    cycle_at: u32,
    /// `DS:9D30`.
    cycling: bool,
    /// `DS:9DBB`.
    skipped: bool,
    /// A key or click waiting to be seen by the next poll.
    key_pending: bool,
    pc: Pc,
    block: Block,
    /// Ripple sub-step of `FUN_2c74_0add` (0..3) inside `Ripples*`/`Linger`.
    sub: u8,
    /// Deadline of the current loop step (`local_8`/`local_6`).
    step_deadline: u32,
    /// Retraces per tick in units of 1/100 (70 Hz / 18.2 Hz), or `None` for
    /// fades that take no game time (as under the emulator oracle).
    retrace_rate: Option<u32>,
    retrace_accum: u32,
    events: Vec<IntroEvent>,
    /// Frames sampled where the oracle samples (wait entries and cycle steps).
    pub samples: Option<Vec<(u32, Vec<u8>, [u8; 96])>>,
}

impl Intro {
    /// Build the intro from its images and the 96-byte table at `DS:1FD9`.
    /// `dac` is the DAC before the intro (6-bit, 32 colours).
    #[must_use]
    pub fn new(images: HashMap<u16, PlanarImage>, table: [u8; 96], dac: [u8; 96]) -> Self {
        Self {
            images,
            screen: Surface::new(),
            back: Surface::new(),
            work: Surface::new(),
            table,
            dac,
            fade: None,
            clock: 0,
            cycle_at: 0,
            cycling: false,
            skipped: false,
            key_pending: false,
            pc: Pc::Start,
            block: Block::None,
            sub: 0,
            step_deadline: 0,
            retrace_rate: Some(385),
            retrace_accum: 0,
            events: Vec::new(),
            samples: None,
        }
    }

    /// Let driver fades complete without using game time (oracle timing).
    pub fn set_instant_fades(&mut self) {
        self.retrace_rate = None;
    }

    /// Start the game clock at `clock` (the oracle's `DS:7E3E` on entry)
    /// with `DS:9D1C` = `cycle_at`.
    pub fn set_clock(&mut self, clock: u32, cycle_at: u32) {
        self.clock = clock;
        self.cycle_at = cycle_at;
    }

    #[must_use]
    pub fn finished(&self) -> bool {
        self.pc == Pc::Done
    }

    /// A key press or mouse click (`FUN_3360_0000` / `DS:7D3E`).
    pub fn skip(&mut self) {
        self.key_pending = true;
    }

    /// The screen as an indexed frame with the current DAC as its palette.
    #[must_use]
    pub fn frame(&self) -> IndexedFrame {
        IndexedFrame::new(LOGICAL_WIDTH, LOGICAL_HEIGHT, self.screen.pixels.clone(), self.palette())
            .expect("logical frame dimensions")
    }

    /// The DAC as 8-bit colours.
    #[must_use]
    pub fn palette(&self) -> Palette {
        Palette::new(
            self.dac
                .chunks(3)
                .map(|c| [c[0], c[1], c[2]].map(|v| (v << 2) | (v >> 4)))
                .collect(),
        )
    }

    /// Start: run up to the first block.
    pub fn start(&mut self) -> Vec<IntroEvent> {
        self.run();
        std::mem::take(&mut self.events)
    }

    /// Advance one game tick.
    pub fn tick(&mut self) -> Vec<IntroEvent> {
        if self.pc != Pc::Done {
            self.clock = self.clock.wrapping_add(1);
            match self.block {
                Block::Fade => {
                    let steps = match self.retrace_rate {
                        Some(rate) => {
                            self.retrace_accum += rate;
                            let n = self.retrace_accum / 100;
                            self.retrace_accum %= 100;
                            n
                        }
                        None => u32::MAX,
                    };
                    for _ in 0..steps {
                        if !self.fade_step() {
                            self.block = Block::None;
                            self.run();
                            break;
                        }
                    }
                }
                Block::Wait(deadline) => {
                    // The tick nearly always lands in the key check of the
                    // `0b31` loop, so the deadline test after it sees the new
                    // clock before the next pass's cycle test does.
                    if self.clock < deadline && !self.skipped {
                        self.poll();
                    }
                    if self.clock >= deadline || self.skipped {
                        self.block = Block::None;
                        self.run();
                    }
                }
                Block::None => self.run(),
            }
        }
        std::mem::take(&mut self.events)
    }

    // --- drawing -----------------------------------------------------------

    fn size_of(&self, src: Src) -> (i16, i16) {
        match src {
            Src::Image(id) => self
                .images
                .get(&id)
                .map_or((0, 0), |i| (i.width as i16, i.height as i16)),
            Src::Work | Src::Back => (LOGICAL_WIDTH as i16, LOGICAL_HEIGHT as i16),
        }
    }

    /// `FUN_14b0_0bbf(src, sx, sy, dst, dx, dy, w, h, 0, flags)`. Flag 0x40
    /// skips source index 0; other flags copy.
    #[allow(clippy::too_many_arguments)]
    fn blit(&mut self, src: Src, sx: i16, sy: i16, dst: Dst, dx: i16, dy: i16, w: i16, h: i16, flags: u16) {
        let src_size = self.size_of(src);
        let dst_size = (LOGICAL_WIDTH as i16, LOGICAL_HEIGHT as i16);
        let Some((s, d)) = blit_rects(src_size, sx, sy, dst_size, dx, dy, w, h, flags) else {
            return;
        };
        let transparent = flags & 0x40 != 0;
        let src_pixels: Vec<u8> = match src {
            Src::Image(id) => match self.images.get(&id) {
                Some(image) => image.indices.clone(),
                None => return,
            },
            Src::Work => self.work.pixels.clone(),
            Src::Back => self.back.pixels.clone(),
        };
        let src_w = src_size.0 as usize;
        let target = match dst {
            Dst::Screen => &mut self.screen,
            Dst::Back => &mut self.back,
            Dst::Work => &mut self.work,
        };
        let width = usize::from(LOGICAL_WIDTH);
        for row in 0..(s.bottom - s.top) as usize {
            let sy = s.top as usize + row;
            let dy = d.top as usize + row;
            for col in 0..(s.right - s.left) as usize {
                let p = src_pixels[sy * src_w + s.left as usize + col];
                if transparent && p == 0 {
                    continue;
                }
                target.pixels[dy * width + d.left as usize + col] = p;
            }
        }
    }

    fn image(&mut self, id: u16, sx: i16, sy: i16, dst: Dst, dx: i16, dy: i16, w: i16, h: i16, flags: u16) {
        self.blit(Src::Image(id), sx, sy, dst, dx, dy, w, h, flags);
    }

    // --- palette -----------------------------------------------------------

    /// `FUN_1316_1111`: fade into the table (`drv_8`).
    fn fade_in(&mut self) {
        let mut target = [0u8; 96];
        for (t, v) in target.iter_mut().zip(self.table) {
            *t = dac_from_nibble(v);
        }
        self.begin_fade(target);
    }

    /// `FUN_1316_1143`: fade to black (`drv_7(0, 0, 0)`).
    fn fade_out(&mut self) {
        self.begin_fade([dac_from_nibble(0); 96]);
    }

    fn begin_fade(&mut self, target: [u8; 96]) {
        self.fade = Some(Fade { target, steps_left: 0x40 });
        self.block = Block::Fade;
        if self.retrace_rate.is_none() {
            while self.fade_step() {}
            self.block = Block::None;
        }
    }

    /// One pass of `09c3`; false once the fade is over.
    fn fade_step(&mut self) -> bool {
        let Some(fade) = self.fade.as_mut() else {
            return false;
        };
        let mut changed = false;
        for (cur, &target) in self.dac.iter_mut().zip(fade.target.iter()) {
            let c = *cur as i8;
            let t = target as i8;
            *cur = if c.wrapping_sub(1) > t {
                changed = true;
                c.wrapping_sub(2) as u8
            } else if c.wrapping_add(1) < t {
                changed = true;
                c.wrapping_add(2) as u8
            } else {
                target
            };
        }
        fade.steps_left -= 1;
        if !changed || fade.steps_left == 0 {
            self.fade = None;
            return false;
        }
        true
    }

    /// `FUN_1316_1302(table)` on VGA: set the DAC straight from the table.
    fn set_palette_direct(&mut self) {
        for (d, v) in self.dac.iter_mut().zip(self.table) {
            *d = dac_direct(v);
        }
    }

    /// `FUN_1316_1302(0, 0, r, g, b, _)` on VGA: every colour = (r, g, b).
    fn set_palette_flat(&mut self, rgb: [u8; 3]) {
        for c in self.dac.chunks_mut(3) {
            for (d, v) in c.iter_mut().zip(rgb) {
                *d = dac_direct(v);
            }
        }
    }

    // --- the wait ----------------------------------------------------------

    /// One iteration of the `FUN_2c74_0b31` loop body.
    fn poll(&mut self) {
        // `DS:7D2A` byte 3 is the sound driver type; 6 disables cycling.
        if self.cycling && self.clock > self.cycle_at {
            self.cycle_at = self.clock.wrapping_add(15);
            // Rotate colours 11..=15 of the table up by one.
            let last = [self.table[45], self.table[46], self.table[47]];
            self.table.copy_within(33..45, 36);
            self.table[33..36].copy_from_slice(&last);
            self.set_palette_direct();
            self.sample();
        }
        if self.key_pending {
            self.key_pending = false;
            self.skipped = true;
        }
    }

    /// Enter `FUN_2c74_0b31` with this deadline: one poll, then block.
    fn wait_until(&mut self, deadline: u32) -> bool {
        self.sample();
        self.poll();
        if self.clock >= deadline || self.skipped {
            return false;
        }
        self.block = Block::Wait(deadline);
        true
    }

    fn sample(&mut self) {
        if let Some(samples) = self.samples.as_mut() {
            let same = samples
                .last()
                .is_some_and(|(_, p, d)| *p == self.screen.pixels && *d == self.dac);
            if !same {
                samples.push((self.clock, self.screen.pixels.clone(), self.dac));
            }
        }
    }

    /// `FUN_2c74_0add`, one of its three steps: ripple frame `sub` to the
    /// screen, then `FUN_2c74_0b31(8)`.
    fn ripple_step(&mut self, sub: u8) -> bool {
        self.image(RIPPLE + u16::from(sub), 0, 0, Dst::Screen, 0x5d, 0xb5, 0x140, 200, 0x46);
        self.wait_until(self.clock.wrapping_add(8))
    }

    /// Run `FUN_2c74_0add` `count` times from sub-step `self.sub`; true when
    /// blocked.
    fn ripples(&mut self) -> bool {
        while self.sub < 3 {
            let s = self.sub;
            self.sub += 1;
            if self.ripple_step(s) {
                return true;
            }
        }
        self.sub = 0;
        false
    }

    // --- the body ------------------------------------------------------------

    /// The rising (and sinking) blade frame at height `i`.
    fn blade_frame(&mut self, i: i16) {
        self.blit(Src::Work, 0x56, 0, Dst::Back, 0x56, 0, 0x24, 200, 4);
        self.image(BLADE, 0, 0, Dst::Back, 0x56, 0xb5 - i, 0x140, i, 0x46);
        let rows = i.min(0x13);
        let wake_y = if i < 0x6c { 0x46 } else { 0xb1 - i };
        self.image(WAKE + (i % 2) as u16, 0, wake_y, Dst::Back, 0x56, 0xb5, 0x140, rows, 0x46);
    }

    fn run(&mut self) {
        loop {
            if self.block != Block::None {
                return;
            }
            match self.pc {
                Pc::Start => {
                    self.events.push(IntroEvent::Music("title"));
                    self.image(LAKE, 0, 0, Dst::Screen, 0, 0, 0x140, 200, 4);
                    self.image(LAKE, 0, 0, Dst::Work, 0, 0, 0x140, 200, 4);
                    self.cycling = true;
                    self.pc = Pc::Hold;
                    self.fade_in();
                }
                Pc::Hold => {
                    self.pc = Pc::Rise(3);
                    self.wait_until(self.clock.wrapping_add(0x1e0));
                }
                Pc::Rise(i) => {
                    if i >= 0xb2 || self.skipped {
                        self.pc = Pc::Ripples1(0);
                        self.sub = 0;
                        continue;
                    }
                    self.step_deadline = self.clock.wrapping_add(3);
                    self.blade_frame(i);
                    if i > 0x78 && i < 0xb1 {
                        let (frame, mut y) = match i {
                            _ if i < 0x85 => (0, 0x126 - i),
                            _ if i < 0x91 => (1, 0x126 - i),
                            _ if i < 0x9d => (2, 0x126 - i),
                            _ => (3, 0x126 - i),
                        };
                        if frame == 3 {
                            if i > 0xa2 {
                                y = 0x92;
                            }
                            if i > 0xa6 {
                                y = 0x9e;
                            }
                        }
                        self.image(HAND + frame, 0, 0, Dst::Back, 0x55, y, 0x140, 200, 0x46);
                    }
                    self.blit(Src::Back, 0x56, 0, Dst::Screen, 0x56, 0, 0x24, 200, 0x4004);
                    self.pc = Pc::Rise(i + 6);
                    self.wait_until(self.step_deadline);
                }
                Pc::Ripples1(n) => {
                    if self.ripples() {
                        return;
                    }
                    if n == 0 {
                        self.pc = Pc::Ripples1(1);
                    } else {
                        self.events.push(IntroEvent::Sound { level: 0x28, sound: 7 });
                        self.pc = Pc::Splash(0);
                    }
                }
                Pc::Splash(i) => {
                    if i >= 0xc || self.skipped {
                        self.pc = Pc::Ripples2(0);
                        self.sub = 0;
                        continue;
                    }
                    self.step_deadline = self.clock.wrapping_add(4);
                    self.blit(Src::Work, 0x4c, 0, Dst::Back, 0x4c, 0, 0x3c, 0x3c, 4);
                    self.image(BLADE, 0, 0, Dst::Back, 0x56, 4, 0x140, 0xb1, 0x46);
                    if i < 0xb {
                        let k = i as usize;
                        let id = if k < 7 { SPLASH + k as u16 } else { SPLASH + (10 - k) as u16 };
                        self.image(id, 0, 0, Dst::Back, i16::from(SPLASH_X[k]), i16::from(SPLASH_Y[k]), 100, 200, 0x46);
                    }
                    self.image(RIPPLE + (i / 4) as u16, 0, 0, Dst::Screen, 0x5d, 0xb5, 0x140, 200, 0x46);
                    self.blit(Src::Back, 0x4c, 0, Dst::Screen, 0x4c, 0, 0x3c, 0x3c, 0x4004);
                    self.pc = Pc::Splash(i + 1);
                    self.wait_until(self.step_deadline);
                }
                Pc::Ripples2(n) => {
                    if self.ripples() {
                        return;
                    }
                    self.pc = if n == 0 { Pc::Ripples2(1) } else { Pc::StartStorm };
                }
                Pc::StartStorm => {
                    self.image(LAKE, 0, 0, Dst::Back, 0, 0, 0x140, 200, 4);
                    self.pc = Pc::Storm(0);
                }
                Pc::Storm(i) => {
                    if i >= 0x14 || self.skipped {
                        self.pc = Pc::Linger(0);
                        self.sub = 0;
                        continue;
                    }
                    self.step_deadline = self.clock.wrapping_add(6);
                    if i == 7 {
                        self.image(TITLE, 0, 0, Dst::Work, 0x14, 7, 0x140, 200, 0x46);
                        self.blit(Src::Work, 0, 0, Dst::Back, 0, 0, 0x140, 200, 4);
                    } else {
                        self.blit(Src::Work, 0x4c, 0, Dst::Back, 0x4c, 0, 0x3c, 0xa0, 4);
                    }
                    self.image(BLADE, 0, 0, Dst::Back, 0x56, 4, 0x140, 0xb1, 0x46);
                    let bolt = BOLT[i as usize];
                    if bolt != 0 {
                        self.events.push(IntroEvent::Sound { level: 0x3c, sound: 0xb });
                        self.image(BOLT_IMAGE + u16::from(bolt - 1), 0, 0, Dst::Back, 0x46, 4, 100, 200, 0x46);
                    }
                    if i == 7 {
                        self.events.push(IntroEvent::Sound { level: 0x7f, sound: 0xb });
                        self.set_palette_flat([0xe, 0xf, 0xe]);
                        self.blit(Src::Back, 0, 0, Dst::Screen, 0, 0, 0x140, 200, 4);
                        self.pc = Pc::StormFlashed(i);
                        self.fade_in();
                        continue;
                    }
                    self.blit(Src::Back, 0x4c, 0, Dst::Screen, 0x4c, 0, 0x3c, 0x78, 4);
                    self.pc = Pc::StormFlashed(i);
                }
                Pc::StormFlashed(i) => {
                    if i == 0xf {
                        // Two `FUN_2c74_0add` calls, then the step's wait.
                        if self.sub < 6 {
                            let s = self.sub % 3;
                            self.sub += 1;
                            if self.ripple_step(s) {
                                return;
                            }
                            continue;
                        }
                        self.sub = 0;
                    } else {
                        let r = ((i / 2) % 4) as u16;
                        self.image(RIPPLE + r, 0, 0, Dst::Screen, 0x5d, 0xb5, 0x140, 200, 0x46);
                    }
                    self.pc = Pc::Storm(i + 1);
                    self.wait_until(self.step_deadline);
                }
                Pc::Linger(n) => {
                    // Five `FUN_2c74_0add` calls while not skipped.
                    if n >= 5 || (self.skipped && self.sub == 0) {
                        self.pc = Pc::Sink(0xb1);
                        continue;
                    }
                    if self.ripples() {
                        return;
                    }
                    self.pc = Pc::Linger(n + 1);
                }
                Pc::Sink(i) => {
                    if i <= 2 || self.skipped {
                        self.pc = Pc::Final;
                        continue;
                    }
                    self.step_deadline = self.clock.wrapping_add(3);
                    self.blade_frame(i);
                    self.blit(Src::Back, 0x56, 0, Dst::Screen, 0x56, 0, 0x24, 200, 4);
                    self.pc = Pc::Sink(i - 6);
                    self.wait_until(self.step_deadline);
                }
                Pc::Final => {
                    self.blit(Src::Work, 0, 0, Dst::Screen, 0, 0, 0x140, 200, 4);
                    self.pc = Pc::FinalWait;
                    self.wait_until(self.clock.wrapping_add(0x168));
                }
                Pc::FinalWait => {
                    self.cycling = false;
                    self.pc = Pc::FadeOut;
                    self.fade_out();
                }
                Pc::FadeOut => {
                    self.pc = Pc::Done;
                    self.events.push(IntroEvent::Finished);
                }
                Pc::Done => return,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_round_out_to_eight_pixel_columns() {
        // 0x56..0x7A widens to 0x50..0x80 (0x7A + 6 + 7 = 0x87 -> 0x80).
        let (s, d) = blit_rects((320, 200), 0x56, 0, (320, 200), 0x56, 0, 0x24, 200, 4).unwrap();
        assert_eq!((s.left, s.right, d.left, d.right), (0x50, 0x80, 0x50, 0x80));
        // Transparent sprites keep their x.
        let (s, d) = blit_rects((33, 177), 0, 0, (320, 200), 0x56, 10, 0x140, 20, 0x46).unwrap();
        assert_eq!((s.left, s.right, s.top, s.bottom), (0, 33, 0, 20));
        assert_eq!((d.left, d.right, d.top, d.bottom), (0x56, 0x56 + 33, 10, 30));
    }

    #[test]
    fn clips_against_the_destination() {
        let (s, d) = blit_rects((33, 177), 0, 0, (320, 200), 0x56, 0xb5 - 3, 0x140, 3, 0x46).unwrap();
        assert_eq!((s.top, s.bottom, d.top, d.bottom), (0, 3, 0xb2, 0xb5));
        assert!(blit_rects((10, 10), 0, 0, (320, 200), 400, 0, 10, 10, 0x46).is_none());
    }

    #[test]
    fn dac_conversions() {
        let table: Vec<u8> = (0..16).map(dac_from_nibble).collect();
        assert_eq!(table, [0, 4, 8, 13, 17, 21, 25, 29, 34, 38, 42, 46, 50, 55, 59, 63]);
        assert_eq!(dac_direct(3), 12);
        assert_eq!(dac_direct(15), 63);
    }

    #[test]
    fn fade_moves_two_per_step() {
        let mut intro = Intro::new(HashMap::new(), [0; 96], [0; 96]);
        intro.fade = Some(Fade { target: [5; 96], steps_left: 0x40 });
        assert!(intro.fade_step());
        assert_eq!(intro.dac[0], 2);
        assert!(intro.fade_step());
        assert_eq!(intro.dac[0], 4);
        // 4 + 1 < 5 is false: snap to the target, no change -> over.
        assert!(!intro.fade_step());
        assert_eq!(intro.dac[0], 5);
    }

    /// Load the intro from the user's game files, or `None` when absent.
    pub(crate) fn load_original() -> Option<Intro> {
        let file = soe_res::ResFile::parse(std::fs::read("../../game/RES32/TITLE.RES").ok()?).ok()?;
        let mut images = HashMap::new();
        for id in image_ids() {
            let record = file.records().into_iter().find(|r| r.id == id)?;
            images.insert(id, soe_video::decode_planar(&file.decode(record).ok()?).ok()?);
        }
        let palettes = crate::palette::PaletteSet::load("../../ghidra/EXCAL_UNPACKED.EXE").ok()?;
        Some(Intro::new(images, palettes.table(crate::palette::TITLE_DS)?, [0; 96]))
    }

    /// Replays the frames recorded from the original by
    /// `tests/oracle/intro_vectors.py`: same pictures, same DAC, in the same
    /// order and at the same game-clock offsets.
    #[test]
    fn matches_original_intro() {
        let Ok(bytes) = std::fs::read("../../tests/vectors/intro/frames.bin") else {
            eprintln!("SKIP: tests/vectors/intro/frames.bin absent");
            return;
        };
        let Some(mut intro) = load_original() else {
            eprintln!("SKIP: game files absent");
            return;
        };
        const LEN: usize = 8 + 64000 + 768;
        let want: Vec<(u32, &[u8], &[u8])> = bytes
            .chunks(LEN)
            .map(|c| (u32::from_le_bytes(c[..4].try_into().unwrap()), &c[8..64008], &c[64008..64008 + 96]))
            .collect();
        let entry: Option<serde_json::Value> = std::fs::read_to_string("../../tests/vectors/intro/entry.json")
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok());
        let (clock, cycle_at) = entry.as_ref().map_or((want[0].0, 0), |e| {
            (e["clock"].as_u64().unwrap() as u32, e["cycle_at"].as_u64().unwrap() as u32)
        });
        if let Some(e) = &entry {
            for (d, v) in intro.dac.iter_mut().zip(e["dac"].as_array().unwrap()) {
                *d = v.as_u64().unwrap() as u8;
            }
        }
        intro.set_instant_fades();
        intro.set_clock(clock, cycle_at);
        intro.samples = Some(Vec::new());
        intro.start();
        while !intro.finished() {
            intro.tick();
        }
        let got = intro.samples.take().unwrap();
        // The emulator's clock also runs during drawing (40 000 instructions
        // a tick), so sample times jitter and a cycle step can fall on either
        // side of a draw. Compare the pictures and the DAC states each as
        // their own sequence, and the time since the previous one within 3 ticks (the emulator's
        // flash fade costs it 3 ticks; the replay's fades take none).
        fn runs<T: PartialEq + Clone>(items: impl Iterator<Item = (u32, T)>) -> Vec<(u32, T)> {
            let mut out: Vec<(u32, T)> = Vec::new();
            for (t, v) in items {
                if out.last().is_none_or(|(_, last)| *last != v) {
                    out.push((t, v));
                }
            }
            out
        }
        let (w0, g0) = (want[0].0, got[0].0);
        let want_pics = runs(want.iter().map(|w| (w.0 - w0, w.1.to_vec())));
        let got_pics = runs(got.iter().map(|g| (g.0 - g0, g.1.clone())));
        for (n, (w, g)) in want_pics.iter().zip(&got_pics).enumerate() {
            let diff = w.1.iter().zip(&g.1).filter(|(a, b)| a != b).count();
            assert_eq!(diff, 0, "picture {n}: {diff} pixels differ (tick {} vs {})", w.0, g.0);
            if n > 0 {
                let (dw, dg) = (w.0 - want_pics[n - 1].0, g.0 - got_pics[n - 1].0);
                assert!(dw.abs_diff(dg) <= 3, "picture {n}: {dw} ticks after the last vs {dg}");
            }
        }
        assert_eq!(want_pics.len(), got_pics.len(), "picture count");
        let want_dacs = runs(want.iter().map(|w| (w.0 - w0, w.2.to_vec())));
        let got_dacs = runs(got.iter().map(|g| (g.0 - g0, g.2.to_vec())));
        for (n, (w, g)) in want_dacs.iter().zip(&got_dacs).enumerate() {
            assert_eq!(w.1, g.1, "DAC {n} (tick {} vs {})", w.0, g.0);
            // Cycling keeps its own 16-tick phase; only the flash moves
            // with the emulator's draw time.
            assert!(w.0.abs_diff(g.0) <= 4, "DAC {n}: tick {} vs {}", w.0, g.0);
        }
        assert_eq!(want_dacs.len(), got_dacs.len(), "DAC count");
    }

    #[test]
    fn skip_runs_to_the_fade_out() {
        let mut intro = Intro::new(HashMap::new(), [0; 96], [0; 96]);
        intro.set_instant_fades();
        let events = intro.start();
        assert_eq!(events, [IntroEvent::Music("title")]);
        intro.skip();
        let mut all = Vec::new();
        for _ in 0..4 {
            all.extend(intro.tick());
        }
        assert!(intro.finished());
        assert_eq!(all.last(), Some(&IntroEvent::Finished));
    }
}
