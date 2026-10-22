//! The room / field scene builder (segment `2b79`).
//!
//! `FUN_2b79_0ea5` picks and loads the `SCENES.RES` record for a scene
//! location; `FUN_2b79_0003` (2b79:0003-0ea4, which Ghidra splits into
//! `FUN_2b79_0003`, `FUN_2b79_0397`, `FUN_2b79_0a7f` and `FUN_2b79_0aca`)
//! builds the scene from it:
//!
//! - every object of the record, in order: still images are stamped into
//!   the background buffer (`DS:9680`); animations (ids `0x2BC..0x320` and
//!   `0x384..0x3E8`) and scenery (`0x76D..=0x770`) become script widgets
//!   (script 0x17); large foreground objects get an occluding widget; parts
//!   of still objects covering an animation become cut-out widgets (id
//!   0x3E7); some objects place people (`0x455`, `0x459`, ...). Army
//!   soldiers (`0x429..=0x42F`) only appear in a campaign battle whose
//!   `DS:9C56` flags ask for them.
//! - the background is copied to the screen (`DS:962C`) and the objects
//!   lying in the location (table `DS:4DD1`, filtered by the field type
//!   `DS:9DF5` and the scene mode `DS:9DAB`) are drawn and spawned.
//! - the palette rules (ported in [`crate::palette::PaletteSet::scene_palette`])
//!   run on every object processed, listed in [`RoomBuild::palette_ids`].
//!
//! It returns the room base (`0xFFE8` when object 0x3E8 is present, else
//! 0), whose low byte `FUN_2d65_13e2` adds to the floor line.
//!
//! Images stay handles from the host ([`ScriptHost::static_image`],
//! [`ScriptHost::load_sprite`], [`RoomHost::cut_image`]); the build returns
//! the stamps for the presenter to draw.

#![forbid(unsafe_code)]

use crate::script::{fun_18cb_16f0, fun_30ec_0006, ScriptHost, ScriptScene, SpawnArgs, Widget};

/// `DS:72EA`: scene ids for scene locations 0x3D and up (fields).
const FIELD_SCENES: usize = 0x72ea;

/// Objects whose lower edge is below the floor line get an occluding
/// widget (2b79:0416-061d).
const FOREGROUND: [u16; 56] = [
    0x3e9, 0x3ea, 0x3eb, 0x3f7, 0x40e, 0x472, 0x473, 0x408, 0x416, 0x417, 0x418, 0x41b, 0x41c, 0x41f,
    0x474, 0x475, 0x476, 0x480, 0x46e, 0x46f, 0x5dc, 0x5df, 0x51a, 0x51d, 0x51b, 0x580, 0x581, 0x582,
    0x583, 0x709, 0x70a, 0x70c, 0x70d, 0x6a5, 0x6a6, 0x6a7, 0x421, 0x422, 0x425, 0x428, 0x462, 0x438,
    0x439, 0x43a, 0x43b, 0x43b, 0x492, 0x493, 0x49b, 0x49c, 0x49d, 0x49e, 0x49f, 0x4a0, 0x58c, 0x58d,
];

/// Callbacks the builder gives widgets (Ghidra segment - 0x1000, as the
/// EXE stores them before relocation).
const PERSON_CALLBACK: (u16, u16) = (0x136d, 0x1d65);
const PERSON_CALLBACK_B: (u16, u16) = (0x1395, 0x1d65);

fn b(ds: &[u8], at: usize) -> u8 {
    ds[at]
}

fn w(ds: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([ds[at], ds[at + 1]])
}

fn set_w(ds: &mut [u8], at: usize, v: u16) {
    ds[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

/// `SCENES.RES` record for scene location `loc` in mode `mode`
/// (`FUN_2b79_0ea5`, 2b79:0eab-0ed4).
#[must_use]
pub fn scene_record(ds: &[u8], loc: u8, mode: u8) -> u16 {
    if loc < 0x3d {
        u16::from(mode).wrapping_mul(100).wrapping_add(u16::from(loc))
    } else {
        w(ds, FIELD_SCENES + usize::from(loc - 0x3d) * 2)
    }
}

/// `FUN_2b79_0ea5` after the record is loaded: store the object count
/// (`DS:9D18`) and the background colour (`DS:9D1A`) and, unless a tune is
/// already playing (`FUN_1316_0765`), choose the chapel or cathedral music
/// for scenes 0x38A..=0x39F. Returns the music to start
/// (`FUN_1316_187b(name, n, 0)`).
pub fn fun_2b79_0ea5(ds: &mut [u8], scene: u16, record: &[u8], music_playing: bool) -> Option<(&'static str, u16)> {
    ds[0x9d18] = record[0];
    ds[0x9d1a] = record[1];
    if music_playing || scene.wrapping_sub(0x38a) > 0x15 {
        return None;
    }
    // The CS:0F5A jump table.
    match scene - 0x38a {
        0 | 4 | 5 | 6 | 11 | 12 | 13 | 18 | 19 | 20 => Some(("CHAPEL", 0xe)),
        7 | 14 | 21 => Some(("CATHEDRL", 0xd)),
        _ => None,
    }
}

/// An image drawn by the builder (`FUN_14b0_0bbf` mode `0x46`: transparent,
/// bit 0 mirrored).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    /// Host image handle.
    pub image: u32,
    /// The resource it was loaded from.
    pub res: u16,
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    pub mirror: bool,
}

/// What `FUN_2b79_0003` drew and decided besides the widgets it spawned.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoomBuild {
    /// The background was first filled with this colour (`DS:9D1A`;
    /// only when the builder's argument is non-zero).
    pub fill: Option<u8>,
    /// Stills stamped into the background, in order.
    pub background: Vec<Stamp>,
    /// Objects lying in the location, drawn on the screen copy of the
    /// background (and also spawned as widgets).
    pub screen: Vec<Stamp>,
    /// Object ids the palette rules saw, in order.
    pub palette_ids: Vec<u16>,
    /// The return value (AX): the room base.
    pub base: u16,
}

/// What the builder needs beyond the scene engine's host.
pub trait RoomHost: ScriptHost {
    /// `FUN_14b0_0060(width, height)` then `FUN_14b0_0bbf` mode 0x46: a new
    /// image holding the `width` x `height` part of `src` (loaded from
    /// `res`) at (`sx`, `sy`).
    fn cut_image(&mut self, src: u32, res: u16, sx: u16, sy: u16, width: u16, height: u16) -> u32;
    /// `FUN_14b0_02c4`: drop one reference to an image.
    fn free_image(&mut self, image: u32);
}

/// `FUN_2b79_0003(arg)`: build the scene from `record` (the `SCENES.RES`
/// record `DS:72E6` holds) into `scene`. `ds` is DGROUP, with `DS:9D18` /
/// `DS:9D1A` set by [`fun_2b79_0ea5`]. A non-zero `arg` clears the
/// background first; 1 also presents the result (`FUN_1316_1111`).
///
/// Not ported: `FUN_1772_0578` on entry (restores the battle panel's
/// save-under when one is up, `DS:031E`), the mouse save-under restore
/// `FUN_17f3_003a` and the screen hand-offs `FUN_1316_1143` / `1111`.
pub fn fun_2b79_0003<H: RoomHost>(scene: &mut ScriptScene, ds: &mut [u8], record: &[u8], arg: u16, host: &mut H) -> RoomBuild {
    let mut out = RoomBuild::default();
    scene.fun_15b8_0978(1, 0xc8, host);
    let mut anim_placed = false; // [BP-5]
    let mut big = false; // [BP-4]
    let mut z: u16 = 1; // [BP-0E]
    ds[0x9d19] = 0;
    let mut people: u8 = 0; // [BP-2]
    ds[0x9dc2] = 0;
    let mut base: u16 = 0; // [BP-8]
    let mut union = [0i16; 4]; // [BP-24]
    // [BP-28]: the last widget spawned (only read in the iteration that
    // spawned it).
    let mut last: Option<usize> = None;
    if arg != 0 {
        out.fill = Some(ds[0x9d1a]);
    }
    let mut i: u16 = 0;
    while i < u16::from(ds[0x9d18]) {
        let at = 2 + usize::from(i) * 4;
        i += 1;
        let v = u32::from_le_bytes([record[at], record[at + 1], record[at + 2], record[at + 3]]);
        if v == 0 {
            continue;
        }
        let id = (v & 0x7ff) as u16;
        let x = (((v >> 11) & 0x1ff) as u16).wrapping_add(0xff42);
        let y = ((v >> 20) & 0xff) as u16;
        let flip = ((v >> 28) & 1) as u16;
        if (0x429..=0x42f).contains(&id) {
            let f = w(ds, 0x9c56);
            if ds[0x5e37] == 0 || f & 0x80 == 0 || (f & 0x100 != 0 && flip == 0) || (f & 0x200 != 0 && flip != 0) {
                continue;
            }
        }
        if id == 0x3e8 {
            base = 0xffe8;
        }
        let (img, mut width, mut height, anim);
        if (0x2bc..0x320).contains(&id) || (0x384..0x3e8).contains(&id) {
            img = host.load_sprite(id, 0);
            height = u16::from(host.sheet(img).get(9).copied().unwrap_or(0));
            width = 0x14;
            anim = true;
        } else {
            let (handle, w_, h_) = host.static_image(id);
            img = handle;
            width = w_;
            height = h_;
            set_w(ds, 0x99bf, y);
            set_w(ds, 0x99c1, x);
            set_w(ds, 0x99c3, y.wrapping_add(height));
            set_w(ds, 0x99c5, x.wrapping_add(width));
            anim = false;
        }
        if (anim_placed || big) && y.wrapping_add(height) >= z {
            z = y.wrapping_add(height);
        } else {
            z = z.wrapping_add(1);
        }
        if z > 200 {
            z = 200;
        }
        let high = if y < 0x5a { 0xc000 } else { 0 };
        let scenery = SpawnArgs { res: id, id, script: 0x17, depth: z as u8, x, ..SpawnArgs::default() };
        if (0x76d..=0x770).contains(&id) {
            let at = spawn(scene, host, SpawnArgs { flags: high | flip | 0x220, y, ..scenery });
            let wd = &mut scene.widgets[at];
            wd[0x27] = (id as u8).wrapping_add(0x96);
            union = fun_18cb_16f0(union, rect(wd, 0x10));
            last = Some(at);
            anim_placed = true;
        } else if anim {
            let at = spawn(scene, host, SpawnArgs { flags: high | flip | 0x240, y: y.wrapping_add(height), ..scenery });
            let wd = &mut scene.widgets[at];
            wd[0x27] = (id as u8).wrapping_add(0x83);
            if id == 0x389 {
                let v = w(wd, 0x12).wrapping_add(0xffab);
                set_w(wd, 0x12, v);
            }
            union = fun_18cb_16f0(union, rect(wd, 0x10));
            let v = w(wd, 0x1e);
            set_w(wd, 0x12, v);
            last = Some(at);
            anim_placed = true;
        } else {
            out.background.push(Stamp { image: img, res: id, x, y, width, height, mirror: flip != 0 });
            let floor = u16::from(ds[0x79f7 + usize::from(ds[0x9dab])]).wrapping_add(base);
            if y.wrapping_add(height) >= floor && FOREGROUND.contains(&id) {
                if !big {
                    z = y.wrapping_add(height);
                }
                if z > 200 {
                    z = 200;
                }
                big = true;
                let at = spawn(scene, host, SpawnArgs {
                    flags: 0x20 | flip,
                    res: id,
                    id,
                    script: id as u8,
                    depth: z as u8,
                    x,
                    y,
                    ..SpawnArgs::default()
                });
                last = Some(at);
                if ds[0x9d19] == 0 && id == 0x472 {
                    ds[0x9d19] = z as u8;
                    set_w(ds, 0x9d10, x);
                    set_w(ds, 0x9d12, y);
                }
            }
            if ds[0x9d19] == 0 && id == 0x46f {
                ds[0x9d19] = z as u8;
                set_w(ds, 0x9d10, (width >> 1).wrapping_add(x).wrapping_add(0xffef));
                set_w(ds, 0x9d12, y.wrapping_add(0xe));
            } else if anim_placed {
                let bx = [0x99bf, 0x99c1, 0x99c3, 0x99c5].map(|a| w(ds, a) as i16);
                let (r, visible) = fun_30ec_0006(bx, union);
                for (k, a) in [0x99bf, 0x99c1, 0x99c3, 0x99c5].into_iter().enumerate() {
                    set_w(ds, a, r[k] as u16);
                }
                if visible {
                    let mut sx = w(ds, 0x99c1).wrapping_sub(x);
                    let sy = w(ds, 0x99bf).wrapping_sub(y);
                    width = w(ds, 0x99c5).wrapping_sub(w(ds, 0x99c1));
                    height = w(ds, 0x99c3).wrapping_sub(w(ds, 0x99bf));
                    if flip != 0 {
                        sx = w(ds, 0x99c5).wrapping_sub(sx).wrapping_sub(width);
                    }
                    let cut = host.cut_image(img, id, sx, sy, width, height);
                    spawn(scene, host, SpawnArgs {
                        flags: 0x10 | flip,
                        image: cut,
                        id: 0x3e7,
                        depth: z as u8,
                        x: w(ds, 0x99c1),
                        y: w(ds, 0x99bf),
                        width,
                        height,
                        ..SpawnArgs::default()
                    });
                }
            }
        }
        // 2b79:07a1: people.
        let person = matches!(id, 0x455 | 0x459 | 0x486)
            || (matches!(id, 0x45a | 0x482) && (flip == 0 || ds[0x9df5] == 6));
        if person {
            let k = if ds[0x9df5] == 6 && people == 2 && ds[0x9dab] == 1 {
                0
            } else if ds[0x9df5] == 6 && people == 3 && ds[0x9dab] == 1 {
                6
            } else {
                people = people.min(2);
                ds[0x7a05 + usize::from(people) * 7 + usize::from(ds[0x9dab])]
            };
            spawn(scene, host, SpawnArgs {
                flags: 8,
                callback: if matches!(id, 0x459 | 0x45a) { PERSON_CALLBACK_B } else { PERSON_CALLBACK },
                id: u16::from(k),
                script: ds[0x79f0 + usize::from(k)],
                depth: z as u8,
                x,
                y,
                width: width.min(0x64),
                height,
                ..SpawnArgs::default()
            });
            people = people.wrapping_add(1);
        }
        if id == 0x489 {
            ds[0x9dc2] = 1;
            spawn(scene, host, SpawnArgs {
                flags: 0x20 | flip,
                res: 0x489,
                id: 0x489,
                script: 0x89,
                depth: z as u8,
                x,
                y,
                ..SpawnArgs::default()
            });
        }
        if ds[0x9dc1] != 0 && ds[0x9dab] == 0 {
            if id == 0x384 {
                if let Some(at) = last {
                    let wd = &mut scene.widgets[at];
                    set_w(wd, 8, PERSON_CALLBACK.1);
                    set_w(wd, 6, PERSON_CALLBACK.0);
                    set_w(wd, 0x0a, 1);
                }
                ds[0x9dc1] = 0x63;
            } else if id == 0x420 || id == 0x410 {
                spawn(scene, host, SpawnArgs {
                    flags: 8,
                    callback: PERSON_CALLBACK,
                    id: 1,
                    depth: z as u8,
                    x,
                    y,
                    width,
                    height,
                    ..SpawnArgs::default()
                });
                ds[0x9dc1] = 0x63;
            }
        }
        host.free_image(img);
        out.palette_ids.push(id);
    }

    // 2b79:0c52: background to screen, then the objects lying here.
    for k in 0..0x55usize {
        let e = &ds[0x4dd1 + k * 4..0x4dd1 + k * 4 + 4];
        let (owner, loc, flags) = (e[0], e[1], e[3]);
        if loc != ds[0x9df5] || flags & 0xe0 != 0 || flags & 0x18 != 0 || flags & 7 != ds[0x9dab] {
            continue;
        }
        let rec = 0x488b + usize::from(owner) * 0x1b;
        let res = w(ds, rec + 0x12);
        if res == 0 {
            continue;
        }
        let (img, width, height) = host.static_image(res);
        let mut lift = b(ds, rec + 0x15);
        let (x, y, depth);
        if ds[0x9d19] != 0 {
            x = host.random(5).wrapping_add(w(ds, 0x9d10).wrapping_add(0xe));
            y = u16::from(lift).wrapping_add(w(ds, 0x9d12)).wrapping_add(5).wrapping_sub(height);
            depth = u16::from(ds[0x9d19]).wrapping_add(u16::from(lift));
        } else {
            lift = lift.wrapping_add(host.random(5) as u8);
            x = host.random(0x14).wrapping_add(0xa0);
            let floor = u16::from(ds[0x79f7 + usize::from(ds[0x9dab])]);
            y = floor.wrapping_add(u16::from(lift)).wrapping_sub(height);
            let mut d = floor.wrapping_add(u16::from(lift));
            if matches!(res, 0x328 | 0x333 | 0x338) {
                d = d.wrapping_sub(10);
            }
            depth = d;
        }
        out.screen.push(Stamp { image: img, res, x, y, width, height, mirror: false });
        host.free_image(img);
        spawn(
            scene,
            host,
            SpawnArgs {
                flags: 0x1020,
                res,
                id: res,
                script: res as u8,
                depth: depth as u8,
                x,
                y,
                ..SpawnArgs::default()
            },
        );
    }
    out.base = base;
    out
}

fn spawn<H: RoomHost>(scene: &mut ScriptScene, host: &mut H, args: SpawnArgs) -> usize {
    scene.fun_15b8_0461(&args, host)
}

fn rect(widget: &Widget, at: usize) -> [i16; 4] {
    [0, 2, 4, 6].map(|k| w(widget, at + k) as i16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::PaletteSet;
    use crate::script::ScriptEvent;
    use std::collections::{HashMap, VecDeque};

    /// Runtime segment of Ghidra segment 0x1000 in the emulator
    /// (`tests/oracle/play.py` `LOAD_SEG`).
    const LOAD_SEG: u16 = 0x214;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    /// Hands out the image handles and random numbers the original got,
    /// checking each call's arguments.
    struct Replay {
        images: VecDeque<(String, Vec<u16>, u32, Vec<u8>)>,
        headers: HashMap<u32, Vec<u8>>,
        rng: VecDeque<u16>,
    }

    impl Replay {
        fn next(&mut self, kind: &str, args: &[u16]) -> (u32, Vec<u8>) {
            let (k, a, handle, header) = self.images.pop_front().expect("more image loads than the original");
            assert_eq!((k.as_str(), &a[..args.len()]), (kind, args), "image load");
            self.headers.insert(handle, header.clone());
            (handle, header)
        }
    }

    impl crate::script::ScriptWorld for Replay {
        fn random(&mut self, _n: u16) -> u16 {
            self.rng.pop_front().expect("more random numbers than the original")
        }
    }

    impl ScriptHost for Replay {
        fn clock(&mut self) -> u32 {
            0
        }
        fn program(&self, _script: u8) -> Vec<u16> {
            Vec::new()
        }
        fn sheet(&self, image: u32) -> Vec<u8> {
            self.headers.get(&image).cloned().unwrap_or_default()
        }
        fn load_image(&mut self, _old: u32, _res: u16) -> u32 {
            unreachable!()
        }
        fn load_sprite(&mut self, res: u16, variant: u16) -> u32 {
            self.next("0251", &[res, variant]).0
        }
        fn static_image(&mut self, res: u16) -> (u32, u16, u16) {
            let (handle, h) = self.next("0122", &[res]);
            (handle, w(&h, 0x0a), w(&h, 0x08))
        }
        fn retain_image(&mut self, _image: u32) {}
        fn release_image(&mut self, _flags: u16, _image: u32) {}
        fn event(&mut self, _event: ScriptEvent) {}
        fn hook(&mut self, _handler: u16, _arg: u16, _scene: &mut ScriptScene) -> u8 {
            2
        }
    }

    impl RoomHost for Replay {
        fn cut_image(&mut self, _src: u32, _res: u16, _sx: u16, _sy: u16, width: u16, height: u16) -> u32 {
            self.next("0060", &[width, height]).0
        }
        fn free_image(&mut self, _image: u32) {}
    }

    fn u16s(v: &serde_json::Value) -> Vec<u16> {
        v.as_array().unwrap().iter().map(|a| a.as_u64().unwrap() as u16).collect()
    }

    fn check_file(file: &str) -> usize {
        let Ok(text) = std::fs::read_to_string(format!("../../tests/vectors/{file}")) else {
            eprintln!("SKIP: tests/vectors/{file} absent (tests/oracle/room_vectors.py)");
            return 0;
        };
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let runs = v["runs"].as_array().unwrap();
        for (n, run) in runs.iter().enumerate() {
            let ctx = format!("{file} run {n}");
            let mut ds = hex(run["ds"].as_str().unwrap());
            let record = hex(run["record"].as_str().unwrap());
            let arg = run["arg"].as_u64().unwrap() as u16;
            let widgets = |key: &str| -> Vec<Widget> {
                run[key].as_array().unwrap().iter().map(|p| hex(p[1].as_str().unwrap()).try_into().unwrap()).collect()
            };
            let mut scene = ScriptScene { widgets: widgets("widgets_in"), ..ScriptScene::default() };
            let mut host = Replay {
                images: run["images"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|i| (i[0].as_str().unwrap().to_owned(), u16s(&i[1]), i[2].as_u64().unwrap() as u32, hex(i[3].as_str().unwrap())))
                    .collect(),
                headers: HashMap::new(),
                rng: u16s(&run["rng"]).into(),
            };
            let palettes = PaletteSet::from_dgroup(&ds).unwrap();
            let build = fun_2b79_0003(&mut scene, &mut ds, &record, arg, &mut host);
            assert!(host.images.is_empty(), "{ctx}: image loads left: {:?}", host.images);
            assert!(host.rng.is_empty(), "{ctx}: random numbers left");
            assert_eq!(build.base, run["ax"].as_u64().unwrap() as u16, "{ctx}: AX");

            let blits: Vec<(u64, Vec<u16>)> =
                run["blits"].as_array().unwrap().iter().map(|b| (b[0].as_u64().unwrap(), u16s(&b[1]))).collect();
            // FUN_14b0_0bbf words: src (off, seg), sx, sy, dst (off, seg),
            // dx, dy, width, height, colour/remap, mode.
            let stamps = |site: u64| -> Vec<(u32, u16, u16, u16, u16, bool)> {
                blits
                    .iter()
                    .filter(|(s, _)| *s == site)
                    .map(|(_, a)| (u32::from(a[1]) << 16 | u32::from(a[0]), a[6], a[7], a[8], a[9], a[11] & 1 != 0))
                    .collect()
            };
            let ours = |s: &[Stamp]| -> Vec<_> { s.iter().map(|s| (s.image, s.x, s.y, s.width, s.height, s.mirror)).collect() };
            assert_eq!(ours(&build.background), stamps(0x03f0), "{ctx}: background stamps");
            assert_eq!(ours(&build.screen), stamps(0x0e3e), "{ctx}: screen stamps");
            let fill: Vec<u8> = blits.iter().filter(|(s, _)| *s == 0x00c0).map(|(_, a)| a[10] as u8).collect();
            assert_eq!(build.fill.into_iter().collect::<Vec<_>>(), fill, "{ctx}: fill");

            let mut got = scene.widgets.clone();
            for wd in &mut got {
                if w(wd, 8) == PERSON_CALLBACK.1 {
                    set_w(wd, 8, PERSON_CALLBACK.1 + LOAD_SEG);
                }
            }
            let want = widgets("widgets_out");
            assert_eq!(got.len(), want.len(), "{ctx}: widget count");
            for (k, (g, e)) in got.iter().zip(&want).enumerate() {
                assert_eq!(g.as_slice(), e.as_slice(), "{ctx}: widget {k}");
            }

            let out = &run["ds_out"];
            let dso = |k: &str| hex(out[k].as_str().unwrap());
            assert_eq!(ds[0x9d10..0x9d14], dso("9d10")[..], "{ctx}: 9D10");
            assert_eq!(ds[0x9d19], dso("9d19")[0], "{ctx}: 9D19");
            assert_eq!(ds[0x9dc1..0x9dc3], dso("9dc1")[..], "{ctx}: 9DC1");
            assert_eq!(ds[0x99bf..0x99c7], dso("99bf")[..], "{ctx}: 99BF");
            let ptr = dso("8e5d");
            let off = usize::from(u16::from_le_bytes([ptr[0], ptr[1]]));
            let table = if off == 0x9ba0 { dso("9ba0") } else { ds[off..off + 0x60].to_vec() };
            let want: Vec<[u8; 3]> = table.chunks(3).map(|c| [c[0] * 17, c[1] * 17, c[2] * 17]).collect();
            let pal = palettes.scene_palette(build.palette_ids.iter().copied());
            assert_eq!(pal.colors, want, "{ctx}: palette");
        }
        runs.len()
    }

    /// Every `SCENES.RES` record built in the original from the hall
    /// snapshot, and the field scene of a recorded duel
    /// (`tests/oracle/room_vectors.py`).
    #[test]
    fn matches_recorded_builds() {
        let n: usize = [
            "room_all0.json",
            "room_all1.json",
            "room_all2.json",
            "room_p1_0.json",
            "room_p1_1.json",
            "room_p1_2.json",
            "room_p2_0.json",
            "room_p2_1.json",
            "room_p2_2.json",
            "room_duel.json",
        ].iter().map(|f| check_file(f)).sum();
        eprintln!("{n} builds matched");
    }

    #[test]
    fn chooses_records_and_music() {
        let mut ds = vec![0u8; 0x10000];
        ds[FIELD_SCENES + 2] = 0x8b;
        ds[FIELD_SCENES + 3] = 0x03;
        assert_eq!(scene_record(&ds, 5, 9), 905);
        assert_eq!(scene_record(&ds, 0x3e, 0), 0x38b);
        assert_eq!(fun_2b79_0ea5(&mut ds, 0x38a, &[3, 14], false), Some(("CHAPEL", 0xe)));
        assert_eq!((ds[0x9d18], ds[0x9d1a]), (3, 14));
        assert_eq!(fun_2b79_0ea5(&mut ds, 0x391, &[0, 0], false), Some(("CATHEDRL", 0xd)));
        assert_eq!(fun_2b79_0ea5(&mut ds, 0x391, &[0, 0], true), None);
        assert_eq!(fun_2b79_0ea5(&mut ds, 0x38b, &[0, 0], false), None);
    }
}
