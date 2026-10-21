//! Regional map data and compositor.
//!
//! The original map is not a single picture. `INTF.RES` holds:
//!
//! - record **1** (`MMAP`): a 72 x 60 byte tile table;
//! - record **500**: a 256 x 256 five-plane tile atlas (16 x 16 tiles);
//! - record **501**: the 320 x 200 ornate frame (transparent centre);
//! - records **600..615**: the location/town/castle icons.
//!
//! A location record's `kind` field (`600..=614`) is the icon id, while its
//! world x/y are pixels in the 1152 x 960 tiled map. The map viewport is
//! 272 x 160 at logical `(16, 16)`; the camera is stored in 16-pixel tile
//! units in the original and is centred on the current location.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::path::Path;

use soe_assets::Palette;
use soe_res::ResFile;
use soe_video::{IndexedFrame, PlanarImage, decode_planar};

use crate::save::GameState;

/// Map tiles per row/column (DGROUP camera fields `+0x44`/`+0x46`).
pub const TILES_W: usize = 72;
pub const TILES_H: usize = 60;
pub const TILE_SIZE: i32 = 16;
pub const MAP_WIDTH: i32 = (TILES_W as i32) * TILE_SIZE;
pub const MAP_HEIGHT: i32 = (TILES_H as i32) * TILE_SIZE;

/// Map viewport inside the 320 x 200 frame.
pub const VIEW_X: i32 = 16;
pub const VIEW_Y: i32 = 16;
pub const VIEW_W: i32 = 272;
pub const VIEW_H: i32 = 160;

/// Camera bias used by `FUN_20b5_2a30` before `FUN_1851_06e1`.
pub const CAMERA_BIAS_X: i32 = 144;
pub const CAMERA_BIAS_Y: i32 = 80;

/// One travel destination from the save/EXE location table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocationMarker {
    /// 0-based row in [`GameState::locations`].
    pub location_index: usize,
    pub name: String,
    pub world_x: u16,
    pub world_y: u16,
    /// Map icon resource id (`kind`, normally `INTF#600..=615`).
    pub icon_id: u16,
}

/// Non-empty locations in save-table order.
#[must_use]
pub fn markers(state: &GameState) -> Vec<LocationMarker> {
    state
        .locations
        .iter()
        .enumerate()
        .filter_map(|(location_index, location)| {
            let name = location.name();
            (!name.is_empty()).then(|| LocationMarker {
                location_index,
                name,
                world_x: location.world_x(),
                world_y: location.world_y(),
                icon_id: location.icon_id(),
            })
        })
        .collect()
}

/// Scene ids tried for a location, in the original loader's order.
///
/// `FUN_2b79_0ea5` builds `region * 100 + location` for location numbers below
/// `0x3d`. When that record is absent `FUN_2d65_13e2` retries with region 0,
/// which is just the location number.
#[must_use]
pub const fn scene_candidates(location_index: usize, region: u16) -> [u16; 2] {
    let local = location_index as u16 + 1;
    [region.wrapping_mul(100).wrapping_add(local), local]
}

/// Generic scene used by the original for location type codes `0x3d` and up.
#[must_use]
pub const fn generic_scene_for_icon(icon_id: u16) -> Option<u16> {
    match icon_id {
        600..=603 => Some(901),
        604 => Some(902),
        605 => Some(903),
        606 | 608 => Some(904),
        607 => Some(905),
        609..=611 => Some(906),
        612 => Some(907),
        613 => Some(908),
        614 => Some(909),
        _ => None,
    }
}

/// Resolve a location to a `SCENES.RES` record that actually exists.
#[must_use]
pub fn resolve_scene_id(
    location_index: usize,
    icon_id: u16,
    region: u16,
    available: impl Fn(u16) -> bool,
) -> Option<u16> {
    scene_candidates(location_index, region)
        .into_iter()
        .find(|id| available(*id))
        .or_else(|| generic_scene_for_icon(icon_id).filter(|id| available(*id)))
}

/// Camera top-left for a location, clamped to the map and snapped to a tile.
#[must_use]
pub fn camera_for(world_x: u16, world_y: u16) -> (i32, i32) {
    let max_x = (MAP_WIDTH - VIEW_W).max(0);
    let max_y = (MAP_HEIGHT - VIEW_H).max(0);
    let x = (i32::from(world_x) - CAMERA_BIAS_X).clamp(0, max_x);
    let y = (i32::from(world_y) - CAMERA_BIAS_Y).clamp(0, max_y);
    (x / TILE_SIZE * TILE_SIZE, y / TILE_SIZE * TILE_SIZE)
}

/// Nearest non-empty location to a world point.
#[must_use]
pub fn nearest_location(markers: &[LocationMarker], world_x: u16, world_y: u16) -> Option<usize> {
    markers
        .iter()
        .min_by_key(|marker| {
            let dx = i32::from(marker.world_x) - i32::from(world_x);
            let dy = i32::from(marker.world_y) - i32::from(world_y);
            dx * dx + dy * dy
        })
        .map(|marker| marker.location_index)
}

/// Decoded regional-map assets, kept indexed for palette/HD override handling.
#[derive(Debug, Clone)]
pub struct WorldMap {
    tilemap: Vec<u8>,
    tiles: PlanarImage,
    frame: PlanarImage,
    icons: HashMap<u16, PlanarImage>,
}

impl WorldMap {
    /// Load the map records from `RES32/INTF.RES`.
    pub fn load(intf_path: impl AsRef<Path>) -> Result<Self, String> {
        let path = intf_path.as_ref();
        let file = ResFile::parse(
            std::fs::read(path).map_err(|error| format!("reading {}: {error}", path.display()))?,
        )
        .map_err(|error| format!("parsing {}: {error}", path.display()))?;

        let tilemap = decode_record(&file, 1)?;
        if tilemap.len() < TILES_W * TILES_H {
            return Err(format!(
                "map tile table is {} bytes, expected {}",
                tilemap.len(),
                TILES_W * TILES_H
            ));
        }
        let tilemap = tilemap[..TILES_W * TILES_H].to_vec();
        let tiles = decode_image(&file, 500)?;
        let frame = decode_image(&file, 501)?;

        let mut icons = HashMap::new();
        for id in 600..=615 {
            if let Ok(image) = decode_image(&file, id) {
                icons.insert(id, image);
            }
        }

        Ok(Self {
            tilemap,
            tiles,
            frame,
            icons,
        })
    }

    /// The 72 x 60 tile table.
    #[must_use]
    pub fn tilemap(&self) -> &[u8] {
        &self.tilemap
    }

    /// One map icon by `INTF` resource id.
    #[must_use]
    pub fn icon(&self, id: u16) -> Option<&PlanarImage> {
        self.icons.get(&id)
    }

    /// Compose the background, markers and ornate frame into a 320 x 200 frame.
    pub fn render(
        &self,
        palette: &Palette,
        camera: (i32, i32),
        markers: &[LocationMarker],
        selected: Option<usize>,
        current: Option<usize>,
    ) -> Result<IndexedFrame, String> {
        let mut frame = IndexedFrame::new(320, 200, vec![0; 320 * 200], palette.clone())
            .map_err(|error| error.to_string())?;

        for y in 0..VIEW_H {
            let world_y = camera.1 + y;
            if !(0..MAP_HEIGHT).contains(&world_y) {
                continue;
            }
            let tile_y = (world_y / TILE_SIZE) as usize;
            for x in 0..VIEW_W {
                let world_x = camera.0 + x;
                if !(0..MAP_WIDTH).contains(&world_x) {
                    continue;
                }
                let tile_x = (world_x / TILE_SIZE) as usize;
                let tile = usize::from(self.tilemap[tile_y * TILES_W + tile_x]);
                let atlas_x = (tile % 16) as i32 * TILE_SIZE + world_x % TILE_SIZE;
                let atlas_y = (tile / 16) as i32 * TILE_SIZE + world_y % TILE_SIZE;
                if let Some(pixel) = self.pixel_at(&self.tiles, atlas_x, atlas_y) {
                    frame.set_pixel((VIEW_X + x) as u16, (VIEW_Y + y) as u16, pixel);
                }
            }
        }

        for marker in markers {
            let Some(icon) = self.marker_icon(marker) else {
                continue;
            };
            let x = i32::from(marker.world_x) - camera.0;
            let y = i32::from(marker.world_y) - camera.1;
            let width = icon_frame_width(icon);
            self.blit_marker(&mut frame, icon, x, y);
            if selected == Some(marker.location_index) {
                outline(&mut frame, x, y, width, icon.height, 15);
            } else if current == Some(marker.location_index) {
                outline(&mut frame, x, y, width, icon.height, 10);
            }
        }

        // The frame's centre is transparent; only its non-zero pixels overlay.
        for y in 0..200usize {
            for x in 0..320usize {
                let pixel = self.frame.indices[y * 320 + x];
                if pixel != 0 {
                    frame.set_pixel(x as u16, y as u16, pixel);
                }
            }
        }
        Ok(frame)
    }

    /// Hit-test a marker in logical screen coordinates.
    ///
    /// A marker is selected when a non-transparent icon pixel or its selection
    /// outline is clicked. This mirrors the original's sprite hit test.
    #[must_use]
    pub fn marker_at(
        &self,
        camera: (i32, i32),
        markers: &[LocationMarker],
        screen_x: i16,
        screen_y: i16,
    ) -> Option<usize> {
        let px = i32::from(screen_x);
        let py = i32::from(screen_y);
        if !(VIEW_X..VIEW_X + VIEW_W).contains(&px) || !(VIEW_Y..VIEW_Y + VIEW_H).contains(&py) {
            return None;
        }
        // Later markers win, matching the original draw order.
        for marker in markers.iter().rev() {
            let Some(icon) = self.marker_icon(marker) else {
                continue;
            };
            let x = i32::from(marker.world_x) - camera.0 - VIEW_X;
            let y = i32::from(marker.world_y) - camera.1 - VIEW_Y;
            let width = icon_frame_width(icon);
            let sx = px - VIEW_X - x;
            let sy = py - VIEW_Y - y;
            if sx >= -1
                && sy >= -1
                && sx <= width
                && sy <= i32::from(icon.height)
                && (sx < 0
                    || sy < 0
                    || sx >= width
                    || sy >= i32::from(icon.height)
                    || icon.indices[sy as usize * usize::from(icon.width) + sx as usize] != 0)
            {
                return Some(marker.location_index);
            }
        }
        None
    }

    /// Some `kind` values (608/610 in the shipped data) are not image records;
    /// fall back to the ordinary village icon so every location stays visible.
    fn marker_icon(&self, marker: &LocationMarker) -> Option<&PlanarImage> {
        self.icons
            .get(&marker.icon_id)
            .or_else(|| self.icons.get(&605))
    }

    fn blit_marker(&self, frame: &mut IndexedFrame, icon: &PlanarImage, x: i32, y: i32) {
        let width = icon_frame_width(icon);
        for sy in 0..i32::from(icon.height) {
            let dy = VIEW_Y + y + sy;
            if !(VIEW_Y..VIEW_Y + VIEW_H).contains(&dy) {
                continue;
            }
            for sx in 0..width {
                let dx = VIEW_X + x + sx;
                if !(VIEW_X..VIEW_X + VIEW_W).contains(&dx) {
                    continue;
                }
                let pixel = icon.indices[sy as usize * usize::from(icon.width) + sx as usize];
                if pixel != 0 {
                    frame.set_pixel(dx as u16, dy as u16, pixel);
                }
            }
        }
    }

    fn pixel_at(&self, image: &PlanarImage, x: i32, y: i32) -> Option<u8> {
        if x < 0 || y < 0 || x >= i32::from(image.width) || y >= i32::from(image.height) {
            return None;
        }
        Some(image.indices[y as usize * usize::from(image.width) + x as usize])
    }
}

fn decode_record(file: &ResFile, id: u16) -> Result<Vec<u8>, String> {
    let record = file
        .records()
        .into_iter()
        .find(|record| record.id == id)
        .ok_or_else(|| format!("INTF#{id} not found"))?;
    file.decode(record).map_err(|error| error.to_string())
}

fn decode_image(file: &ResFile, id: u16) -> Result<PlanarImage, String> {
    decode_planar(&decode_record(file, id)?).map_err(|error| format!("INTF#{id}: {error}"))
}

/// Map icon records hold two 48-pixel-wide variants side by side. The game
/// draws the first one for ordinary locations.
fn icon_frame_width(icon: &PlanarImage) -> i32 {
    i32::from(icon.width).min(48)
}

fn outline(frame: &mut IndexedFrame, x: i32, y: i32, width: i32, height: u16, color: u8) {
    let left = VIEW_X + x - 1;
    let top = VIEW_Y + y - 1;
    let right = VIEW_X + x + width;
    let bottom = VIEW_Y + y + i32::from(height);
    for px in left..=right {
        for py in [top, bottom] {
            if (VIEW_X..VIEW_X + VIEW_W).contains(&px) && (VIEW_Y..VIEW_Y + VIEW_H).contains(&py) {
                frame.set_pixel(px as u16, py as u16, color);
            }
        }
    }
    for py in top..=bottom {
        for px in [left, right] {
            if (VIEW_X..VIEW_X + VIEW_W).contains(&px) && (VIEW_Y..VIEW_Y + VIEW_H).contains(&py) {
                frame.set_pixel(px as u16, py as u16, color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_candidates_follow_the_loader() {
        assert_eq!(scene_candidates(0, 1), [101, 1]);
        assert_eq!(scene_candidates(5, 1), [106, 6]);
        assert_eq!(scene_candidates(18, 1), [119, 19]);
    }

    #[test]
    fn resolves_region_then_fallback_then_generic() {
        let available = [1u16, 102, 106, 119, 901, 903];
        let has = |id: u16| available.contains(&id);
        // Colchester: 101 is absent, so the region-0 scene 1 wins.
        assert_eq!(resolve_scene_id(0, 605, 1, has), Some(1));
        // Gloucester/London: the region-1 scene exists.
        assert_eq!(resolve_scene_id(1, 604, 1, has), Some(102));
        assert_eq!(resolve_scene_id(5, 604, 1, has), Some(106));
        // A location whose region and fallback record are absent uses its icon type.
        assert_eq!(resolve_scene_id(30, 605, 1, has), Some(903));
        assert_eq!(resolve_scene_id(14, 600, 1, has), Some(901));
    }

    #[test]
    fn generic_scenes_match_the_documented_types() {
        assert_eq!(generic_scene_for_icon(600), Some(901));
        assert_eq!(generic_scene_for_icon(604), Some(902));
        assert_eq!(generic_scene_for_icon(607), Some(905));
        assert_eq!(generic_scene_for_icon(614), Some(909));
        assert_eq!(generic_scene_for_icon(99), None);
    }

    #[test]
    fn camera_is_clamped_and_tile_aligned() {
        assert_eq!(camera_for(635, 665), (480, 576));
        assert_eq!(camera_for(0, 0), (0, 0));
        assert_eq!(camera_for(2000, 2000), (880, 800));
    }

    #[test]
    fn nearest_location_uses_world_distance() {
        let markers = vec![
            LocationMarker {
                location_index: 0,
                name: "A".into(),
                world_x: 100,
                world_y: 100,
                icon_id: 605,
            },
            LocationMarker {
                location_index: 1,
                name: "B".into(),
                world_x: 500,
                world_y: 500,
                icon_id: 605,
            },
        ];
        assert_eq!(nearest_location(&markers, 480, 520), Some(1));
        assert_eq!(nearest_location(&markers, 0, 0), Some(0));
        assert_eq!(nearest_location(&[], 0, 0), None);
    }
}
