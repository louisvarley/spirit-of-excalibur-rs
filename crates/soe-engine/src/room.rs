//! Room composition from `SCENES.RES` object lists.
//!
//! Object order is draw order. `x` is signed and is clipped; `y` is the top
//! coordinate. Palette index 0 is transparent for room images. FRML animation
//! objects are skipped until their per-frame pixel codec is reversed.

#![forbid(unsafe_code)]

use soe_assets::Palette;
use soe_video::{IndexedFrame, LOGICAL_HEIGHT, LOGICAL_WIDTH, PlanarImage};

use crate::scene::Scene;

/// Compose one room frame. `resolve` returns a decoded still image for an
/// object id, or `None` for animations/missing records.
#[must_use]
pub fn compose_scene(
    scene: &Scene,
    palette: &Palette,
    mut resolve: impl FnMut(u16) -> Option<PlanarImage>,
) -> IndexedFrame {
    let mut frame = IndexedFrame::new(
        LOGICAL_WIDTH,
        LOGICAL_HEIGHT,
        vec![0; usize::from(LOGICAL_WIDTH) * usize::from(LOGICAL_HEIGHT)],
        palette.clone(),
    )
    .expect("logical frame dimensions");

    for object in &scene.objects {
        let Some(image) = resolve(object.resource_id) else {
            continue;
        };
        blit_transparent(
            &mut frame,
            &image,
            i32::from(object.x),
            i32::from(object.y),
            object.flags & 1 != 0,
        );
    }

    frame
}

/// Handle bit for still images, so they never collide with sprite banks
/// (whose handle is the resource id).
pub const STILL: u32 = 0x1_0000;
/// Handle bit for cut-out images (`FUN_14b0_0060`), indexing
/// [`SceneImages::cut_of`].
pub const CUT: u32 = 0x2_0000;

/// A part of a still image, made by the room builder for a cut-out widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cut {
    pub res: u16,
    pub sx: u16,
    pub sy: u16,
    pub width: u16,
    pub height: u16,
}

/// The images a scene has loaded, by handle: the stand-in for the
/// original's image heap. Images stay resource records until drawn.
#[derive(Debug, Clone, Default)]
pub struct SceneImages {
    /// Sprite banks (`FUN_14b0_0251`), handle = resource id, as
    /// `[refcount, 0, FRML record...]` (the layout the scene engine reads).
    sheets: std::collections::HashMap<u32, Vec<u8>>,
    /// Stills (`FUN_14b0_0122`), handle `STILL | resource`.
    stills: std::collections::HashMap<u32, Vec<u8>>,
    cuts: Vec<Cut>,
}

impl SceneImages {
    fn fetch(resources: &mut crate::resources::ResourceManager, res: u16) -> Option<Vec<u8>> {
        let file = resources.find_file(res)?.to_owned();
        resources.record(&file, res).ok().map(<[u8]>::to_vec)
    }

    /// `FUN_14b0_0251`: a sprite bank's handle (0 when missing).
    pub fn load_sprite(&mut self, res: u16, resources: &mut crate::resources::ResourceManager) -> u32 {
        let handle = u32::from(res);
        if !self.sheets.contains_key(&handle) {
            let Some(data) = Self::fetch(resources, res) else { return 0 };
            let mut sheet = vec![0, 0];
            sheet.extend_from_slice(&data);
            self.sheets.insert(handle, sheet);
        }
        handle
    }

    /// `FUN_14b0_0122`: a still's handle, width and height.
    pub fn static_image(&mut self, res: u16, resources: &mut crate::resources::ResourceManager) -> (u32, u16, u16) {
        let handle = STILL | u32::from(res);
        if !self.stills.contains_key(&handle) {
            let Some(data) = Self::fetch(resources, res) else { return (0, 0, 0) };
            self.stills.insert(handle, data);
        }
        match soe_video::decode_planar(&self.stills[&handle]) {
            Ok(image) => (handle, image.width, image.height),
            Err(_) => (0, 0, 0),
        }
    }

    /// `FUN_14b0_0060` + copy: a new image cut from a still.
    pub fn cut(&mut self, cut: Cut) -> u32 {
        self.cuts.push(cut);
        CUT | (self.cuts.len() as u32 - 1)
    }

    /// A sprite bank's header bytes (empty when unknown).
    #[must_use]
    pub fn sheet(&self, image: u32) -> Vec<u8> {
        self.sheets.get(&image).cloned().unwrap_or_default()
    }

    /// The resource record of a loaded still.
    #[must_use]
    pub fn still(&self, res: u16) -> Option<&[u8]> {
        self.stills.get(&(STILL | u32::from(res))).map(Vec::as_slice)
    }

    /// The `ANI.RES` frame bank of a loaded sprite resource.
    #[must_use]
    pub fn bank(&self, res: u16) -> Option<&[u8]> {
        self.sheets.get(&u32::from(res)).map(|sheet| &sheet[2..])
    }

    #[must_use]
    pub fn cut_of(&self, image: u32) -> Option<Cut> {
        if image & CUT == 0 {
            return None;
        }
        self.cuts.get((image & 0xffff) as usize).copied()
    }

    /// Decode frame `frame` of the image behind `handle`.
    #[must_use]
    pub fn image(&self, handle: u32, frame: u8) -> Option<PlanarImage> {
        if let Some(c) = self.cut_of(handle) {
            let src = soe_video::decode_planar(self.still(c.res)?).ok()?;
            let mut indices = vec![0u8; usize::from(c.width) * usize::from(c.height)];
            for y in 0..c.height {
                for x in 0..c.width {
                    let (sx, sy) = (c.sx + x, c.sy + y);
                    if sx < src.width && sy < src.height {
                        indices[usize::from(y) * usize::from(c.width) + usize::from(x)] =
                            src.indices[usize::from(sy) * usize::from(src.width) + usize::from(sx)];
                    }
                }
            }
            return Some(PlanarImage { width: c.width, height: c.height, planes: src.planes, indices });
        }
        if handle & STILL != 0 {
            return soe_video::decode_planar(self.still(handle as u16)?).ok();
        }
        crate::ani::FrameBank::parse(self.bank(handle as u16)?).ok()?.frame_image(usize::from(frame & 0x7f))
    }
}

/// Draw a built room's background: the fill colour, then the stills
/// stamped into it, in order.
pub fn draw_background(frame: &mut IndexedFrame, build: &crate::room_builder::RoomBuild, images: &SceneImages) {
    if let Some(colour) = build.fill {
        frame.pixels.fill(colour);
    }
    for s in &build.background {
        if let Some(image) = images.image(s.image, 0) {
            blit_transparent(frame, &image, i32::from(s.x as i16), i32::from(s.y as i16), s.mirror);
        }
    }
}

/// Draw scene widgets back to front (the original draws the list from its
/// end), skipping widgets without an image, hidden frames (bit 7) and
/// widgets parked at x >= 9999. `remap` gives the colour table for a
/// widget's `+0x0C`.
pub fn draw_widgets(
    frame: &mut IndexedFrame,
    widgets: &[crate::script::Widget],
    images: &SceneImages,
    remap: impl Fn(u16) -> Option<[u8; 32]>,
) {
    for w in widgets.iter().rev() {
        let handle = u32::from_le_bytes([w[2], w[3], w[4], w[5]]);
        let x = i16::from_le_bytes([w[0x12], w[0x13]]);
        if handle == 0 || w[0x26] & 0x80 != 0 || x >= 0x270f {
            continue;
        }
        let Some(image) = images.image(handle, w[0x26]) else { continue };
        let table = remap(u16::from_le_bytes([w[0x0c], w[0x0d]]));
        let y = i16::from_le_bytes([w[0x10], w[0x11]]);
        blit_remapped(frame, &image, i32::from(x), i32::from(y), w[0] & 1 != 0, table.as_ref());
    }
}

/// Blit an indexed image, skipping palette index 0 and clipping to the frame.
pub(crate) fn blit_transparent(frame: &mut IndexedFrame, image: &PlanarImage, x: i32, y: i32, flip_x: bool) {
    blit_remapped(frame, image, x, y, flip_x, None);
}

/// [`blit_transparent`] with an optional index remap applied to every
/// non-zero source pixel (MCGA.DVR `drv_0`, flag 2).
pub(crate) fn blit_remapped(
    frame: &mut IndexedFrame,
    image: &PlanarImage,
    x: i32,
    y: i32,
    flip_x: bool,
    remap: Option<&[u8; 32]>,
) {
    let width = i32::from(image.width);
    for sy in 0..i32::from(image.height) {
        let dy = y + sy;
        if !(0..i32::from(LOGICAL_HEIGHT)).contains(&dy) {
            continue;
        }
        for sx in 0..width {
            let dx = x + sx;
            if !(0..i32::from(LOGICAL_WIDTH)).contains(&dx) {
                continue;
            }
            let source_x = if flip_x { width - 1 - sx } else { sx };
            let index = image.indices[(sy * width + source_x) as usize];
            if index != 0 {
                let index = remap.map_or(index, |table| table.get(usize::from(index)).copied().unwrap_or(index));
                frame.set_pixel(dx as u16, dy as u16, index);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::SceneObject;

    fn image(width: u16, height: u16, indices: Vec<u8>) -> PlanarImage {
        PlanarImage {
            width,
            height,
            planes: 5,
            indices,
        }
    }

    #[test]
    fn draws_in_order_with_transparency_and_clipping() {
        let scene = Scene {
            background: 0,
            objects: vec![
                SceneObject {
                    resource_id: 1,
                    x: 0,
                    y: 0,
                    flags: 0,
                },
                SceneObject {
                    resource_id: 2,
                    x: 319,
                    y: 199,
                    flags: 0,
                },
            ],
            trailer: 0,
        };
        let frame = compose_scene(&scene, &Palette::black(), |id| match id {
            1 => Some(image(2, 2, vec![1, 0, 2, 3])),
            2 => Some(image(2, 2, vec![7, 8, 9, 10])),
            _ => None,
        });
        assert_eq!(frame.pixel(0, 0), Some(1));
        assert_eq!(frame.pixel(1, 0), Some(0));
        assert_eq!(frame.pixel(0, 1), Some(2));
        assert_eq!(frame.pixel(1, 1), Some(3));
        assert_eq!(frame.pixel(319, 199), Some(7));
    }

    #[test]
    fn flag_bit_zero_flips_horizontally() {
        let scene = Scene {
            background: 0,
            objects: vec![SceneObject {
                resource_id: 1,
                x: 0,
                y: 0,
                flags: 1,
            }],
            trailer: 0,
        };
        let frame = compose_scene(&scene, &Palette::black(), |_| Some(image(2, 1, vec![1, 2])));
        assert_eq!(frame.pixel(0, 0), Some(2));
        assert_eq!(frame.pixel(1, 0), Some(1));
    }

    #[test]
    fn skips_negative_and_offscreen_objects() {
        let scene = Scene {
            background: 0,
            objects: vec![SceneObject {
                resource_id: 1,
                x: -2,
                y: 0,
                flags: 0,
            }],
            trailer: 0,
        };
        let frame = compose_scene(&scene, &Palette::black(), |_| Some(image(1, 1, vec![9])));
        assert_eq!(frame.pixel(0, 0), Some(0));
        assert!(frame.pixels.iter().all(|&p| p == 0));
    }
}
