//! Integrated headless session: save state, resources, room rendering and
//! palettes. This is the seam the eventual game loop drives.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use soe_assets::Palette;
use soe_res::ResFile;
use soe_video::{IndexedFrame, decode_planar};

use crate::ani::FrameBank;
use crate::font::BitmapFont;
use crate::palette::PaletteSet;
use crate::window::WindowStyle;
use crate::resources::{ResourceError, ResourceManager};
use crate::room::compose_scene;
use crate::save::GameState;
use crate::scene::Scene;

#[derive(Debug)]
pub enum SessionError {
    Io(std::io::Error),
    Res(String),
    Resource(ResourceError),
    File(crate::files::FileError),
    State(String),
    Scene(String),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::Res(error) => write!(f, "resource error: {error}"),
            Self::Resource(error) => write!(f, "{error}"),
            Self::File(error) => write!(f, "{error}"),
            Self::State(error) => write!(f, "save error: {error}"),
            Self::Scene(error) => write!(f, "scene error: {error}"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<std::io::Error> for SessionError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<ResourceError> for SessionError {
    fn from(error: ResourceError) -> Self {
        Self::Resource(error)
    }
}
impl From<crate::files::FileError> for SessionError {
    fn from(error: crate::files::FileError) -> Self {
        Self::File(error)
    }
}

/// A loaded episode plus its resource archives.
pub struct Session {
    game_dir: PathBuf,
    resources: ResourceManager,
    palettes: PaletteSet,
    /// Palette left active by the room renderer for the current scene.
    scene_palette: Palette,
    pub state: GameState,
    pub scene_id: u16,
    pub scene: Scene,
    pub frame: IndexedFrame,
    font: Option<BitmapFont>,
    window_style: Option<WindowStyle>,
    /// EXE DGROUP image (initialised data) for the ported map code.
    dgroup: Option<Vec<u8>>,
    static_images: std::collections::HashMap<u16, soe_video::PlanarImage>,
    animations: std::collections::HashMap<u16, Vec<soe_video::PlanarImage>>,
    animation_frame: usize,
}

impl Session {
    /// Empty session for headless state-machine tests. It has no resources,
    /// no font and an all-zero save state.
    #[doc(hidden)]
    #[must_use]
    pub fn headless() -> Self {
        let state = GameState::parse(&[0u8; crate::save::FILE_SIZE]).expect("empty save state");
        Self {
            game_dir: PathBuf::new(),
            resources: ResourceManager::new(),
            palettes: PaletteSet::black(),
            scene_palette: Palette::black(),
            state,
            scene_id: 0,
            scene: Scene {
                background: 0,
                objects: Vec::new(),
                trailer: 0,
            },
            frame: IndexedFrame::black(),
            font: None,
            window_style: None,
            dgroup: None,
            static_images: std::collections::HashMap::new(),
            animations: std::collections::HashMap::new(),
            animation_frame: 0,
        }
    }

    /// Load `EPISODE1.DAT`, `SCENES.RES`, all `RES32/*.RES`, and palettes from
    /// the unpacked executable (falling back to black if unavailable).
    pub fn load(
        game_dir: impl AsRef<Path>,
        exe: impl AsRef<Path>,
        scene_id: u16,
    ) -> Result<Self, SessionError> {
        Self::load_save(game_dir, exe, scene_id, "EPISODE1.DAT")
    }

    /// Load an arbitrary `.sav`/episode file instead of `EPISODE1.DAT`.
    pub fn load_save(
        game_dir: impl AsRef<Path>,
        exe: impl AsRef<Path>,
        scene_id: u16,
        save_name: &str,
    ) -> Result<Self, SessionError> {
        let game_dir = game_dir.as_ref().to_path_buf();
        let state_bytes = std::fs::read(game_dir.join(save_name))?;
        let state = GameState::parse(&state_bytes).map_err(SessionError::State)?;

        let mut resources = ResourceManager::new();
        let res_dir = game_dir.join("RES32");
        for entry in std::fs::read_dir(&res_dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.to_ascii_uppercase().ends_with(".RES") {
                resources.load_from_dir(&res_dir, &name)?;
            }
        }

        let palettes = PaletteSet::load(&exe).unwrap_or_else(|_| PaletteSet::black());
        let font = load_font(&mut resources, exe.as_ref());
        let exe_bytes = std::fs::read(exe.as_ref()).ok();
        let window_style = exe_bytes.as_deref().and_then(WindowStyle::from_exe);
        let dgroup = exe_bytes.as_deref().and_then(crate::mapsim::dgroup_from_exe);

        let mut session = Self {
            game_dir,
            resources,
            scene_palette: palettes.room.clone(),
            palettes,
            state,
            scene_id,
            scene: Scene {
                background: 0,
                objects: Vec::new(),
                trailer: 0,
            },
            frame: IndexedFrame::black(),
            font,
            window_style,
            dgroup,
            static_images: std::collections::HashMap::new(),
            animations: std::collections::HashMap::new(),
            animation_frame: 0,
        };
        session.enter_scene(scene_id)?;
        Ok(session)
    }

    /// Load and render a scene by logical id.
    pub fn enter_scene(&mut self, scene_id: u16) -> Result<(), SessionError> {
        let scenes_path = self.game_dir.join("SCENES.RES");
        let file = ResFile::parse(std::fs::read(scenes_path)?)
            .map_err(|e| SessionError::Res(e.to_string()))?;
        let record = file
            .records()
            .into_iter()
            .find(|record| record.id == scene_id)
            .ok_or_else(|| SessionError::Scene(format!("scene {scene_id} not found")))?;
        let data = file
            .decode(record)
            .map_err(|e| SessionError::Res(e.to_string()))?;
        let scene = Scene::parse(&data).map_err(SessionError::Scene)?;

        let mut static_images = std::collections::HashMap::new();
        let mut animations = std::collections::HashMap::new();
        for object in &scene.objects {
            let id = object.resource_id;
            if static_images.contains_key(&id) || animations.contains_key(&id) {
                continue;
            }
            let Some(name) = self.resources.find_file(id).map(str::to_owned) else {
                continue;
            };
            let data = self.resources.record(&name, id)?;
            if name.eq_ignore_ascii_case("ANI.RES") {
                if let Ok(bank) = FrameBank::parse(data) {
                    let frames: Vec<_> = (0..bank.frames.len())
                        .filter_map(|index| bank.frame_image(index))
                        .collect();
                    if !frames.is_empty() {
                        animations.insert(id, frames);
                    }
                }
            } else if let Ok(image) = decode_planar(data) {
                static_images.insert(id, image);
            }
        }

        self.scene_palette = self
            .palettes
            .scene_palette(scene.objects.iter().map(|object| object.resource_id));
        self.scene = scene;
        self.scene_id = scene_id;
        self.static_images = static_images;
        self.animations = animations;
        self.animation_frame = 0;
        self.recompose();
        Ok(())
    }

    /// Re-render the current scene using the current animation frame.
    fn recompose(&mut self) {
        let animation_frame = self.animation_frame;
        let frame = compose_scene(&self.scene, &self.scene_palette, |id| {
            self.static_images.get(&id).cloned().or_else(|| {
                self.animations
                    .get(&id)
                    .and_then(|frames| frames.get(animation_frame % frames.len()))
                    .cloned()
            })
        });
        self.frame = frame;
    }

    /// The current scene with the duel's widgets drawn over it, back to
    /// front: each sprite is frame `frame` of its `ANI.RES` bank with its
    /// top-left at the widget position, mirrored when the widget is, and
    /// recoloured through its remap table.
    #[must_use]
    pub fn compose_duel(&self, duel: &crate::duel::Duel) -> soe_video::IndexedFrame {
        let mut frame = self.frame.clone();
        for sprite in duel.sprites() {
            let image = if sprite.still {
                duel.still(sprite.resource).and_then(|data| decode_planar(data).ok())
            } else {
                duel.bank(sprite.resource)
                    .and_then(|data| FrameBank::parse(data).ok())
                    .and_then(|bank| bank.frame_image(usize::from(sprite.frame & 0x7f)))
            };
            let Some(image) = image else {
                continue;
            };
            let remap = duel.remap_table(sprite.remap);
            crate::room::blit_remapped(
                &mut frame,
                &image,
                i32::from(sprite.x),
                i32::from(sprite.y),
                sprite.mirrored,
                remap.as_ref(),
            );
        }
        frame
    }

    /// The army battle's field (image at (0x90, 0x50), as `FUN_20b5_085b`
    /// draws it) and its tokens over `frame`. The battle keeps the regional
    /// map's palette (the original's DAC is unchanged from the map).
    #[must_use]
    pub fn compose_army(&self, mut frame: soe_video::IndexedFrame, battle: &crate::army::ArmyBattle) -> soe_video::IndexedFrame {
        frame.palette = self.palettes.regional_map();
        if let Some(field) = battle.field_image().and_then(|d| decode_planar(d).ok()) {
            crate::room::blit_remapped(&mut frame, &field, 0x90, 0x50, false, None);
        }
        for sprite in battle.sprites() {
            let image = battle
                .bank(sprite.resource)
                .and_then(|data| FrameBank::parse(data).ok())
                .and_then(|bank| bank.frame_image(usize::from(sprite.frame & 0x7f)));
            if let Some(image) = image {
                crate::room::blit_remapped(&mut frame, &image, i32::from(sprite.x), i32::from(sprite.y), sprite.mirrored, None);
            }
        }
        frame
    }

    /// Advance every animation by one frame and re-render.
    pub fn advance_animation_frame(&mut self) {
        self.animation_frame = self.animation_frame.wrapping_add(1);
        self.recompose();
    }

    /// Reset animations to their first frame and re-render.
    pub fn reset_animation_frame(&mut self) {
        self.animation_frame = 0;
        self.recompose();
    }

    /// Number of loaded animation resources in the current scene.
    #[must_use]
    pub fn animation_count(&self) -> usize {
        self.animations.len()
    }

    /// Write the current state as an episode/save file in `dir`.
    pub fn save_to(&self, dir: impl AsRef<Path>, name: &str) -> Result<(), SessionError> {
        use crate::files::GameFiles;
        let mut files = crate::files::CaseInsensitiveDir::new(dir.as_ref());
        files.write(name, &self.state.to_bytes())?;
        Ok(())
    }

    /// The EXE's DGROUP image, when the EXE was readable.
    #[must_use]
    pub fn dgroup(&self) -> Option<&[u8]> {
        self.dgroup.as_deref()
    }

    /// Dialog window tables (`DS:5D62`, `DS:1F4D`), when the EXE was readable.
    #[must_use]
    pub fn window_style(&self) -> Option<&WindowStyle> {
        self.window_style.as_ref()
    }

    #[must_use]
    pub fn font(&self) -> Option<&BitmapFont> {
        self.font.as_ref()
    }

    /// Draw text onto the current room frame. Returns false when the font is
    /// unavailable.
    pub fn draw_text(&mut self, x: i32, y: i32, text: &str, color: Option<u8>) -> bool {
        let Some(font) = self.font.as_ref() else {
            return false;
        };
        font.draw_text(&mut self.frame, x, y, text, color);
        true
    }

    /// Topmost scene object whose opaque pixel covers `(x, y)`.
    ///
    /// Draw order is the scene object order, so the last match wins. This is
    /// the hit test used to select room characters; animation objects use
    /// their current frame.
    #[must_use]
    pub fn object_at(&self, x: i16, y: i16) -> Option<usize> {
        let px = i32::from(x);
        let py = i32::from(y);
        for (index, object) in self.scene.objects.iter().enumerate().rev() {
            let image = self.static_images.get(&object.resource_id).or_else(|| {
                self.animations
                    .get(&object.resource_id)
                    .and_then(|frames| frames.get(self.animation_frame % frames.len()))
            });
            let Some(image) = image else {
                continue;
            };
            let width = i32::from(image.width);
            let sx = px - i32::from(object.x);
            let sy = py - i32::from(object.y);
            if sx < 0 || sy < 0 || sx >= width || sy >= i32::from(image.height) {
                continue;
            }
            let source_x = if object.flags & 1 != 0 {
                width - 1 - sx
            } else {
                sx
            };
            if image.indices[(sy * width + source_x) as usize] != 0 {
                return Some(index);
            }
        }
        None
    }

    #[must_use]
    pub fn palettes(&self) -> &PaletteSet {
        &self.palettes
    }

    /// Palette for presenting the current room (see `PaletteSet::scene_palette`).
    #[must_use]
    pub fn scene_palette(&self) -> &Palette {
        &self.scene_palette
    }

    pub fn resources_mut(&mut self) -> &mut ResourceManager {
        &mut self.resources
    }
}

fn load_font(resources: &mut ResourceManager, exe: &Path) -> Option<BitmapFont> {
    let exe_bytes = std::fs::read(exe).ok()?;
    let (_, spacing, metrics) = BitmapFont::metrics_from_excal(&exe_bytes)?;
    let data = resources.record("TITLE.RES", 200).ok()?;
    let atlas = decode_planar(data).ok()?;
    BitmapFont::new(atlas, spacing, &metrics).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_episode_and_renders_scene_one() {
        let game = Path::new("../../game");
        let exe = Path::new("../../ghidra/EXCAL_UNPACKED.EXE");
        if !game.join("EPISODE1.DAT").exists() || !game.join("RES32").exists() {
            eprintln!("SKIP: game data absent");
            return;
        }
        let mut session = Session::load(game, exe, 1).unwrap();
        assert_eq!(session.state.characters[0].name(), "Constantine");
        assert_eq!(session.scene.objects.len(), 16);
        assert_eq!((session.frame.width, session.frame.height), (320, 200));
        assert!(session.frame.pixels.iter().any(|pixel| *pixel != 0));
        assert!(session.font().is_some());
        assert!(session.draw_text(4, 4, "Test", Some(3)));
    }

    #[test]
    fn loads_example_save_when_present() {
        let game = Path::new("../../game");
        let exe = Path::new("../../ghidra/EXCAL_UNPACKED.EXE");
        if !game.join("example.sav").exists() || !game.join("RES32").exists() {
            eprintln!("SKIP: example.sav or game data absent");
            return;
        }
        let session = Session::load_save(game, exe, 5, "example.sav").unwrap();
        assert_eq!(session.scene_id, 5);
        assert_eq!(
            session.state.to_bytes(),
            std::fs::read(game.join("example.sav")).unwrap()
        );
    }

    #[test]
    fn animation_frames_cycle_in_scene_five() {
        let game = Path::new("../../game");
        let exe = Path::new("../../ghidra/EXCAL_UNPACKED.EXE");
        if !game.join("EPISODE1.DAT").exists() || !game.join("RES32").exists() {
            eprintln!("SKIP: game data absent");
            return;
        }
        let mut session = Session::load(game, exe, 5).unwrap();
        assert_eq!(session.animation_count(), 3);
        let first = session.frame.pixels.clone();
        session.advance_animation_frame();
        assert_ne!(first, session.frame.pixels);
        session.reset_animation_frame();
        assert_eq!(first, session.frame.pixels);
    }

    #[test]
    fn saves_a_loadable_state() {
        let game = Path::new("../../game");
        let exe = Path::new("../../ghidra/EXCAL_UNPACKED.EXE");
        if !game.join("EPISODE1.DAT").exists() {
            eprintln!("SKIP: game data absent");
            return;
        }
        let session = Session::load(game, exe, 1).unwrap();
        let dir = std::env::temp_dir().join(format!("soe-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        session.save_to(&dir, "slot1.sav").unwrap();
        let data = std::fs::read(dir.join("slot1.sav")).unwrap();
        let restored = GameState::parse(&data).unwrap();
        assert_eq!(restored.characters[0].name(), "Constantine");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn every_scene_composes_without_error() {
        let game = Path::new("../../game");
        let exe = Path::new("../../ghidra/EXCAL_UNPACKED.EXE");
        if !game.join("EPISODE1.DAT").exists() || !game.join("RES32").exists() {
            eprintln!("SKIP: game data absent");
            return;
        }
        let mut session = Session::load(game, exe, 1).unwrap();
        let scenes = ResFile::parse(std::fs::read(game.join("SCENES.RES")).unwrap()).unwrap();
        let ids: Vec<u16> = scenes.records().iter().map(|record| record.id).collect();
        assert_eq!(ids.len(), 138);
        for id in ids {
            session.enter_scene(id).unwrap();
            assert_eq!((session.frame.width, session.frame.height), (320, 200));
        }
    }
}
