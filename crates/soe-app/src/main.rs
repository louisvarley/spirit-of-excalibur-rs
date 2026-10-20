//! Playable room viewer for the original assets.
//!
//! This is deliberately small: it loads `EPISODE1.DAT`, the region archives,
//! palettes and font, renders a room with animated objects, and lets the
//! player walk through the 138 scene records. The full game state machine and
//! UI are still being ported.

#![forbid(unsafe_code)]

use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use soe_assets::Palette;
use soe_engine::session::Session;
use soe_video::{IndexedFrame, decode_planar};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

const TICK: Duration = Duration::from_micros(54_945); // 18.2 Hz

struct App {
    window: Option<Arc<Window>>,
    context: Option<softbuffer::Context<Arc<Window>>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    session: Session,
    scene_ids: Vec<u16>,
    scene_index: usize,
    animation_accum: Duration,
    last_tick: Instant,
    cursor: (f64, f64),
    map: Option<IndexedFrame>,
    map_mode: bool,
}

impl App {
    fn new(
        session: Session,
        scene_ids: Vec<u16>,
        scene_index: usize,
        map: Option<IndexedFrame>,
        start_map: bool,
    ) -> Self {
        Self {
            window: None,
            context: None,
            surface: None,
            session,
            scene_ids,
            scene_index,
            animation_accum: Duration::ZERO,
            last_tick: Instant::now(),
            cursor: (0.0, 0.0),
            map,
            map_mode: start_map,
        }
    }

    fn change_scene(&mut self, delta: isize) {
        if self.scene_ids.is_empty() {
            return;
        }
        let len = self.scene_ids.len() as isize;
        let next = (self.scene_index as isize + delta).rem_euclid(len) as usize;
        let id = self.scene_ids[next];
        if let Err(error) = self.session.enter_scene(id) {
            eprintln!("scene {id}: {error}");
            return;
        }
        self.scene_index = next;
        self.animation_accum = Duration::ZERO;
        if let Some(window) = &self.window {
            window.set_title(&format!(
                "Spirit of Excalibur - scene {} ({}/{})",
                id,
                next + 1,
                self.scene_ids.len()
            ));
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        let Some(surface) = &mut self.surface else {
            return;
        };
        if let (Some(w), Some(h)) = (NonZeroU32::new(width), NonZeroU32::new(height)) {
            let _ = surface.resize(w, h);
        }
    }

    fn redraw(&mut self) {
        let Some(window) = &self.window else {
            return;
        };
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.resize(size.width, size.height);

        let id = self.session.scene_id;
        let mut map_copy = if self.map_mode {
            self.map.clone()
        } else {
            None
        };
        if let Some(map) = map_copy.as_mut() {
            fill_rect(map, 0, 0, 320, 13, 0);
            if let Some(font) = self.session.font() {
                font.draw_text(
                    map,
                    2,
                    2,
                    "MAP  M ROOM  ARROWS SCENE  SPACE FRAME  ESC QUIT",
                    Some(7),
                );
            }
        } else {
            fill_rect(&mut self.session.frame, 0, 0, 320, 13, 0);
            fill_rect(&mut self.session.frame, 0, 188, 320, 12, 0);
            self.session.draw_text(
                2,
                2,
                &format!("SCENE {id}  M MAP  ARROWS SCENE  SPACE FRAME"),
                Some(7),
            );
            self.session.draw_text(2, 190, "R RESET  ESC QUIT", Some(7));
        }

        let Some(surface) = &mut self.surface else {
            return;
        };
        let Ok(mut buffer) = surface.buffer_mut() else {
            return;
        };
        let out_w = size.width as usize;
        let out_h = size.height as usize;
        let src = map_copy.as_ref().unwrap_or(&self.session.frame);
        let palette = &self.session.palettes().room.colors;
        let sw = usize::from(src.width);
        let sh = usize::from(src.height);
        for y in 0..out_h {
            let sy = y * sh / out_h;
            for x in 0..out_w {
                let sx = x * sw / out_w;
                let index = usize::from(src.pixels[sy * sw + sx]);
                let [r, g, b] = palette.get(index).copied().unwrap_or([0, 0, 0]);
                buffer[y * out_w + x] = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
            }
        }
        let _ = buffer.present();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title(format!(
                "Spirit of Excalibur - scene {}",
                self.session.scene_id
            ))
            .with_inner_size(winit::dpi::PhysicalSize::new(960, 600))
            .with_resizable(true);
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                eprintln!("could not create window: {error}");
                event_loop.exit();
                return;
            }
        };
        let context = match softbuffer::Context::new(window.clone()) {
            Ok(context) => context,
            Err(error) => {
                eprintln!("could not create graphics context: {error}");
                event_loop.exit();
                return;
            }
        };
        let surface = match softbuffer::Surface::new(&context, window.clone()) {
            Ok(surface) => surface,
            Err(error) => {
                eprintln!("could not create window surface: {error}");
                event_loop.exit();
                return;
            }
        };
        self.window = Some(window);
        self.context = Some(context);
        self.surface = Some(surface);
        self.last_tick = Instant::now();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self.window.as_ref().is_some_and(|w| w.id() != window_id) {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => self.resize(size.width, size.height),
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x, position.y);
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                let width = self
                    .window
                    .as_ref()
                    .map_or(1.0, |window| f64::from(window.inner_size().width));
                self.change_scene(if self.cursor.0 >= width / 2.0 { 1 } else { -1 });
            }
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed && !event.repeat =>
            {
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::Escape) => event_loop.exit(),
                    PhysicalKey::Code(KeyCode::ArrowLeft) => self.change_scene(-1),
                    PhysicalKey::Code(KeyCode::ArrowRight) => self.change_scene(1),
                    PhysicalKey::Code(KeyCode::Space) => {
                        self.session.advance_animation_frame();
                        self.animation_accum = Duration::ZERO;
                    }
                    PhysicalKey::Code(KeyCode::KeyR) => {
                        self.session.reset_animation_frame();
                        self.animation_accum = Duration::ZERO;
                    }
                    PhysicalKey::Code(KeyCode::KeyM) => {
                        self.map_mode = !self.map_mode;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let delta = now.saturating_duration_since(self.last_tick);
        self.last_tick = now;
        if self.session.animation_count() > 0 {
            self.animation_accum += delta;
            while self.animation_accum >= TICK {
                self.session.advance_animation_frame();
                self.animation_accum -= TICK;
            }
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(16),
        ));
    }
}

fn fill_rect(frame: &mut IndexedFrame, x: u16, y: u16, width: u16, height: u16, color: u8) {
    for py in y..y.saturating_add(height) {
        for px in x..x.saturating_add(width) {
            frame.set_pixel(px, py, color);
        }
    }
}

fn load_record_image(path: &Path, id: u16) -> Result<soe_video::PlanarImage> {
    let file = soe_res::ResFile::parse(
        std::fs::read(path).with_context(|| format!("reading {}", path.display()))?,
    )
    .with_context(|| format!("parsing {}", path.display()))?;
    let record = file
        .records()
        .into_iter()
        .find(|record| record.id == id)
        .with_context(|| format!("record {id} not found in {}", path.display()))?;
    decode_planar(&file.decode(record)?)
        .with_context(|| format!("decoding record {id} from {}", path.display()))
}

fn load_map(game_dir: &Path, palette: &Palette) -> Result<Option<IndexedFrame>> {
    let res32 = game_dir.join("RES32");
    let background = res32.join("ENGLAND.RES");
    let frame_path = res32.join("INTF.RES");
    if !background.exists() || !frame_path.exists() {
        return Ok(None);
    }
    let background = load_record_image(&background, 510)?;
    let frame = load_record_image(&frame_path, 501)?;
    if background.width != 320
        || background.height != 200
        || frame.width != 320
        || frame.height != 200
    {
        return Ok(None);
    }
    let mut out = IndexedFrame::new(320, 200, background.indices, palette.clone())?;
    for y in 0..200u16 {
        for x in 0..320u16 {
            let pixel = frame.indices[usize::from(y) * 320 + usize::from(x)];
            if pixel != 0 {
                out.set_pixel(x, y, pixel);
            }
        }
    }
    Ok(Some(out))
}

fn main() -> Result<()> {
    let game_dir = std::env::var_os("SOE_GAME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("game"));
    let exe = std::env::var_os("SOE_EXE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ghidra/EXCAL_UNPACKED.EXE"));

    let scenes = soe_res::ResFile::parse(std::fs::read(game_dir.join("SCENES.RES"))?)
        .context("parsing SCENES.RES")?;
    let mut scene_ids: Vec<u16> = scenes.records().iter().map(|record| record.id).collect();
    scene_ids.sort_unstable();
    scene_ids.dedup();
    if scene_ids.is_empty() {
        anyhow::bail!("SCENES.RES contains no scene records");
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let start_map = args.iter().any(|arg| arg == "--map");
    let requested = args
        .windows(2)
        .find(|pair| pair[0] == "--scene")
        .and_then(|pair| pair[1].parse::<u16>().ok());
    let start = requested
        .and_then(|id| scene_ids.iter().position(|candidate| *candidate == id))
        .or_else(|| scene_ids.iter().position(|id| *id == 5))
        .unwrap_or(0);
    let session = Session::load(&game_dir, &exe, scene_ids[start])?;
    let map = load_map(&game_dir, &session.palettes().room)?;

    let event_loop = EventLoop::new().context("creating event loop")?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::new(session, scene_ids, start, map, start_map);
    event_loop.run_app(&mut app).context("running window")?;
    Ok(())
}
