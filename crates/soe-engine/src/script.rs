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
//! combat (`combat::fun_20b5_1dff`). Movement (1, 8/9), signalling (5),
//! spawning (12) and resource/depth changes (13) are not ported yet.

#![forbid(unsafe_code)]

pub type Widget = [u8; 0x36];

/// What a handler asks of the outside world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptEvent {
    /// `FUN_1316_181d(n)`: start music.
    Music(u16),
    /// `FUN_1316_183c(n)`: play a sound.
    Sound(u16),
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
    matches!(handler, 0 | 2 | 3 | 4 | 6 | 7 | 10 | 11)
}

/// Run handler `handler` (0..=15) with `arg` (`word & 0xFFF`) on `widget`.
/// `clock` is the 32-bit timer at `DS:7E3E`; `stale` is the word above
/// the argument on the caller's stack, which handler 11 reads as its sound
/// number (it is declared with two parameters but called with one).
/// Returns AL, or `None` for handlers not ported yet.
pub fn run_handler(
    handler: u16,
    arg: u16,
    widget: &mut Widget,
    clock: u32,
    stale: u16,
    world: &mut impl ScriptWorld,
    events: &mut Vec<ScriptEvent>,
) -> Option<u8> {
    Some(match handler {
        0 => fun_18cb_08ff(widget, arg, clock),
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
            events.push(ScriptEvent::Sound(stale));
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

#[cfg(test)]
mod tests {
    use super::*;

    struct Replay(Vec<u16>);

    impl ScriptWorld for Replay {
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
            let mut world = Replay(
                call["rng"].as_array().unwrap().iter().map(|r| r.as_u64().unwrap() as u16).collect(),
            );
            let mut events = Vec::new();
            let arg = call["arg"].as_u64().unwrap() as u16;
            let clock = call["clock"].as_u64().unwrap() as u32;
            let ret = run_handler(handler, arg, &mut widget, clock, 0, &mut world, &mut events).unwrap();
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
}
