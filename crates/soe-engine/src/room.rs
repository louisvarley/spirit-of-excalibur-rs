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

/// Blit an indexed image, skipping palette index 0 and clipping to the frame.
pub(crate) fn blit_transparent(frame: &mut IndexedFrame, image: &PlanarImage, x: i32, y: i32, flip_x: bool) {
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
