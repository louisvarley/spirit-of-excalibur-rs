//! Replays army-battle calls recorded in the original by
//! `tests/oracle/army_vectors.py`.

use super::*;
use crate::resources::ResourceManager;
use crate::script::{ScriptEvent, ScriptWorld};
use serde_json::Value;
use std::collections::HashMap;

/// Bytes the compared functions do not own: DS:01EC (also skipped by the
/// arrival test; set outside the battle), the random seed, the clock
/// (the timer interrupt runs during a recording), the name buffers of
/// `FUN_2d65_08a4`, and the token image pointers (normalised separately).
const SKIP: [(usize, usize); 5] = [
    (0x01ec, 0x01ee),
    (0x01fc, 0x0200),
    (0x7e3e, 0x7e4a),
    (0x9d32, 0x9daa),
    (0x9cca, 0x9cda),
];
const RUNTIME_SEG: u16 = 0x2449 - 0x1000 + 0x214;

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

struct Replay {
    rng: Vec<u16>,
    fall_back: Vec<u16>,
}

impl ArmyWorld for Replay {
    fn random(&mut self, _n: u16) -> u16 {
        assert!(
            !self.rng.is_empty(),
            "port drew a random number the original did not"
        );
        self.rng.remove(0)
    }
    fn fall_back(&mut self, _ds: &[u8]) -> u16 {
        assert!(
            !self.fall_back.is_empty(),
            "port asked to fall back; the original did not"
        );
        self.fall_back.remove(0)
    }
}

struct Host<'a> {
    resources: Option<&'a mut ResourceManager>,
    sheets: HashMap<u32, Vec<u8>>,
}

impl ScriptWorld for Host<'_> {
    fn random(&mut self, _n: u16) -> u16 {
        0
    }
}

impl ScriptHost for Host<'_> {
    fn clock(&mut self) -> u32 {
        0
    }
    fn program(&self, _script: u8) -> Vec<u16> {
        Vec::new()
    }
    fn sheet(&self, image: u32) -> Vec<u8> {
        self.sheets.get(&image).cloned().unwrap_or_default()
    }
    fn load_image(&mut self, _old: u32, res: u16) -> u32 {
        self.load_sprite(res, 0)
    }
    fn load_sprite(&mut self, res: u16, _variant: u16) -> u32 {
        let h = u32::from(res);
        if let Some(r) = self.resources.as_deref_mut() {
            if !self.sheets.contains_key(&h) {
                let file = r.find_file(res).map(str::to_owned);
                if let Some(data) = file.and_then(|f| r.record(&f, res).ok().map(<[u8]>::to_vec)) {
                    let mut sheet = vec![0, 0];
                    sheet.extend_from_slice(&data);
                    self.sheets.insert(h, sheet);
                }
            }
        }
        h
    }
    fn static_image(&mut self, _res: u16) -> (u32, u16, u16) {
        (0, 0, 0)
    }
    fn retain_image(&mut self, _image: u32) {}
    fn release_image(&mut self, _flags: u16, _image: u32) {}
    fn event(&mut self, _event: ScriptEvent) {}
    fn hook(&mut self, _handler: u16, _arg: u16, _scene: &mut ScriptScene) -> u8 {
        2
    }
}

/// DGROUP and widgets of a recorded state, token image pointers replaced
/// by their resource ids and runtime callback segments by Ghidra's.
fn state(base: &[u8], st: &Value) -> (Vec<u8>, Vec<Widget>) {
    let mut ds = base.to_vec();
    for (addr, bytes) in st["regions"].as_object().unwrap() {
        let at = usize::from_str_radix(addr, 16).unwrap();
        let b = hex(bytes.as_str().unwrap());
        ds[at..at + b.len()].copy_from_slice(&b);
    }
    let ptrs: Vec<u32> = (0..4).map(|k| long(&ds, IMAGES + 4 * k)).collect();
    for k in 0..4 {
        set_long(&mut ds, IMAGES + 4 * k, u32::from(TOKEN_IMAGES) + k as u32);
    }
    let widgets = st["widgets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| {
            let mut w: Widget = hex(w.as_str().unwrap()).try_into().unwrap();
            let img = u32::from_le_bytes([w[2], w[3], w[4], w[5]]);
            if ww(&w, 0) & 0x248 == 0x248 {
                if let Some(k) = ptrs.iter().position(|&p| p == img) {
                    w[2..6].copy_from_slice(&(u32::from(TOKEN_IMAGES) + k as u32).to_le_bytes());
                }
            }
            if ww(&w, 8) == RUNTIME_SEG {
                w[8..10].copy_from_slice(&0x2449u16.to_le_bytes());
            }
            w
        })
        .collect();
    (ds, widgets)
}

fn locals(hexs: &str) -> RoundLocals {
    let b = hex(hexs);
    let w = |k: usize| u16::from_le_bytes([b[0x84 - k], b[0x85 - k]]);
    RoundLocals {
        rounds: w(0x56),
        result: b[0x84 - 0x7e],
        castle: [w(0x70), w(0x6e)],
        dx: [w(0x7c), w(0x7a)],
        dy: [w(0x78), w(0x76)],
    }
}

fn arg_value(a: &Arg) -> Value {
    match a {
        Arg::Number(n) => Value::from(*n),
        Arg::Char(c) => Value::from(*c),
        Arg::Text(t) => Value::from(t.clone()),
    }
}

/// The port's events in the recorder's terms.
fn subcalls(ev: &[ArmyEvent]) -> Vec<Value> {
    use serde_json::json;
    let mut out = Vec::new();
    for e in ev {
        match e {
            ArmyEvent::Message { format, args } => out.push(json!([
                "message",
                format,
                args.iter().map(arg_value).collect::<Vec<_>>()
            ])),
            ArmyEvent::Notice(f) => out.push(json!(["notice", f])),
            ArmyEvent::Wait(f) => out.push(json!(["wait", f])),
            ArmyEvent::Mark { x, y, code } => out.push(json!(["mark", x, y, code])),
            ArmyEvent::UnitRow { id, slot } => out.push(json!(["unit_row", id, slot])),
            ArmyEvent::Strength { .. } => {
                out.push(json!(["blit"]));
                out.push(json!(["text"]));
            }
            ArmyEvent::Music { name, number, flag } => {
                out.push(json!(["music", name, number, flag]))
            }
            ArmyEvent::MapMove(a, b) => out.push(json!(["map_071c", a, b])),
            ArmyEvent::Summary => out.push(json!(["box"])),
            ArmyEvent::ClearOrders => out.push(json!(["blit"])),
            ArmyEvent::MouseOff => out.push(json!(["mouse_off"])),
            ArmyEvent::MouseOn => out.push(json!(["mouse_on"])),
            ArmyEvent::Combat(_) => {}
        }
    }
    out
}

fn recorded(call: &Value) -> (Vec<Value>, Vec<u16>) {
    use serde_json::json;
    let mut out = Vec::new();
    let mut fall_back = Vec::new();
    for s in call["subcalls"].as_array().unwrap() {
        let s = s.as_array().unwrap();
        let n = |k: usize| s[k].as_u64().unwrap();
        match s[0].as_str().unwrap() {
            "message" => out.push(json!(["message", s[1]["format"], s[1]["args"]])),
            "fall_back" => fall_back.push(n(1) as u16),
            "music" => out.push(json!(["music", n(1), n(3), n(4)])),
            "unit_row" => out.push(json!(["unit_row", n(1) & 0xff, n(2)])),
            "notice" | "wait" => out.push(json!([s[0], n(1)])),
            "mark" => out.push(json!(["mark", n(1), n(2), n(3)])),
            "map_071c" => out.push(json!(["map_071c", n(1), n(2)])),
            _ => out.push(json!([s[0]])),
        }
    }
    (out, fall_back)
}

fn compare(ctx: &str, got: &[u8], want: &[u8], story: bool) {
    // A story event raised by a death (FUN_26c5_000b, not ported) changes
    // character records (Andulf's death disbands his army); then the
    // records are left out.
    for (a, n) in [
        (0x01e0usize, 0x200usize),
        (0x2c5e, 0x5e * 0x38),
        (0x4fbf, 4),
        (0x5e30, 0x10),
        (0x64b0, 0xa8),
        (0x9c3e, 0x1d0),
    ] {
        for at in a..a + n {
            if SKIP.iter().any(|&(lo, hi)| (lo..hi).contains(&at)) {
                continue;
            }
            if story && (CHAR..CHAR + 0x5e * CHAR_SIZE).contains(&at) {
                continue;
            }
            assert_eq!(got[at], want[at], "{ctx}: DS:{at:04X}");
        }
    }
}

#[test]
fn matches_original_army_calls() {
    let Ok(text) = std::fs::read_to_string("../../tests/vectors/army_calls.json") else {
        eprintln!("SKIP: tests/vectors/army_calls.json absent");
        return;
    };
    let v: Value = serde_json::from_str(&text).unwrap();
    let base = hex(v["dgroup"].as_str().unwrap());
    let game = std::path::Path::new("../../game");
    let exe = std::path::Path::new("../../ghidra/EXCAL_UNPACKED.EXE");
    let mut session = (game.join("EPISODE1.DAT").exists() && exe.exists())
        .then(|| crate::session::Session::load(game, exe, 1).unwrap());
    let mut done: HashMap<String, usize> = HashMap::new();
    for (n, call) in v["calls"].as_array().unwrap().iter().enumerate() {
        let f = call["fn"].as_str().unwrap();
        let ctx = format!("call {n} {f} (run {})", call["run"]);
        if f == "0aed" && session.is_none() {
            continue;
        }
        let (mut ds, widgets) = state(&base, &call["before"]);
        let (want_ds, want_widgets) = state(&base, &call["after"]);
        let mut scene = ScriptScene {
            widgets,
            current: call["before"]["current"].as_i64().unwrap() as i16,
            ..ScriptScene::default()
        };
        let (want_ev, fall_back) = recorded(call);
        let mut world = Replay {
            rng: call["rng"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r.as_u64().unwrap() as u16)
                .collect(),
            fall_back,
        };
        let mut ev = Vec::new();
        let args: Vec<u16> = call["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_u64().unwrap() as u16)
            .collect();
        let ret = call["ret"].as_u64().map(|r| r as u16);
        match f {
            "0aed" => {
                let mut host = Host {
                    resources: session.as_mut().map(|s| s.resources_mut()),
                    sheets: HashMap::new(),
                };
                fun_2449_0aed(&mut ds, &mut scene, &mut host);
            }
            "0255" => fun_2449_0255(&mut ds, &mut scene, &mut ev),
            "0faa" => fun_2449_0faa(&mut ds),
            "1021" => {
                let r = fun_2449_1021(
                    &mut ds, args[0], args[1], args[2], args[3], &mut world, &mut ev,
                );
                assert_eq!(u16::from(r), ret.unwrap() & 0xff, "{ctx}: result");
            }
            "12de" => {
                let r = fun_2449_12de(&mut ds, args[0]);
                assert_eq!(u16::from(r), ret.unwrap() & 0xff, "{ctx}: result");
            }
            "round" => {
                let mut loc = locals(call["locals_before"].as_str().unwrap());
                let end = fun_2449_134e_round(&mut ds, &mut scene, &mut loc, &mut world, &mut ev);
                let want_end = if call["end"].as_u64().unwrap() == 0x15cb {
                    RoundEnd::Continue
                } else {
                    RoundEnd::Over
                };
                assert_eq!(end, want_end, "{ctx}: end");
                let want_loc = locals(call["locals_after"].as_str().unwrap());
                assert_eq!(
                    (loc.rounds, loc.result),
                    (want_loc.rounds, want_loc.result),
                    "{ctx}: locals"
                );
            }
            _ => unreachable!(),
        }
        // Nested recorded calls (0faa inside 1021 or a round) were stubbed
        // nowhere: their effects are in the outer call's state too.
        assert!(
            world.rng.is_empty(),
            "{ctx}: unused random numbers {:?}",
            world.rng
        );
        // The recorder resolves only the arguments the format uses; the
        // original pushes the pronoun for every wound message.
        let mut got_ev = subcalls(&ev);
        for (g, w) in got_ev.iter_mut().zip(&want_ev) {
            if g[0] == "message" && w[0] == "message" {
                let keep = w[2].as_array().map_or(0, Vec::len);
                if let Some(a) = g[2].as_array_mut() {
                    a.truncate(keep);
                }
            }
        }
        assert_eq!(got_ev, want_ev, "{ctx}: delegated calls");
        let story = ev
            .iter()
            .any(|e| matches!(e, ArmyEvent::Combat(CombatEvent::StoryEvent { .. })));
        compare(&ctx, &ds, &want_ds, story);
        assert_eq!(
            scene.widgets.len(),
            want_widgets.len(),
            "{ctx}: widget count"
        );
        for (k, (a, b)) in scene.widgets.iter().zip(&want_widgets).enumerate() {
            assert_eq!(a.to_vec(), b.to_vec(), "{ctx}: widget {k}");
        }
        *done.entry(f.to_string()).or_default() += 1;
    }
    eprintln!("army calls matched: {done:?}");
}
