//! Widget animation scripts (the scene engine in segment `18cb`).
//!
//! Every scene widget (room actors, battle units, map entities) runs a
//! word-coded script. `FUN_18cb_066a` takes the widget's script
//! (`+0x0E` indexes the far-pointer table `DS:1E2A`), reads the word at the
//! program counter `+0x27`, and calls handler `word >> 12` from the table at
//! `DS:0389` with `word & 0xFFF`. A handler's return value tells the loop
//! what to do: 1 stops for this frame, bit 1 (2) advances the program
//! counter, 6 ends the frame with a sprite-frame update, 0 runs the next
//! instruction straight away.
//!
//! Widget fields used here: `+0x00` flags (bit 0 mirrored, bit 1 redraw),
//! `+0x0E` script, `+0x24` speed / call depth, `+0x26` sprite frame,
//! `+0x27` program counter, `+0x28..0x2D` return stack, `+0x30` timer.
//!
//! Handlers 14 and 15 are installed at run time: talk (`FUN_26c5_071a`) and
//! combat (`combat::fun_20b5_1dff`). Signalling (5), spawning (12) and
//! resource/depth changes (13) are not ported yet.

#![forbid(unsafe_code)]

pub type Widget = [u8; 0x36];

/// What a handler asks of the outside world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptEvent {
    /// `FUN_1316_181d(n)`: start music.
    Music(u16),
    /// `FUN_1316_183c(n)`: play a sound.
    Sound(u16),
    /// `FUN_1316_1828(n)`.
    Cue(u16),
    /// `FUN_14b0_0bbf`: draw frame `frame` of `image` into the background
    /// at (`x`, `y`) (handler 12, 0x400).
    Stamp { image: u32, x: u16, y: u16, frame: u8, param: u16, mirror: bool },
}

/// Scene state a handler reads besides its own widget.
#[derive(Debug, Clone)]
pub struct ScriptCtx<'a> {
    /// `FUN_15b8_16ca(0x220, 0, widget+0x2E)`: the widget this one follows.
    pub other: Option<Widget>,
    /// The widget's script words (`DS:1E2A[widget+0x0E]`), indexed by `+0x27`.
    pub program: &'a [u16],
    /// `DS:01F5`.
    pub flag_01f5: u8,
    /// `DS:9A76`: the word the conditional compares against.
    pub word_9a76: u16,
    /// `DS:9A78`: bits tested (and bit 7 set) by the conditional.
    pub bits_9a78: u8,
}

pub trait ScriptWorld {
    /// `FUN_15b8_17e0(n)`.
    fn random(&mut self, n: u16) -> u16;
}

fn w(widget: &Widget, at: usize) -> u16 {
    u16::from_le_bytes([widget[at], widget[at + 1]])
}

fn set_w(widget: &mut Widget, at: usize, v: u16) {
    widget[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

fn set_timer(widget: &mut Widget, v: u32) {
    widget[0x30..0x34].copy_from_slice(&v.to_le_bytes());
}

/// Handlers this module implements.
#[must_use]
pub const fn is_ported(handler: u16) -> bool {
    matches!(handler, 0 | 1 | 2 | 3 | 4 | 6 | 7 | 8 | 9 | 10 | 11)
}

/// Run handler `handler` (0..=15) with `arg` (`word & 0xFFF`) on `widget`.
/// `clock` is the 32-bit timer at `DS:7E3E`.
/// Returns AL, or `None` for handlers not ported yet.
pub fn run_handler(
    handler: u16,
    arg: u16,
    widget: &mut Widget,
    clock: u32,
    ctx: &mut ScriptCtx<'_>,
    world: &mut impl ScriptWorld,
    events: &mut Vec<ScriptEvent>,
) -> Option<u8> {
    Some(match handler {
        0 => fun_18cb_08ff(widget, arg, clock),
        1 => fun_18cb_0944(widget, arg, ctx),
        8 | 9 => fun_18cb_0e5f(widget, arg, ctx, world),
        2 => fun_18cb_0b06(widget, arg, clock, world),
        3 => fun_18cb_0bb1(widget, arg),
        4 => fun_18cb_0bdb(widget, arg),
        6 => fun_18cb_0c8a(widget, arg),
        7 => 2,
        10 => {
            events.push(ScriptEvent::Music(arg));
            2
        }
        11 => {
            events.push(ScriptEvent::Sound(arg));
            2
        }
        _ => return None,
    })
}

/// Handler 0: wait `(arg >> 5) & 0x7F` ticks showing frame `arg & 0x1F`
/// (0x1F hides the sprite: frame bit 7).
fn fun_18cb_08ff(widget: &mut Widget, arg: u16, clock: u32) -> u8 {
    set_timer(widget, clock.wrapping_add(u32::from((arg >> 5) & 0x7f)));
    if arg & 0x1f == 0x1f {
        widget[0x26] |= 0x80;
    } else {
        widget[0x26] = (arg & 0x1f) as u8;
    }
    6
}

/// Handler 1: step along the movement set by handler 4 (`+0x24`: mode
/// `& 0xE00`, target `& 0x1FF`). Bit 11 of `arg` moves vertically (bit 10
/// down), otherwise horizontally (forward when bit 10 matches the mirror
/// bit), by `arg & 0x1FF`. On arrival it unwinds the call stack to depth
/// one, returns from the call and clears `+0x24`.
fn fun_18cb_0944(widget: &mut Widget, arg: u16, ctx: &ScriptCtx<'_>) -> u8 {
    let mode = w(widget, 0x24) & 0xe00;
    let lo = (w(widget, 0x24) & 0x1ff) as i16;
    let other = if mode == 0x200 { ctx.other.as_ref() } else { None };
    let step = arg & 0x1ff;
    let x = |wd: &Widget| w(wd, 0x12) as i16;
    let done = if arg & 0x800 != 0 {
        let y = if arg & 0x400 != 0 {
            w(widget, 0x10).wrapping_add(step)
        } else {
            w(widget, 0x10).wrapping_sub(step)
        };
        set_w(widget, 0x10, y);
        if arg & 0x400 != 0 { mode == 0x800 && lo <= y as i16 } else { mode == 0x600 && y as i16 <= lo }
    } else if (arg & 0x400) >> 10 == w(widget, 0) & 1 {
        set_w(widget, 0x12, w(widget, 0x12).wrapping_add(step));
        other.is_some_and(|o| (w(o, 0x1e).wrapping_sub(w(widget, 0x16)) as i16) <= lo.wrapping_sub(100))
            || (mode == 0x400 && lo <= x(widget))
    } else {
        if x(widget) > -200 {
            set_w(widget, 0x12, w(widget, 0x12).wrapping_sub(step));
        }
        other.is_some_and(|o| (w(widget, 0x12).wrapping_sub(w(o, 0x16)) as i16) <= lo.wrapping_sub(100))
            || (mode == 0x400 && x(widget) <= lo)
    };
    set_w(widget, 0, w(widget, 0) | 2);
    if !done {
        return 2;
    }
    while (w(widget, 0x24) & 0x7000) as i16 > 0x1000 {
        fun_18cb_0c8a(widget, 0xc00);
    }
    fun_18cb_0c8a(widget, 0x200);
    set_w(widget, 0x24, 0);
    0
}

/// `FUN_18cb_0ded`: is the horizontal gap between `a` and `b` (edge to
/// edge, on the side given by their centres) less than `limit`?
fn fun_18cb_0ded(a: &Widget, b: &Widget, limit: u16) -> bool {
    let centre = |wd: &Widget| w(wd, 0x12).wrapping_add(w(wd, 0x1a) >> 1) as i16;
    let gap = if centre(a) < centre(b) {
        w(b, 0x12).wrapping_sub(w(a, 0x16))
    } else {
        w(a, 0x12).wrapping_sub(w(b, 0x16))
    };
    (gap as i16) < limit as i16
}

/// Handlers 8 and 9: conditional. When the test holds, run on past any
/// following 9xxx words (alternatives); otherwise skip the following 8xxx
/// words and fall into a 9xxx word if one follows.
fn fun_18cb_0e5f(
    widget: &mut Widget,
    arg: u16,
    ctx: &mut ScriptCtx<'_>,
    world: &mut impl ScriptWorld,
) -> u8 {
    let other = ctx.other.unwrap_or(*widget);
    let me = &*widget;
    let sx = |wd: &Widget, at| w(wd, at) as i16;
    let cond = if arg & 0xf80 == 0 {
        match arg {
            0 => w(me, 0) & 1 == 0,
            1 => w(me, 0) & 1 != 0,
            2 => sx(me, 0x12) < sx(me, 0x24),
            3 => sx(me, 0x12) >= sx(me, 0x24),
            4 => sx(me, 0x16) < sx(&other, 0x1e),
            5 => sx(me, 0x12) >= sx(&other, 0x16),
            6 => ctx.flag_01f5 != 0,
            7 => ctx.flag_01f5 == 0,
            _ => false,
        }
    } else if arg & 0x800 == 0 {
        let v = ctx.word_9a76 & 0x7f;
        let bits = u16::from(ctx.bits_9a78);
        let frame = u16::from(me[0x26]);
        match arg & 0x780 {
            0x100 => world.random(100) < v,
            0x180 => frame < v,
            0x200 => frame > v,
            0x280 => frame == v,
            0x300 => u16::from(other[0x26]) == v,
            0x380 => u16::from(other[0x0e]) == v,
            0x400 => {
                ctx.bits_9a78 |= 0x80;
                bits & v == v
            }
            0x480 => {
                ctx.bits_9a78 |= 0x80;
                v & bits == 0
            }
            _ => false,
        }
    } else {
        let v = ctx.word_9a76 & 0x1ff;
        match arg & 0xe00 {
            0x800 => w(me, 0x12) < v,
            0xa00 => w(me, 0x12) > v,
            0xc00 => fun_18cb_0ded(me, &other, v),
            0xe00 => !fun_18cb_0ded(me, &other, v),
            _ => false,
        }
    };
    let op = |widget: &Widget| ctx.program.get(usize::from(widget[0x27])).copied().unwrap_or(0) & 0xf000;
    if cond {
        loop {
            widget[0x27] = widget[0x27].wrapping_add(1);
            if op(widget) != 0x9000 {
                return 0;
            }
        }
    }
    loop {
        widget[0x27] = widget[0x27].wrapping_add(1);
        if op(widget) != 0x8000 {
            break;
        }
    }
    if op(widget) == 0x9000 { 0 } else { 2 }
}

/// Handler 2: random wait. `arg == 0` waits until the next redraw;
/// otherwise wait `lo + rand(hi - lo)` ticks (`lo = arg >> 6 & 0x1F`,
/// `hi = arg & 0x3F`, both in seconds when bit 11 is set).
fn fun_18cb_0b06(widget: &mut Widget, arg: u16, clock: u32, world: &mut impl ScriptWorld) -> u8 {
    if arg == 0 {
        if w(widget, 0) & 2 != 0 {
            return 1;
        }
        set_timer(widget, 0xffff_ffff);
    } else if arg & 0x7ff != 0 {
        let mut lo = (arg >> 6) & 0x1f;
        let mut hi = arg & 0x3f;
        if arg & 0x800 != 0 {
            lo = lo.wrapping_mul(0x3c);
            hi = hi.wrapping_mul(0x3c);
        }
        if hi <= lo {
            hi = lo.wrapping_add(1);
        }
        let r = world.random(hi.wrapping_sub(lo));
        set_timer(widget, clock.wrapping_add(u32::from(r)).wrapping_add(u32::from(lo)));
    }
    widget[0x27] = widget[0x27].wrapping_add(1);
    1
}

/// Handler 3: mirror the sprite (2 toggles, otherwise set bit 0 to `arg & 1`).
fn fun_18cb_0bb1(widget: &mut Widget, arg: u16) -> u8 {
    let mut flags = w(widget, 0);
    if arg == 2 {
        flags ^= 1;
    } else {
        flags = (flags & 0xfffe) | (arg & 1);
    }
    set_w(widget, 0, flags);
    2
}

/// Handler 4: set the movement word `+0x24` (`0x400 | x` is relative to
/// the widget's `+0x12`).
fn fun_18cb_0bdb(widget: &mut Widget, arg: u16) -> u8 {
    let mut value = arg & 0x1ff;
    let mut mode = arg & 0xe00;
    if mode == 0x400 {
        value = value.wrapping_add(w(widget, 0x12));
    }
    if (value as i16) < 0 {
        value = 0;
    }
    if mode == 0 {
        mode = 0x400;
    }
    set_w(widget, 0x24, mode | value);
    2
}

/// Handler 6: control flow (`CS:0CAC` table by `arg & 0xE00`).
fn fun_18cb_0c8a(widget: &mut Widget, arg: u16) -> u8 {
    let depth = |widget: &mut Widget, delta: i16| {
        let v = w(widget, 0x24);
        set_w(widget, 0x24, v.wrapping_add(delta as u16));
    };
    match arg & 0xe00 {
        0x000 => {
            // Call: push the return address, then jump.
            if w(widget, 0x24) & 0xfff != 0 {
                depth(widget, 0x1000);
            }
            for k in (1..=5).rev() {
                widget[0x28 + k] = widget[0x27 + k];
            }
            widget[0x28] = widget[0x27].wrapping_add(1);
            widget[0x27] = arg as u8;
            0
        }
        0x400 => {
            widget[0x27] = arg as u8;
            0
        }
        0x200 | 0xc00 => {
            if w(widget, 0x24) & 0x7000 != 0 {
                depth(widget, -0x1000);
            }
            let ret = arg & 0xe00 == 0x200;
            if ret {
                widget[0x27] = widget[0x28];
            }
            for k in 0..5 {
                widget[0x28 + k] = widget[0x29 + k];
            }
            widget[0x2d] = 0;
            if ret { 0 } else { 2 }
        }
        0x800 | 0xa00 => {
            widget[0x0e] = ((arg >> 6) & 0x0f) as u8;
            widget[0x27] = (arg & 0x3f) as u8;
            0
        }
        _ => 0,
    }
}

/// The widget list (`DS:969D` count, `DS:969F` table) in drawing order,
/// with the scene globals the interpreter reads and writes.
#[derive(Debug, Clone, Default)]
pub struct ScriptScene {
    pub widgets: Vec<Widget>,
    /// `DS:9A6C`: the widget being run.
    pub current: i16,
    /// `DS:9A76`: the last script word read.
    pub word_9a76: u16,
    /// `DS:9A78`.
    pub bits_9a78: u8,
    /// `DS:01F5`.
    pub flag_01f5: u8,
    /// `DS:9A79`: cleared by handler 5.
    pub word_9a79: u16,
    /// `DS:03C9`: the scene's clip rectangle (top, left, bottom, right).
    pub clip: [i16; 4],
}

/// What the interpreter needs from outside: scripts, sprite-sheet
/// metadata, image loading, the clock and the talk/combat hooks.
pub trait ScriptHost: ScriptWorld {
    /// The 32-bit clock `DS:7E3E`, read live.
    fn clock(&mut self) -> u32;
    /// Script `n` (`DS:1E2A[n]`) as words.
    fn program(&self, script: u8) -> Vec<u16>;
    /// The sheet header of image handle `image` (widget `+0x02`):
    /// byte 3 frame count, 6-byte frame records from byte 4.
    fn sheet(&self, image: u32) -> Vec<u8>;
    /// `FUN_14b0_0301(old)` then `FUN_14b0_0251(res, 0)`: replace a
    /// widget's image, returning the new handle.
    fn load_image(&mut self, old: u32, res: u16) -> u32;
    /// `FUN_14b0_0251(res, variant)`: load a sprite sheet.
    fn load_sprite(&mut self, res: u16, variant: u16) -> u32;
    /// `FUN_14b0_0122(res)`: a whole image's handle, width and height.
    fn static_image(&mut self, res: u16) -> (u32, u16, u16);
    /// A spawned widget shares its parent's image (header byte 0 counts).
    fn retain_image(&mut self, image: u32);
    /// `FUN_15b8_0834` frees a removed widget's image by its flags
    /// (0x10, 0x20 or 0x40 select the release call).
    fn release_image(&mut self, flags: u16, image: u32);
    fn event(&mut self, event: ScriptEvent);
    /// Handlers 14 (talk, `FUN_26c5_071a`) and 15 (combat,
    /// `FUN_20b5_1dff`).
    fn hook(&mut self, handler: u16, arg: u16, scene: &mut ScriptScene) -> u8;
}

/// Arguments of `FUN_15b8_0461`. `width` doubles as the first frame when
/// a sprite sheet is used (flag 0x40).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpawnArgs {
    pub flags: u16,
    /// Resource to load (0: use `image`).
    pub res: u16,
    pub image: u32,
    /// `+0x06`/`+0x08`: the widget's callback (offset, segment).
    pub callback: (u16, u16),
    pub id: u16,
    /// `+0x0C`; also the variant passed to the sprite loader.
    pub variant: u16,
    pub script: u8,
    pub depth: u8,
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

fn image(widget: &Widget) -> u32 {
    u32::from_le_bytes([widget[2], widget[3], widget[4], widget[5]])
}

fn rect(widget: &Widget, at: usize) -> [i16; 4] {
    [0, 2, 4, 6].map(|k| w(widget, at + k) as i16)
}

fn set_rect(widget: &mut Widget, at: usize, r: [i16; 4]) {
    for (k, v) in r.into_iter().enumerate() {
        set_w(widget, at + 2 * k, v as u16);
    }
}

/// `FUN_18cb_16f0`: the union of two rects, ignoring one that is empty or
/// off the original's 320x200 screen.
pub(crate) fn fun_18cb_16f0(a: [i16; 4], b: [i16; 4]) -> [i16; 4] {
    let valid = |r: [i16; 4]| {
        r[0] <= 200 && r[1] <= 320 && r[2] >= 0 && r[3] >= 0 && r[2] > r[0] && r[3] > r[1]
    };
    if !valid(b) {
        return a;
    }
    if !valid(a) {
        return b;
    }
    [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]
}

/// `FUN_30ec_0006`: intersect `a` with `b`; true when the result is not
/// empty.
pub(crate) fn fun_30ec_0006(a: [i16; 4], b: [i16; 4]) -> ([i16; 4], bool) {
    let r = [a[0].max(b[0]), a[1].max(b[1]), a[2].min(b[2]), a[3].min(b[3])];
    (r, r[0] < r[2] && r[1] < r[3])
}

impl ScriptScene {
    fn cur(&self) -> usize {
        self.current as usize
    }

    /// `FUN_15b8_162e(mask, depth, id)`: the first widget with id `+0x0A`,
    /// depth `+0x0F` (0 = any) and a flag in `mask` (0 = any).
    #[must_use]
    pub fn find(&self, mask: u16, depth: u8, id: u16) -> Option<usize> {
        self.widgets.iter().position(|wd| {
            w(wd, 0x0a) == id && (depth == 0 || wd[0x0f] == depth) && (mask == 0 || w(wd, 0) & mask != 0)
        })
    }

    /// `FUN_15b8_0461`: create a widget and insert it after every widget
    /// deeper than it (`+0x0F`, larger first). Returns its index.
    pub fn fun_15b8_0461(&mut self, a: &SpawnArgs, host: &mut dyn ScriptHost) -> usize {
        let mut image_handle = a.image;
        let (mut x, mut y, mut depth) = (a.x, a.y, a.depth);
        let (mut width, mut height) = (a.width, a.height);
        let mut frame = 0u8;
        if a.flags & 0x20 != 0 && a.res != 0 {
            // FUN_14b0_0122: a whole image; header +0x0A width, +0x08 height.
            let (handle, w_, h_) = host.static_image(a.res);
            image_handle = handle;
            width = w_;
            height = h_;
            if a.flags & 0x100 != 0 && depth != 0xd3 {
                let span = if a.flags & 0x2000 != 0 && width > 0x30 { width - 0x30 } else { width };
                x = x.wrapping_sub(span >> 1);
                y = y.wrapping_sub(height >> 1);
            }
            if u16::from(depth) == y {
                depth = depth.wrapping_add(height as u8);
            }
        }
        if a.flags & 0x40 != 0 {
            if a.res != 0 {
                image_handle = host.load_sprite(a.res, a.variant);
            }
            if image_handle != 0 {
                frame = width as u8;
                let sheet = host.sheet(image_handle);
                let at = usize::from(frame) * 6;
                height = u16::from(sheet.get(at + 9).copied().unwrap_or(0));
                width = u16::from(sheet.get(at + 8).copied().unwrap_or(0));
                if a.res != 0 {
                    y = y.wrapping_sub(height);
                }
            }
        }
        let mut nw: Widget = [0; 0x36];
        set_w(&mut nw, 0, a.flags);
        nw[2..6].copy_from_slice(&image_handle.to_le_bytes());
        set_w(&mut nw, 0x06, a.callback.0);
        set_w(&mut nw, 0x08, a.callback.1);
        set_w(&mut nw, 0x0a, a.id);
        set_w(&mut nw, 0x0c, a.variant);
        // FUN_335c_000f: toupper.
        nw[0x0e] = a.script.to_ascii_uppercase();
        nw[0x0f] = depth;
        let r = [y, x, y.wrapping_add(height), x.wrapping_add(width)].map(|v| v as i16);
        set_rect(&mut nw, 0x10, r);
        set_w(&mut nw, 0x18, height);
        set_w(&mut nw, 0x1a, width);
        set_rect(&mut nw, 0x1c, r);
        nw[0x26] = frame;
        if a.flags & 0x2000 != 0 && width > 0x30 {
            set_w(&mut nw, 0x1a, width - 0x30);
        }
        let mut at = self.widgets.len();
        if at == 0 {
            self.widgets.push(nw);
            return 0;
        }
        while at > 0 && self.widgets[at - 1][0x0f] <= depth {
            at -= 1;
        }
        self.widgets.insert(at, nw);
        if at as i16 <= self.current {
            self.current += 1;
        }
        if self.widgets.len() as i16 <= self.current {
            self.current = self.widgets.len() as i16 - 1;
        }
        at
    }

    /// Handler 5: show (1) or hide (0) the partner widget.
    fn fun_18cb_0c2d(&mut self, arg: u16) -> u8 {
        let cur = self.cur();
        if let Some(o) = self.find(0x220, 0, w(&self.widgets[cur], 0x2e)) {
            let other = &mut self.widgets[o];
            if arg == 0 {
                set_w(other, 0x12, 0x4e1f);
            } else if arg == 1 {
                set_w(other, 0x12, w(other, 0x1e));
            }
            set_w(other, 0, w(other, 0) | 2);
            self.word_9a79 = 0;
        }
        2
    }

    /// Handler 12: spawn a copy (0x000), end (0x100), end matching copies
    /// (0x200), hide (0x300) or stamp onto the background (0x400).
    fn fun_18cb_1201(&mut self, arg: u16, host: &mut dyn ScriptHost) -> u8 {
        let cur = self.cur();
        let me = self.widgets[cur];
        let kill = |wd: &mut Widget| {
            wd[0x27] = 0;
            wd[0x0e] = 0x17;
            set_timer(wd, 0);
        };
        match arg & 0xf00 {
            0x000 => {
                let args = SpawnArgs {
                    flags: w(&me, 0),
                    image: image(&me),
                    callback: (0, 0),
                    id: w(&me, 0x0a) | 0x8000,
                    variant: w(&me, 0x0c),
                    script: me[0x0e],
                    depth: me[0x0f],
                    x: w(&me, 0x12),
                    y: w(&me, 0x10),
                    width: u16::from(me[0x26]),
                    ..SpawnArgs::default()
                };
                let at = self.fun_15b8_0461(&args, host);
                host.retain_image(image(&me));
                self.widgets[at][0x27] = arg as u8;
                set_w(&mut self.widgets[at], 0x2e, w(&me, 0x2e));
            }
            0x100 => {
                kill(&mut self.widgets[cur]);
                return 0;
            }
            0x200 => {
                for (i, wd) in self.widgets.iter_mut().enumerate() {
                    if i != cur
                        && me[0x0f] == wd[0x0f]
                        && w(&me, 0x0a) == w(wd, 0x0a) & 0x7fff
                        && image(&me) == image(wd)
                    {
                        kill(wd);
                    }
                }
                host.event(ScriptEvent::Cue(9));
            }
            0x300 => set_w(&mut self.widgets[cur], 0x12, 0x4e1f),
            0x400 => host.event(ScriptEvent::Stamp {
                image: image(&me),
                x: w(&me, 0x12),
                y: w(&me, 0x10),
                frame: arg as u8,
                param: w(&me, 0x0c),
                mirror: w(&me, 0) & 1 != 0,
            }),
            _ => {}
        }
        2
    }

    /// Handler 13: new image (0x000), id `+0x0A` (0x400), `+0x0C` (0x800)
    /// or depth `+0x0F` (0xC00, moving the widget in the drawing order).
    fn fun_18cb_13c4(&mut self, arg: u16, host: &mut dyn ScriptHost) -> u8 {
        let cur = self.cur();
        match arg & 0xc00 {
            0x000 => {
                let img = host.load_image(image(&self.widgets[cur]), arg & 0x3ff);
                let sheet = host.sheet(img);
                let wd = &mut self.widgets[cur];
                wd[2..6].copy_from_slice(&img.to_le_bytes());
                let h = u16::from(sheet.get(9).copied().unwrap_or(0));
                let wi = u16::from(sheet.get(8).copied().unwrap_or(0));
                set_w(wd, 0x18, h);
                set_w(wd, 0x1a, wi);
                set_w(wd, 0x14, w(wd, 0x10).wrapping_add(h));
                set_w(wd, 0x16, w(wd, 0x12).wrapping_add(wi));
                wd[0x26] = 0;
            }
            0x400 => set_w(&mut self.widgets[cur], 0x0a, arg & 0x3ff),
            0x800 => set_w(&mut self.widgets[cur], 0x0c, arg & 0x3ff),
            _ => {
                let depth = arg as u8;
                let mut at = 0;
                while at < self.widgets.len() && self.widgets[at][0x0f] > depth {
                    at += 1;
                }
                self.widgets[cur][0x0f] = depth;
                if at == cur || at == cur + 1 {
                    return 2;
                }
                let moved = self.widgets.remove(cur);
                let to = if at < cur { at } else { at - 1 };
                self.widgets.insert(to, moved);
                self.current = to as i16;
            }
        }
        2
    }

    fn dispatch(&mut self, handler: u16, arg: u16, clock: u32, host: &mut dyn ScriptHost) -> u8 {
        match handler {
            5 => self.fun_18cb_0c2d(arg),
            12 => self.fun_18cb_1201(arg, host),
            13 => self.fun_18cb_13c4(arg, host),
            14 | 15 => host.hook(handler, arg, self),
            _ => {
                let cur = self.cur();
                let program = host.program(self.widgets[cur][0x0e]);
                let other = self.find(0x220, 0, w(&self.widgets[cur], 0x2e)).map(|o| self.widgets[o]);
                let mut ctx = ScriptCtx {
                    other,
                    program: &program,
                    flag_01f5: self.flag_01f5,
                    word_9a76: self.word_9a76,
                    bits_9a78: self.bits_9a78,
                };
                let mut events = Vec::new();
                let ret = run_handler(
                    handler,
                    arg,
                    &mut self.widgets[cur],
                    clock,
                    &mut ctx,
                    &mut HostWorld(host),
                    &mut events,
                )
                .unwrap_or(2);
                self.bits_9a78 = ctx.bits_9a78;
                for e in events {
                    host.event(e);
                }
                ret
            }
        }
    }

    /// `FUN_18cb_066a`: run the current widget's script for up to 60 clock
    /// ticks, until a handler waits (1) or shows a frame (6); then move the
    /// widget to the new frame's anchor and mark it for redraw if its
    /// changed area is on screen.
    pub fn fun_18cb_066a(&mut self, host: &mut dyn ScriptHost) {
        let deadline = host.clock().wrapping_add(0x3c);
        let mut cur = self.cur();
        {
            let wd = &mut self.widgets[cur];
            let old = rect(wd, 0x10);
            set_rect(wd, 0x1c, old);
        }
        let me = self.widgets[cur];
        let sheet = host.sheet(image(&me));
        let rec = 4 + usize::from(me[0x26] & 0x7f) * 6;
        let b = |k: usize| u16::from(sheet.get(rec + k).copied().unwrap_or(0));
        let anchor_x = if w(&me, 0) & 1 != 0 { b(4).wrapping_sub(b(2)) } else { b(2) };
        let anchor_y = b(3) & 0x7f;
        loop {
            // Handlers 0 and 2 read the clock again; a tick between the
            // two reads is not modelled.
            let clock = host.clock();
            if clock > deadline {
                return;
            }
            let program = host.program(self.widgets[cur][0x0e]);
            let word = program.get(usize::from(self.widgets[cur][0x27])).copied().unwrap_or(0);
            self.word_9a76 = word;
            let ret = self.dispatch(word >> 12, word & 0xfff, clock, host);
            if self.widgets.len() as i16 <= self.current {
                self.current = self.widgets.len() as i16 - 1;
            }
            cur = self.cur();
            if ret == 1 {
                return;
            }
            if ret & 2 != 0 {
                self.widgets[cur][0x27] = self.widgets[cur][0x27].wrapping_add(1);
            }
            if ret == 6 {
                break;
            }
        }
        let sheet = host.sheet(image(&self.widgets[cur]));
        let wd = &mut self.widgets[cur];
        if usize::from(wd[0x26]) < usize::from(sheet.get(3).copied().unwrap_or(0)) {
            let rec = 4 + usize::from(wd[0x26]) * 6;
            let b = |k: usize| u16::from(sheet.get(rec + k).copied().unwrap_or(0));
            let dx = if w(wd, 0) & 1 != 0 {
                b(2).wrapping_add(anchor_x).wrapping_sub(b(4))
            } else {
                anchor_x.wrapping_sub(b(2))
            };
            set_w(wd, 0x12, w(wd, 0x12).wrapping_add(dx));
            set_w(wd, 0x10, w(wd, 0x10).wrapping_add(anchor_y.wrapping_sub(b(3) & 0x7f)));
            set_w(wd, 0x18, b(5));
            set_w(wd, 0x14, b(5).wrapping_add(w(wd, 0x10)));
            set_w(wd, 0x1a, b(4));
            set_w(wd, 0x16, b(4).wrapping_add(w(wd, 0x12)));
            let u = fun_18cb_16f0(rect(wd, 0x1c), rect(wd, 0x10));
            set_rect(wd, 0x1c, u);
        }
        let (clipped, visible) = fun_30ec_0006(rect(wd, 0x1c), self.clip);
        set_rect(wd, 0x1c, clipped);
        if visible {
            set_w(wd, 0, w(wd, 0) | 2);
        }
    }

    /// `FUN_18cb_05b0`: run a follower widget (flags 0x220) to its next
    /// frame (6) or wait (1), then clip its old rect and mark it for redraw.
    pub fn fun_18cb_05b0(&mut self, host: &mut dyn ScriptHost) {
        loop {
            let clock = host.clock();
            let cur = self.cur();
            let program = host.program(self.widgets[cur][0x0e]);
            let word = program.get(usize::from(self.widgets[cur][0x27])).copied().unwrap_or(0);
            self.word_9a76 = word;
            let ret = self.dispatch(word >> 12, word & 0xfff, clock, host);
            let cur = self.cur();
            if ret == 1 {
                return;
            }
            if ret & 2 != 0 {
                self.widgets[cur][0x27] = self.widgets[cur][0x27].wrapping_add(1);
            }
            if ret == 6 {
                break;
            }
        }
        let cur = self.cur();
        let wd = &mut self.widgets[cur];
        let (clipped, _) = fun_30ec_0006(rect(wd, 0x1c), self.clip);
        set_rect(wd, 0x1c, clipped);
        set_w(wd, 0, w(wd, 0) | 2);
    }

    /// `FUN_15b8_0978(lo, hi)`: remove every widget whose depth `+0x0F` is
    /// in `lo..=hi`.
    pub fn fun_15b8_0978(&mut self, lo: u8, hi: u8, host: &mut dyn ScriptHost) {
        let mut i = 0;
        while i < self.widgets.len() {
            let depth = self.widgets[i][0x0f];
            if depth < lo || depth > hi {
                i += 1;
            } else {
                self.fun_15b8_0834(i, host);
            }
        }
    }

    /// `FUN_15b8_0834`: remove widget `index`, releasing its image, and
    /// keep `current` pointing at a valid widget.
    fn fun_15b8_0834(&mut self, index: usize, host: &mut dyn ScriptHost) {
        let wd = self.widgets.remove(index);
        host.release_image(w(&wd, 0), image(&wd));
        if index as i16 + 1 <= self.current {
            self.current -= 1;
        }
        if self.widgets.len() as i16 <= self.current {
            self.current = self.widgets.len() as i16 - 1;
        }
    }

    /// `FUN_18cb_0002`: one scene frame. Runs every widget but the first
    /// whose timer is due (last to first), then collects its changed area
    /// together with any overlapping widgets that also need redrawing.
    /// Returns those areas, clipped to the scene, in the order the
    /// original redraws them (all widgets meeting an area are drawn back
    /// to front by the presenter). Finally removes widgets parked at
    /// x >= 9999 that need no redraw.
    pub fn fun_18cb_0002(&mut self, host: &mut dyn ScriptHost) -> Vec<[i16; 4]> {
        let mut drawn = Vec::new();
        let align = |r: &mut [i16; 4]| {
            r[1] &= !7;
            r[3] = r[3].wrapping_add(8) & !7;
        };
        self.current = self.widgets.len() as i16 - 1;
        while self.current > 0 {
            let cur = self.cur();
            let timer = u32::from_le_bytes([
                self.widgets[cur][0x30],
                self.widgets[cur][0x31],
                self.widgets[cur][0x32],
                self.widgets[cur][0x33],
            ]);
            if timer <= host.clock() {
                let flags = w(&self.widgets[cur], 0);
                if flags & 0x240 == 0x240 {
                    self.fun_18cb_066a(host);
                } else if flags & 0x220 == 0x220 {
                    self.fun_18cb_05b0(host);
                }
                let cur = self.cur();
                let wd = &mut self.widgets[cur];
                if w(wd, 0) & 2 != 0 {
                    set_w(wd, 0, (w(wd, 0) ^ 2) | 4);
                    let mut dirty = rect(wd, 0x1c);
                    align(&mut dirty);
                    'merge: loop {
                        for o in &mut self.widgets {
                            let f = w(o, 0);
                            if f & 0xc04 != 0 || f == 0 || !fun_30ec_0006(rect(o, 0x1c), dirty).1 {
                                continue;
                            }
                            set_w(o, 0, f | 4);
                            if f & 2 != 0 {
                                dirty = fun_18cb_16f0(rect(o, 0x1c), dirty);
                                align(&mut dirty);
                                set_w(o, 0, (f | 4) ^ 2);
                                continue 'merge;
                            }
                        }
                        break;
                    }
                    let (clipped, visible) = fun_30ec_0006(dirty, self.clip);
                    if visible {
                        for o in self.widgets.iter_mut().rev() {
                            if w(o, 0) & 4 != 0 {
                                set_w(o, 0, w(o, 0) ^ 4);
                            }
                        }
                        drawn.push(clipped);
                    }
                }
            }
            self.current -= 1;
        }
        self.current = 1;
        while (self.widgets.len() as i16) > self.current {
            let cur = self.cur();
            let wd = &self.widgets[cur];
            if w(wd, 0x12) as i16 >= 0x270f && w(wd, 0) & 2 == 0 {
                self.fun_15b8_0834(cur, host);
            }
            self.current += 1;
        }
        drawn
    }

    /// The character half of `FUN_2d65_13e2` (2d65:1767-1b5e): spawn every
    /// character present as a script widget, foes (battle list
    /// `DS:9DC3 + 0x14`) first, then the party (`DS:9DC3`). `ds` is DGROUP;
    /// `room_base` is what the room builder `FUN_2b79_0003` returned (AL)
    /// and `mode` the scene bits `DS:9DAB & 0xC0` taken on entry (0x80 swaps
    /// the sides' placement, 0x40 moves the party right).
    pub fn fun_2d65_13e2_characters(&mut self, ds: &[u8], room_base: u8, mode: u8, host: &mut dyn ScriptHost) {
        let b = |at: usize| ds[at];
        let wd = |at: usize| u16::from_le_bytes([ds[at], ds[at + 1]]);
        let base = room_base.wrapping_add(b(0x79f7 + usize::from(b(0x9dab))));
        let rec = |c: u8| 0x2c5e + usize::from(c) * 0x38;
        let campaign = b(0x5e37) != 0;
        let mut foes = 0u16;
        for side in [0x14usize, 0] {
            let mut i = side;
            loop {
                let c = b(0x9dc3 + i);
                if c == 0 {
                    break;
                }
                let r = rec(c);
                if wd(r + 0x16) & 0x8000 == 0 {
                    let slot = if side == 0 { i } else { usize::from(foes) };
                    let kind = usize::from(b(r + 0x26));
                    let y = u16::from(base)
                        .wrapping_add(u16::from(b(0x7a2e + slot)))
                        .wrapping_add(wd(0x7a66 + kind * 2));
                    let random_x = |host: &mut dyn ScriptHost| {
                        host.random(foes.wrapping_mul(10)).wrapping_add(wd(0x7a38 + kind * 2))
                    };
                    let x = match (side, mode) {
                        (0x14, 0x80) => wd(0x7a1a + slot * 2),
                        (0x14, _) => random_x(host),
                        (_, 0x80) => random_x(host),
                        (_, 0x40) => wd(0x7a1a + slot * 2).wrapping_add(0x96),
                        _ => wd(0x7a1a + slot * 2),
                    };
                    let mirrored = (side == 0x14) != (mode == 0x80);
                    let args = SpawnArgs {
                        flags: u16::from(mirrored) | 0x240,
                        res: wd(r + 0x22),
                        callback: (9, 0x2a6e),
                        id: u16::from(c),
                        variant: u16::from(b(r + 0x27)),
                        script: kind as u8,
                        depth: (y as u8).wrapping_add(0xfd),
                        x,
                        y,
                        ..SpawnArgs::default()
                    };
                    let at = self.fun_15b8_0461(&args, host);
                    let first = if side == 0 { i == 0 } else { foes == 0 };
                    let lead = if campaign { c == b(0x9c4e) || c == b(0x9c4f) } else { first };
                    let nw = &mut self.widgets[at];
                    nw[0x27] = if lead { 3 } else { 4 };
                    set_w(nw, 0x34, i as u16);
                    set_timer(nw, 0);
                    if side == 0x14 {
                        foes += 1;
                    }
                }
                i += 1;
            }
        }
    }
}

struct HostWorld<'a>(&'a mut dyn ScriptHost);

impl ScriptWorld for HostWorld<'_> {
    fn random(&mut self, n: u16) -> u16 {
        self.0.random(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Rng(Vec<u16>);

    impl ScriptWorld for Rng {
        fn random(&mut self, _n: u16) -> u16 {
            assert!(!self.0.is_empty(), "port drew a random number the original did not");
            self.0.remove(0)
        }
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// Replays handler calls recorded in the original
    /// (`tests/oracle/script_vectors.py`).
    #[test]
    fn matches_original_script_handlers() {
        let Ok(text) = std::fs::read_to_string("../../tests/vectors/script_ops.json") else {
            eprintln!("SKIP: tests/vectors/script_ops.json absent");
            return;
        };
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let mut done = [0usize; 16];
        let mut skipped = [0usize; 16];
        for (n, call) in v["calls"].as_array().unwrap().iter().enumerate() {
            let handler = call["handler"].as_u64().unwrap() as u16;
            if !is_ported(handler) {
                skipped[usize::from(handler)] += 1;
                continue;
            }
            let mut widget: Widget = hex(call["before"].as_str().unwrap()).try_into().unwrap();
            let mut world = Rng(
                call["rng"].as_array().unwrap().iter().map(|r| r.as_u64().unwrap() as u16).collect(),
            );
            let mut events = Vec::new();
            let arg = call["arg"].as_u64().unwrap() as u16;
            let clock = call["clock"].as_u64().unwrap() as u32;
            let program: Vec<u16> = hex(call["program"].as_str().unwrap_or(""))
                .chunks(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            let before = hex(call["flags_before"].as_str().unwrap_or("0000"));
            let mut ctx = ScriptCtx {
                other: call["other"].as_str().map(|o| hex(o).try_into().unwrap()),
                program: &program,
                flag_01f5: before[0],
                word_9a76: call["word"].as_u64().unwrap() as u16,
                bits_9a78: before[1],
            };
            let ret = run_handler(handler, arg, &mut widget, clock, &mut ctx, &mut world, &mut events).unwrap();
            if let Some(after) = call["flags_after"].as_str() {
                assert_eq!(ctx.bits_9a78, hex(after)[1], "call {n}: DS:9A78");
            }
            let ctx = format!("call {n}: handler {handler} arg {arg:#05x}");
            assert_eq!(u64::from(ret), call["ret"].as_u64().unwrap(), "{ctx}: return");
            assert_eq!(
                widget.as_slice(),
                hex(call["after"].as_str().unwrap()).as_slice(),
                "{ctx}: widget"
            );
            assert!(world.0.is_empty(), "{ctx}: unused random numbers");
            done[usize::from(handler)] += 1;
        }
        eprintln!("script handlers matched: {done:?}; not ported yet: {skipped:?}");
        assert!(done.iter().sum::<usize>() > 0);
    }

    struct Replay {
        clocks: Vec<u32>,
        rng: Vec<u16>,
        loads: Vec<u32>,
        programs: std::collections::HashMap<u8, Vec<u16>>,
        sheets: std::collections::HashMap<u32, Vec<u8>>,
        sounds: Vec<(String, u16)>,
        combat: Vec<serde_json::Value>,
        combat_calls: usize,
    }

    struct CombatReplay<'a> {
        rng: &'a mut Vec<u16>,
        rout: Vec<bool>,
    }

    impl crate::combat::CombatWorld for CombatReplay<'_> {
        fn random(&mut self, _n: u16) -> u16 {
            assert!(!self.rng.is_empty(), "combat drew a random number the original did not");
            self.rng.remove(0)
        }
        fn rout_check(&mut self) -> bool {
            self.rout.remove(0)
        }
        fn location_of(&self, ds: &[u8], id: u8) -> u8 {
            crate::fight::fun_2d65_0a2a(ds, &[], id)
        }
    }

    impl ScriptWorld for Replay {
        fn random(&mut self, _n: u16) -> u16 {
            assert!(!self.rng.is_empty(), "port drew a random number the original did not");
            self.rng.remove(0)
        }
    }

    impl ScriptHost for Replay {
        fn clock(&mut self) -> u32 {
            assert!(!self.clocks.is_empty(), "port checked the clock more often than the original");
            self.clocks.remove(0)
        }
        fn program(&self, script: u8) -> Vec<u16> {
            self.programs.get(&script).cloned().unwrap_or_default()
        }
        fn sheet(&self, image: u32) -> Vec<u8> {
            self.sheets.get(&image).cloned().unwrap_or_default()
        }
        fn load_image(&mut self, _old: u32, _res: u16) -> u32 {
            self.loads.remove(0)
        }
        fn retain_image(&mut self, _image: u32) {}
        fn load_sprite(&mut self, _res: u16, _variant: u16) -> u32 {
            self.loads.remove(0)
        }
        fn static_image(&mut self, _res: u16) -> (u32, u16, u16) {
            unreachable!("not recorded")
        }
        fn release_image(&mut self, _flags: u16, _image: u32) {}
        fn event(&mut self, event: ScriptEvent) {
            match event {
                ScriptEvent::Music(n) => self.sounds.push(("music".into(), n)),
                ScriptEvent::Sound(n) => self.sounds.push(("sound".into(), n)),
                ScriptEvent::Cue(n) => self.sounds.push(("cue".into(), n)),
                ScriptEvent::Stamp { .. } => {}
            }
        }
        fn hook(&mut self, handler: u16, arg: u16, scene: &mut ScriptScene) -> u8 {
            assert_eq!(handler, 15, "steps that reach the talk hook are skipped");
            let rec = self.combat.remove(0);
            let mut ds = hex(rec["ds"].as_str().unwrap());
            let before = ds.clone();
            let mut world = CombatReplay {
                rng: &mut self.rng,
                rout: rec["rout"].as_array().unwrap().iter().map(|r| r.as_u64().unwrap() != 0).collect(),
            };
            let stale = rec["stale"].as_u64().unwrap() as u16;
            let (ret, events) = crate::combat::script_hook(scene, arg, &mut ds, stale, &mut world);
            for e in events {
                if let crate::combat::CombatEvent::Sound(n) = e {
                    self.sounds.push(("sound".into(), n));
                }
            }
            // Compare only the state the combat step owns (the regions
            // battle_vectors.py records): routines it delegates as events
            // (messages, unit removal, UI) write elsewhere.
            let owned = |i: usize| {
                (0x2c5e..0x2c5e + 0x5e * 0x38).contains(&i)
                    || [(0x01f2, 1), (0x032d, 1), (0x5e37, 1), (0x5e41, 4), (0x7d3e, 4), (0x9a6c, 2),
                        (0x9c4e, 0x0a), (0x9c56, 2), (0x9c82, 0x28), (0x9dac, 2), (0x9dae, 3), (0x9dc3, 0x32)]
                        .iter()
                        .any(|&(a, n)| (a..a + n).contains(&i))
            };
            let changes: Vec<(u64, u64)> = (0..ds.len())
                .filter(|&i| ds[i] != before[i] && owned(i))
                .map(|i| (i as u64, u64::from(ds[i])))
                .collect();
            let want: Vec<(u64, u64)> = rec["changes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| (c[0].as_u64().unwrap(), c[1].as_u64().unwrap()))
                .filter(|(i, _)| owned(*i as usize))
                .collect();
            assert_eq!(changes, want, "DGROUP changes of combat call {}", self.combat_calls);
            self.combat_calls += 1;
            ret
        }
    }

    /// Replays whole `FUN_18cb_066a` steps recorded in the original
    /// (`tests/oracle/scene_vectors.py`).
    #[test]
    fn matches_original_scene_steps() {
        replay_scene("scene_steps.json", false);
    }

    /// Replays whole scene frames (`FUN_18cb_0002`) recorded in the
    /// original (`tests/oracle/scene_vectors.py --frames`).
    #[test]
    fn matches_original_scene_frames() {
        replay_scene("scene_frames.json", true);
    }

    fn replay_scene(file: &str, frames: bool) {
        let Ok(text) = std::fs::read_to_string(format!("../../tests/vectors/{file}")) else {
            eprintln!("SKIP: tests/vectors/{file} absent");
            return;
        };
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let (mut done, mut hooked, mut combat) = (0, 0, 0);
        for run in v["runs"].as_array().unwrap() {
            let programs: std::collections::HashMap<u8, Vec<u16>> = run["programs"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, p)| {
                    let b = hex(p.as_str().unwrap());
                    (k.parse().unwrap(), b.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect())
                })
                .collect();
            let sheets: std::collections::HashMap<u32, Vec<u8>> = run["sheets"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, s)| (k.parse().unwrap(), hex(s.as_str().unwrap())))
                .collect();
            let widgets = |list: &serde_json::Value| -> Vec<Widget> {
                list.as_array().unwrap().iter().map(|w| hex(w.as_str().unwrap()).try_into().unwrap()).collect()
            };
            let nums = |x: &serde_json::Value| -> Vec<u64> {
                x.as_array().unwrap().iter().map(|n| n.as_u64().unwrap()).collect()
            };
            for (n, call) in run["calls"].as_array().unwrap().iter().enumerate() {
                if call["hooked"].as_bool().unwrap() {
                    hooked += 1;
                    continue;
                }
                let g = &call["globals"];
                let clip = nums(&g["clip"]);
                let mut scene = ScriptScene {
                    widgets: widgets(&call["before"]),
                    current: g["current"].as_u64().unwrap() as i16,
                    word_9a76: g["word_9a76"].as_u64().unwrap() as u16,
                    bits_9a78: g["bits_9a78"].as_u64().unwrap() as u8,
                    flag_01f5: g["flag_01f5"].as_u64().unwrap() as u8,
                    word_9a79: g["word_9a79"].as_u64().unwrap() as u16,
                    clip: [0, 1, 2, 3].map(|k| clip[k] as u16 as i16),
                };
                let mut clocks = if frames { Vec::new() } else { vec![call["clock0"].as_u64().unwrap() as u32] };
                clocks.extend(nums(&call["clocks"]).into_iter().map(|c| c as u32));
                let mut host = Replay {
                    clocks,
                    rng: nums(&call["rng"]).into_iter().map(|r| r as u16).collect(),
                    loads: nums(&call["loads"]).into_iter().map(|r| r as u32).collect(),
                    programs: programs.clone(),
                    sheets: sheets.clone(),
                    sounds: Vec::new(),
                    combat: call["combat"].as_array().cloned().unwrap_or_default(),
                    combat_calls: 0,
                };
                if frames {
                    scene.fun_18cb_0002(&mut host);
                } else {
                    scene.fun_18cb_066a(&mut host);
                }
                let ctx = format!("{} step {n}", run["snap"]);
                let after = widgets(&call["after"]);
                assert_eq!(scene.widgets.len(), after.len(), "{ctx}: widget count");
                for (i, (a, b)) in scene.widgets.iter().zip(&after).enumerate() {
                    assert_eq!(a.as_slice(), b.as_slice(), "{ctx}: widget {i}");
                }
                let ga = &call["globals_after"];
                assert_eq!(scene.current as u16 as u64, ga["current"].as_u64().unwrap(), "{ctx}: current");
                assert_eq!(u64::from(scene.word_9a76), ga["word_9a76"].as_u64().unwrap(), "{ctx}: 9A76");
                assert_eq!(u64::from(scene.bits_9a78), ga["bits_9a78"].as_u64().unwrap(), "{ctx}: 9A78");
                assert_eq!(u64::from(scene.word_9a79), ga["word_9a79"].as_u64().unwrap(), "{ctx}: 9A79");
                let sounds: Vec<(String, u16)> = call["sounds"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|s| (s[0].as_str().unwrap().to_string(), s[1].as_u64().unwrap() as u16))
                    .collect();
                assert_eq!(host.sounds, sounds, "{ctx}: sounds");
                assert!(host.clocks.is_empty() && host.rng.is_empty(), "{ctx}: unused clock/rng");
                assert!(host.combat.is_empty(), "{ctx}: combat calls not made");
                combat += host.combat_calls;
                done += 1;
            }
        }
        eprintln!("{file}: matched {done} (with {combat} combat calls); skipped (talk hook): {hooked}");
        assert!(done > 0);
    }

    /// Replays the character spawns of `FUN_2d65_13e2` recorded as the
    /// duel starts (`tests/oracle/duel_spawn_vectors.py`).
    #[test]
    fn matches_original_duel_spawns() {
        let Ok(text) = std::fs::read_to_string("../../tests/vectors/duel_spawn.json") else {
            eprintln!("SKIP: tests/vectors/duel_spawn.json absent");
            return;
        };
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let mut done = 0;
        for run in v["runs"].as_array().unwrap() {
            let ds = hex(run["ds"].as_str().unwrap());
            // The callback segment (+0x08) was recorded at run time; the
            // port keeps Ghidra's (load segment 0x214 vs 0x1000).
            let spawned: Vec<Widget> = run["spawned"]
                .as_array()
                .unwrap()
                .iter()
                .map(|w| {
                    let mut wd: Widget = hex(w.as_str().unwrap()).try_into().unwrap();
                    let seg = u16::from_le_bytes([wd[8], wd[9]]);
                    if seg != 0 {
                        wd[8..10].copy_from_slice(&(seg - 0x214 + 0x1000).to_le_bytes());
                    }
                    wd
                })
                .collect();
            let mut host = Replay {
                clocks: Vec::new(),
                rng: run["rng"].as_array().unwrap().iter().map(|r| r.as_u64().unwrap() as u16).collect(),
                loads: spawned.iter().map(image).collect(),
                programs: std::collections::HashMap::new(),
                sheets: run["sheets"]
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(k, s)| (k.parse().unwrap(), hex(s.as_str().unwrap())))
                    .collect(),
                sounds: Vec::new(),
                combat: Vec::new(),
                combat_calls: 0,
            };
            let mut scene = ScriptScene::default();
            scene.fun_2d65_13e2_characters(
                &ds,
                run["al"].as_u64().unwrap() as u8,
                run["mode"].as_u64().unwrap() as u8,
                &mut host,
            );
            assert_eq!(scene.widgets.len(), spawned.len(), "spawn count");
            for want in &spawned {
                let got = scene.widgets.iter().find(|w| w[0x0a..0x0c] == want[0x0a..0x0c]).expect("widget");
                assert_eq!(got.as_slice(), want.as_slice(), "widget {:#x}", w(want, 0x0a));
            }
            assert!(host.rng.is_empty(), "unused random numbers");
            done += spawned.len();
        }
        eprintln!("duel spawns matched: {done}");
    }
}
