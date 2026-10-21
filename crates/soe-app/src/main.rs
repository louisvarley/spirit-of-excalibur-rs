//! Playable room viewer for the original assets.
//!
//! This is deliberately small: it loads `EPISODE1.DAT`, the region archives,
//! palettes and font, renders a room with animated objects, and lets the
//! player walk through the 138 scene records. The full game state machine and
//! UI are still being ported.

#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use soe_assets::Palette;
use soe_engine::session::Session;
use soe_engine::story::HintBook;
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
    game_dir: PathBuf,
    exe: PathBuf,
    scene_ids: Vec<u16>,
    scene_index: usize,
    animation_accum: Duration,
    last_tick: Instant,
    cursor: (f64, f64),
    map: Option<IndexedFrame>,
    panel: Option<soe_video::PlanarImage>,
    story: Option<HintBook>,
    map_mode: bool,
    status_mode: bool,
    story_mode: bool,
    paused: bool,
    story_page: usize,
    travel_mode: bool,
    travel_index: usize,
}

impl App {
    fn new(
        session: Session,
        game_dir: PathBuf,
        exe: PathBuf,
        scene_ids: Vec<u16>,
        scene_index: usize,
        map: Option<IndexedFrame>,
        panel: Option<soe_video::PlanarImage>,
        story: Option<HintBook>,
        start_map: bool,
        start_status: bool,
        start_story: bool,
    ) -> Self {
        Self {
            window: None,
            context: None,
            surface: None,
            session,
            game_dir,
            exe,
            scene_ids,
            scene_index,
            animation_accum: Duration::ZERO,
            last_tick: Instant::now(),
            cursor: (0.0, 0.0),
            map,
            panel,
            story,
            map_mode: start_map,
            status_mode: start_status,
            story_mode: start_story,
            paused: false,
            story_page: 0,
            travel_mode: false,
            travel_index: 0,
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
        self.update_title();
    }

    fn travel_to_selected(&mut self) {
        let locations: Vec<_> = self
            .session
            .state
            .locations
            .iter()
            .filter(|location| !location.name().is_empty())
            .collect();
        let Some(location) =
            locations.get(self.travel_index.min(locations.len().saturating_sub(1)))
        else {
            return;
        };
        let Some(scene_id) = location.scene_id() else {
            return;
        };
        if let Err(error) = self.session.enter_scene(scene_id) {
            eprintln!("travel to scene {scene_id}: {error}");
            return;
        }
        self.travel_mode = false;
        self.update_title();
    }

    fn update_title(&self) {
        let Some(window) = &self.window else {
            return;
        };
        let mode = if self.travel_mode {
            "TRAVEL".to_string()
        } else if self.story_mode {
            "CHRONICLE".to_string()
        } else if self.paused {
            "PAUSED".to_string()
        } else if self.status_mode {
            "STATUS".to_string()
        } else if self.map_mode {
            "MAP".to_string()
        } else {
            format!(
                "SCENE {} ({}/{})",
                self.session.scene_id,
                self.scene_index + 1,
                self.scene_ids.len()
            )
        };
        window.set_title(&format!(
            "Spirit of Excalibur - {mode}  |  arrows/click scene  M map  I status  T travel  H chronicle  P pause  F5 save  F9 load  Esc quit"
        ));
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

        let mut display = if self.map_mode {
            self.map.clone()
        } else {
            None
        };
        if self.status_mode {
            let mut copy = display.take().unwrap_or_else(|| self.session.frame.clone());
            draw_status(&mut copy, &self.session);
            display = Some(copy);
        }
        if self.paused {
            let mut copy = display.take().unwrap_or_else(|| self.session.frame.clone());
            if let Some(font) = self.session.font() {
                if let Some(panel) = self.panel.as_ref() {
                    draw_panel(&mut copy, panel, 0, 72, 320, 56);
                }
                font.draw_text(&mut copy, 14, 82, "THE GAME IS PAUSED.", Some(0));
                font.draw_text(&mut copy, 14, 96, "TO CONTINUE, PRESS A KEY.", Some(0));
            }
            display = Some(copy);
        }
        if self.story_mode {
            let mut copy = display.take().unwrap_or_else(|| self.session.frame.clone());
            draw_story(
                &mut copy,
                &self.session,
                self.story.as_ref(),
                self.panel.as_ref(),
                self.story_page,
            );
            display = Some(copy);
        }
        if self.travel_mode {
            let mut copy = display.take().unwrap_or_else(|| self.session.frame.clone());
            draw_travel(&mut copy, &self.session, self.travel_index);
            display = Some(copy);
        }

        let Some(surface) = &mut self.surface else {
            return;
        };
        let Ok(mut buffer) = surface.buffer_mut() else {
            return;
        };
        let out_w = size.width as usize;
        let out_h = size.height as usize;
        let src = display.as_ref().unwrap_or(&self.session.frame);
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
            .with_title("Spirit of Excalibur")
            .with_inner_size(winit::dpi::PhysicalSize::new(640, 400))
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
        self.update_title();
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
                if self.travel_mode {
                    if self.cursor.0 >= width / 2.0 {
                        self.travel_index = self.travel_index.saturating_add(1);
                    } else {
                        self.travel_index = self.travel_index.saturating_sub(1);
                    }
                } else if self.story_mode {
                    if self.cursor.0 >= width / 2.0 {
                        self.story_page = self.story_page.wrapping_add(1);
                    } else {
                        self.story_page = self.story_page.saturating_sub(1);
                    }
                } else {
                    self.change_scene(if self.cursor.0 >= width / 2.0 { 1 } else { -1 });
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed && !event.repeat =>
            {
                if self.travel_mode {
                    match event.physical_key {
                        PhysicalKey::Code(KeyCode::Escape) => event_loop.exit(),
                        PhysicalKey::Code(KeyCode::KeyT) => {
                            self.travel_mode = false;
                            self.update_title();
                        }
                        PhysicalKey::Code(KeyCode::ArrowUp) => {
                            self.travel_index = self.travel_index.saturating_sub(1);
                        }
                        PhysicalKey::Code(KeyCode::ArrowDown) => {
                            self.travel_index = self.travel_index.saturating_add(1);
                        }
                        PhysicalKey::Code(KeyCode::Enter) => self.travel_to_selected(),
                        _ => {}
                    }
                    return;
                }
                if self.story_mode {
                    match event.physical_key {
                        PhysicalKey::Code(KeyCode::Escape) => event_loop.exit(),
                        PhysicalKey::Code(KeyCode::KeyH) => {
                            self.story_mode = false;
                            self.update_title();
                        }
                        PhysicalKey::Code(KeyCode::Space)
                        | PhysicalKey::Code(KeyCode::ArrowRight) => {
                            self.story_page = self.story_page.wrapping_add(1);
                        }
                        PhysicalKey::Code(KeyCode::ArrowLeft) => {
                            self.story_page = self.story_page.saturating_sub(1);
                        }
                        _ => {}
                    }
                    return;
                }
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
                        self.update_title();
                    }
                    PhysicalKey::Code(KeyCode::KeyI) => {
                        self.status_mode = !self.status_mode;
                        self.update_title();
                    }
                    PhysicalKey::Code(KeyCode::KeyP) => {
                        self.paused = !self.paused;
                        self.update_title();
                    }
                    PhysicalKey::Code(KeyCode::KeyH) => {
                        self.story_mode = true;
                        self.update_title();
                    }
                    PhysicalKey::Code(KeyCode::KeyT) => {
                        self.travel_mode = true;
                        self.update_title();
                    }
                    PhysicalKey::Code(KeyCode::F5) => {
                        if let Err(error) = self.session.save_to(&self.game_dir, "QUICK.SAV") {
                            eprintln!("quick save: {error}");
                        }
                    }
                    PhysicalKey::Code(KeyCode::F9) => {
                        match Session::load_save(
                            &self.game_dir,
                            &self.exe,
                            self.session.scene_id,
                            "QUICK.SAV",
                        ) {
                            Ok(session) => {
                                self.session = session;
                                self.update_title();
                            }
                            Err(error) => eprintln!("quick load: {error}"),
                        }
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
        if self.session.animation_count() > 0
            && !self.paused
            && !self.story_mode
            && !self.travel_mode
        {
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

fn wrap_text(font: &soe_engine::font::BitmapFont, text: &str, max_width: u32) -> Vec<String> {
    let mut lines = Vec::new();
    for raw in text.split('\n') {
        let mut line = String::new();
        for word in raw.split_whitespace() {
            let candidate = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };
            if !line.is_empty() && font.text_width(&candidate) > max_width {
                lines.push(std::mem::take(&mut line));
                line.push_str(word);
            } else {
                line = candidate;
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
    }
    lines
}

fn draw_story(
    frame: &mut IndexedFrame,
    session: &Session,
    story: Option<&HintBook>,
    panel: Option<&soe_video::PlanarImage>,
    page: usize,
) {
    if let Some(panel) = panel {
        draw_panel(frame, panel, 0, 18, 320, 170);
    } else {
        fill_rect(frame, 0, 18, 320, 170, 0);
    }
    let Some(font) = session.font() else {
        return;
    };
    let color = if panel.is_some() { 0 } else { 7 };
    font.draw_text(
        frame,
        12,
        24,
        "CHRONICLE  H CLOSE  ARROWS PAGE",
        Some(color),
    );
    let Some(story) = story else {
        font.draw_text(frame, 12, 42, "NO HINT DATA", Some(color));
        return;
    };
    if story.is_empty() {
        font.draw_text(frame, 12, 42, "NO HINT DATA", Some(color));
        return;
    }
    let index = page % story.paragraphs.len();
    let lines = wrap_text(font, &story.paragraphs[index], 292);
    for (line, text) in lines.iter().take(12).enumerate() {
        font.draw_text(frame, 12, 42 + line as i32 * 10, text, Some(color));
    }
    font.draw_text(
        frame,
        270,
        174,
        &format!("{}/{}", index + 1, story.paragraphs.len()),
        Some(color),
    );
}

fn draw_travel(frame: &mut IndexedFrame, session: &Session, selected: usize) {
    fill_rect(frame, 0, 0, 320, 200, 0);
    let Some(font) = session.font() else {
        return;
    };
    font.draw_text(frame, 8, 6, "TRAVEL", Some(8));
    font.draw_text(frame, 60, 6, "ENTER GO  T CLOSE  UP/DOWN", Some(8));
    let locations: Vec<_> = session
        .state
        .locations
        .iter()
        .filter(|location| !location.name().is_empty())
        .collect();
    if locations.is_empty() {
        font.draw_text(frame, 8, 24, "NO LOCATIONS", Some(8));
        return;
    }
    let selected = selected.min(locations.len() - 1);
    let page = selected / 13;
    for row in 0..13 {
        let index = page * 13 + row;
        let Some(location) = locations.get(index) else {
            break;
        };
        let marker = if index == selected { "*" } else { " " };
        let scene = location
            .scene_id()
            .map_or_else(|| "--".to_string(), |id| id.to_string());
        let line = format!("{marker}{:<18} {}", location.name(), scene);
        font.draw_text(frame, 8, 20 + row as i32 * 12, &line, Some(8));
    }
    font.draw_text(
        frame,
        220,
        188,
        &format!("PAGE {}/{}", page + 1, locations.len().div_ceil(13)),
        Some(8),
    );
}

fn blit_scaled(
    frame: &mut IndexedFrame,
    src: &soe_video::PlanarImage,
    sx: i32,
    sy: i32,
    sw: i32,
    sh: i32,
    dx: i32,
    dy: i32,
    dw: i32,
    dh: i32,
) {
    if sw <= 0 || sh <= 0 || dw <= 0 || dh <= 0 {
        return;
    }
    let src_w = i32::from(src.width);
    for y in 0..dh {
        let syy = sy + y * sh / dh;
        if !(0..i32::from(src.height)).contains(&syy) {
            continue;
        }
        for x in 0..dw {
            let sxx = sx + x * sw / dw;
            if !(0..src_w).contains(&sxx) {
                continue;
            }
            let pixel = src.indices[(syy * src_w + sxx) as usize];
            frame.set_pixel((dx + x) as u16, (dy + y) as u16, pixel);
        }
    }
}

fn draw_panel(
    frame: &mut IndexedFrame,
    src: &soe_video::PlanarImage,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) {
    let corner = 16
        .min(i32::from(src.width) / 2)
        .min(i32::from(src.height) / 2);
    let sw = i32::from(src.width);
    let sh = i32::from(src.height);
    blit_scaled(
        frame,
        src,
        corner,
        corner,
        sw - 2 * corner,
        sh - 2 * corner,
        x + corner,
        y + corner,
        width - 2 * corner,
        height - 2 * corner,
    );
    blit_scaled(frame, src, 0, 0, corner, corner, x, y, corner, corner);
    blit_scaled(
        frame,
        src,
        sw - corner,
        0,
        corner,
        corner,
        x + width - corner,
        y,
        corner,
        corner,
    );
    blit_scaled(
        frame,
        src,
        0,
        sh - corner,
        corner,
        corner,
        x,
        y + height - corner,
        corner,
        corner,
    );
    blit_scaled(
        frame,
        src,
        sw - corner,
        sh - corner,
        corner,
        corner,
        x + width - corner,
        y + height - corner,
        corner,
        corner,
    );
    blit_scaled(
        frame,
        src,
        corner,
        0,
        sw - 2 * corner,
        corner,
        x + corner,
        y,
        width - 2 * corner,
        corner,
    );
    blit_scaled(
        frame,
        src,
        corner,
        sh - corner,
        sw - 2 * corner,
        corner,
        x + corner,
        y + height - corner,
        width - 2 * corner,
        corner,
    );
    blit_scaled(
        frame,
        src,
        0,
        corner,
        corner,
        sh - 2 * corner,
        x,
        y + corner,
        corner,
        height - 2 * corner,
    );
    blit_scaled(
        frame,
        src,
        sw - corner,
        corner,
        corner,
        sh - 2 * corner,
        x + width - corner,
        y + corner,
        corner,
        height - 2 * corner,
    );
}

fn draw_status(frame: &mut IndexedFrame, session: &Session) {
    fill_rect(frame, 10, 18, 300, 168, 0);
    fill_rect(frame, 10, 18, 300, 1, 7);
    fill_rect(frame, 10, 185, 300, 1, 7);
    fill_rect(frame, 10, 18, 1, 168, 7);
    fill_rect(frame, 309, 18, 1, 168, 7);
    let Some(font) = session.font() else {
        return;
    };
    font.draw_text(frame, 16, 24, "ROUND TABLE", Some(8));
    font.draw_text(frame, 16, 38, "KNIGHTS", Some(8));
    font.draw_text(frame, 170, 38, "ITEMS", Some(8));
    fill_rect(frame, 16, 48, 286, 1, 7);
    for (index, character) in session
        .state
        .characters
        .iter()
        .filter(|character| !character.name().is_empty())
        .take(12)
        .enumerate()
    {
        let name = character.name().to_ascii_uppercase();
        font.draw_text(frame, 16, 52 + index as i32 * 10, &name, Some(8));
    }
    for (index, item) in session
        .state
        .items
        .iter()
        .filter(|item| !item.name().is_empty())
        .take(12)
        .enumerate()
    {
        font.draw_text(
            frame,
            170,
            52 + index as i32 * 10,
            &item.name().to_ascii_uppercase(),
            Some(8),
        );
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
    let start_status = args.iter().any(|arg| arg == "--status");
    let requested = args
        .windows(2)
        .find(|pair| pair[0] == "--scene")
        .and_then(|pair| pair[1].parse::<u16>().ok());
    let start = requested
        .and_then(|id| scene_ids.iter().position(|candidate| *candidate == id))
        .or_else(|| scene_ids.iter().position(|id| *id == 5))
        .unwrap_or(0);
    let save_name = args
        .windows(2)
        .find(|pair| pair[0] == "--save")
        .map_or("EPISODE1.DAT", |pair| pair[1].as_str());
    let session = Session::load_save(&game_dir, &exe, scene_ids[start], save_name)?;
    let map = load_map(&game_dir, &session.palettes().room)?;
    let panel = load_record_image(&game_dir.join("RES32").join("INTF.RES"), 621).ok();
    let story = HintBook::load(game_dir.join("HINT.RES")).ok();
    let start_story = args.iter().any(|arg| arg == "--story");

    let event_loop = EventLoop::new().context("creating event loop")?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let paused = args.iter().any(|arg| arg == "--paused");
    let mut app = App::new(
        session,
        game_dir.clone(),
        exe.clone(),
        scene_ids,
        start,
        map,
        panel,
        story,
        start_map,
        start_status,
        start_story,
    );
    app.paused = paused;
    app.travel_mode = args.iter().any(|arg| arg == "--travel");
    event_loop.run_app(&mut app).context("running window")?;
    Ok(())
}
