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

mod audio;

use anyhow::{Context, Result};
use soe_assets::Palette;
use soe_engine::runtime::{Key, Runtime, RuntimeEvent, Screen};
use soe_engine::session::Session;
use soe_engine::smus::MusicLibrary;
use soe_engine::story::HintBook;
use soe_video::{IndexedFrame, decode_planar};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

struct App {
    window: Option<Arc<Window>>,
    context: Option<softbuffer::Context<Arc<Window>>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    runtime: Runtime,
    last_tick: Instant,
    cursor: (f64, f64),
    audio: Option<audio::AudioPlayer>,
}

impl App {
    fn new(runtime: Runtime) -> Self {
        Self {
            window: None,
            context: None,
            surface: None,
            runtime,
            last_tick: Instant::now(),
            cursor: (0.0, 0.0),
            audio: None,
        }
    }

    fn update_title(&self) {
        let Some(window) = &self.window else {
            return;
        };
        let mode = match self.runtime.screen() {
            Screen::Room => format!(
                "SCENE {} ({}/{})",
                self.runtime.scene_id(),
                self.runtime.scene_index() + 1,
                self.runtime.scene_count()
            ),
            other => other.label().to_string(),
        };
        window.set_title(&format!(
            "Spirit of Excalibur - {mode}  |  arrows/click scene  M map  I status  T travel  S saves  H chronicle  A actions  P pause  Esc quit"
        ));
    }

    fn apply(&mut self, event_loop: &ActiveEventLoop, events: Vec<RuntimeEvent>) {
        for event in events {
            match event {
                RuntimeEvent::Quit => event_loop.exit(),
                RuntimeEvent::MuteToggled => {
                    if let Some(audio) = &self.audio {
                        audio.toggle_mute();
                    }
                }
                RuntimeEvent::Error(error) => eprintln!("{error}"),
                RuntimeEvent::Saved(name) => eprintln!("saved {name}"),
                RuntimeEvent::Loaded(name) => eprintln!("loaded {name}"),
                RuntimeEvent::ScreenChanged(_)
                | RuntimeEvent::SceneChanged(_)
                | RuntimeEvent::GameLost
                | RuntimeEvent::MenuEntered => {}
            }
        }
        self.update_title();
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
        let fade = self.runtime.fade();
        let screen = self.runtime.screen();

        let mut display = if screen == Screen::Map {
            self.runtime.map().cloned()
        } else {
            None
        };
        match screen {
            Screen::Status => {
                let mut copy = display
                    .take()
                    .unwrap_or_else(|| self.runtime.session().frame.clone());
                draw_status(&mut copy, self.runtime.session());
                display = Some(copy);
            }
            Screen::Pause => {
                let mut copy = display
                    .take()
                    .unwrap_or_else(|| self.runtime.session().frame.clone());
                if let Some(font) = self.runtime.session().font() {
                    if let Some(panel) = self.runtime.panel() {
                        draw_panel(&mut copy, panel, 0, 72, 320, 56);
                    }
                    font.draw_text(&mut copy, 14, 82, "THE GAME IS PAUSED.", Some(0));
                    font.draw_text(&mut copy, 14, 96, "TO CONTINUE, PRESS A KEY.", Some(0));
                }
                display = Some(copy);
            }
            Screen::Story => {
                let mut copy = display
                    .take()
                    .unwrap_or_else(|| self.runtime.session().frame.clone());
                draw_story(
                    &mut copy,
                    self.runtime.session(),
                    self.runtime.story(),
                    self.runtime.panel(),
                    self.runtime.story_page(),
                );
                display = Some(copy);
            }
            Screen::Travel => {
                let mut copy = display
                    .take()
                    .unwrap_or_else(|| self.runtime.session().frame.clone());
                draw_travel(
                    &mut copy,
                    self.runtime.session(),
                    self.runtime.travel_index(),
                );
                display = Some(copy);
            }
            Screen::Save => {
                let mut copy = display
                    .take()
                    .unwrap_or_else(|| self.runtime.session().frame.clone());
                draw_saves(
                    &mut copy,
                    self.runtime.session(),
                    self.runtime.save_files(),
                    self.runtime.save_index(),
                );
                display = Some(copy);
            }
            Screen::Action => {
                let mut copy = display
                    .take()
                    .unwrap_or_else(|| self.runtime.session().frame.clone());
                draw_actions(
                    &mut copy,
                    self.runtime.session(),
                    self.runtime.panel(),
                    self.runtime.action_index(),
                );
                display = Some(copy);
            }
            Screen::Item => {
                let mut copy = display
                    .take()
                    .unwrap_or_else(|| self.runtime.session().frame.clone());
                draw_items(
                    &mut copy,
                    self.runtime.session(),
                    self.runtime.panel(),
                    self.runtime.item_index(),
                    self.runtime.item_action(),
                );
                display = Some(copy);
            }
            Screen::Target => {
                let mut copy = display
                    .take()
                    .unwrap_or_else(|| self.runtime.session().frame.clone());
                draw_targets(
                    &mut copy,
                    self.runtime.session(),
                    self.runtime.panel(),
                    self.runtime.target_index(),
                );
                display = Some(copy);
            }
            Screen::Menu => {
                let mut copy = self.runtime.session().frame.clone();
                draw_menu(&mut copy, self.runtime.session(), self.runtime.panel());
                display = Some(copy);
            }
            Screen::GameLost => {
                let mut copy = self.runtime.session().frame.clone();
                draw_game_lost(&mut copy, self.runtime.session());
                display = Some(copy);
            }
            Screen::Room | Screen::Map | Screen::Quit => {}
        }

        let Some(surface) = &mut self.surface else {
            return;
        };
        let Ok(mut buffer) = surface.buffer_mut() else {
            return;
        };
        let out_w = size.width as usize;
        let out_h = size.height as usize;
        let src = display.as_ref().unwrap_or(&self.runtime.session().frame);
        let palette = &self.runtime.session().palettes().room.colors;
        let sw = usize::from(src.width);
        let sh = usize::from(src.height);
        for y in 0..out_h {
            let sy = y * sh / out_h;
            for x in 0..out_w {
                let sx = x * sw / out_w;
                let index = usize::from(src.pixels[sy * sw + sx]);
                let [r, g, b] = palette.get(index).copied().unwrap_or([0, 0, 0]);
                let r = (f32::from(r) * fade) as u8;
                let g = (f32::from(g) * fade) as u8;
                let b = (f32::from(b) * fade) as u8;
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
                let (width, height) = self.window.as_ref().map_or((1.0, 1.0), |window| {
                    let size = window.inner_size();
                    (f64::from(size.width), f64::from(size.height))
                });
                let x = if width > 0.0 {
                    (self.cursor.0 / width * f64::from(soe_video::LOGICAL_WIDTH)) as i16
                } else {
                    0
                };
                let y = if height > 0.0 {
                    (self.cursor.1 / height * f64::from(soe_video::LOGICAL_HEIGHT)) as i16
                } else {
                    0
                };
                let events = self.runtime.handle_click(x, y);
                self.apply(event_loop, events);
            }
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed && !event.repeat =>
            {
                if let Some(key) = runtime_key(event.physical_key) {
                    let events = self.runtime.handle_key(key);
                    self.apply(event_loop, events);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let delta = now.saturating_duration_since(self.last_tick);
        self.last_tick = now;
        let events = self.runtime.tick(delta);
        if !events.is_empty() {
            self.apply(event_loop, events);
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(16),
        ));
    }
}

fn runtime_key(key: PhysicalKey) -> Option<Key> {
    match key {
        PhysicalKey::Code(KeyCode::ArrowUp) => Some(Key::Up),
        PhysicalKey::Code(KeyCode::ArrowDown) => Some(Key::Down),
        PhysicalKey::Code(KeyCode::ArrowLeft) => Some(Key::Left),
        PhysicalKey::Code(KeyCode::ArrowRight) => Some(Key::Right),
        PhysicalKey::Code(KeyCode::Enter) => Some(Key::Confirm),
        PhysicalKey::Code(KeyCode::Escape) => Some(Key::Cancel),
        PhysicalKey::Code(KeyCode::Space) => Some(Key::Space),
        PhysicalKey::Code(KeyCode::KeyM) => Some(Key::Map),
        PhysicalKey::Code(KeyCode::KeyI) => Some(Key::Status),
        PhysicalKey::Code(KeyCode::KeyH) => Some(Key::Story),
        PhysicalKey::Code(KeyCode::KeyT) => Some(Key::Travel),
        PhysicalKey::Code(KeyCode::KeyS) => Some(Key::Save),
        PhysicalKey::Code(KeyCode::KeyP) => Some(Key::Pause),
        PhysicalKey::Code(KeyCode::KeyA) => Some(Key::Action),
        PhysicalKey::Code(KeyCode::F5) => Some(Key::QuickSave),
        PhysicalKey::Code(KeyCode::F9) => Some(Key::QuickLoad),
        PhysicalKey::Code(KeyCode::F2) => Some(Key::Mute),
        _ => None,
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
        draw_panel(frame, panel, 0, 0, 320, 96);
    } else {
        fill_rect(frame, 0, 0, 320, 96, 0);
    }
    let Some(font) = session.font() else {
        return;
    };
    let color = if panel.is_some() { 0 } else { 7 };
    font.draw_text(frame, 12, 8, "CHRONICLE  H CLOSE  ARROWS PAGE", Some(color));
    let Some(story) = story else {
        font.draw_text(frame, 12, 24, "NO HINT DATA", Some(color));
        return;
    };
    if story.is_empty() {
        font.draw_text(frame, 12, 24, "NO HINT DATA", Some(color));
        return;
    }
    let index = page % story.paragraphs.len();
    let lines = wrap_text(font, &story.paragraphs[index], 292);
    for (line, text) in lines.iter().take(7).enumerate() {
        font.draw_text(frame, 12, 22 + line as i32 * 10, text, Some(color));
    }
    font.draw_text(
        frame,
        270,
        83,
        &format!("{}/{}", index + 1, story.paragraphs.len()),
        Some(color),
    );
}

fn draw_actions(
    frame: &mut IndexedFrame,
    session: &Session,
    panel: Option<&soe_video::PlanarImage>,
    selected: usize,
) {
    const ACTIONS: [&str; 12] = [
        "Pickup", "Drop", "Talk", "Use", "Search", "Trade", "Request", "Magic", "Seize", "Give",
        "Bribe", "Attack",
    ];
    if let Some(panel) = panel {
        draw_panel(frame, panel, 0, 0, 320, 96);
    } else {
        fill_rect(frame, 0, 0, 320, 96, 0);
    }
    let Some(font) = session.font() else {
        return;
    };
    let name = session
        .state
        .characters
        .first()
        .map(|character| character.name().to_ascii_uppercase())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "CONSTANTINE".to_string());
    font.draw_text(
        frame,
        10,
        6,
        &format!("SELECT AN ACTION FOR SIR {name}:"),
        Some(0),
    );
    for (index, action) in ACTIONS.iter().enumerate() {
        let column = (index % 4) as i32;
        let row = (index / 4) as i32;
        let x = 12 + column * 76;
        let y = 26 + row * 20;
        fill_rect(
            frame,
            x as u16,
            y as u16,
            70,
            15,
            if index == selected { 0 } else { 7 },
        );
        font.draw_text(
            frame,
            x + 4,
            y + 3,
            action,
            Some(if index == selected { 8 } else { 0 }),
        );
    }
}

fn draw_targets(
    frame: &mut IndexedFrame,
    session: &Session,
    panel: Option<&soe_video::PlanarImage>,
    selected: usize,
) {
    if let Some(panel) = panel {
        draw_panel(frame, panel, 0, 0, 320, 96);
    } else {
        fill_rect(frame, 0, 0, 320, 96, 0);
    }
    let Some(font) = session.font() else {
        return;
    };
    font.draw_text(frame, 10, 6, "PLEASE SELECT:", Some(0));
    let targets: Vec<_> = session
        .state
        .characters
        .iter()
        .skip(1)
        .filter(|character| !character.name().is_empty())
        .collect();
    if targets.is_empty() {
        font.draw_text(frame, 12, 26, "NO TARGETS", Some(0));
        return;
    }
    let selected = selected.min(targets.len() - 1);
    let page = selected / 8;
    for row in 0..8 {
        let index = page * 8 + row;
        let Some(target) = targets.get(index) else {
            break;
        };
        let marker = if index == selected { "*" } else { " " };
        font.draw_text(
            frame,
            12,
            24 + row as i32 * 8,
            &format!("{marker}{}", target.name().to_ascii_uppercase()),
            Some(0),
        );
    }
}

fn draw_items(
    frame: &mut IndexedFrame,
    session: &Session,
    panel: Option<&soe_video::PlanarImage>,
    selected: usize,
    action: usize,
) {
    if let Some(panel) = panel {
        draw_panel(frame, panel, 0, 0, 320, 96);
    } else {
        fill_rect(frame, 0, 0, 320, 96, 0);
    }
    let Some(font) = session.font() else {
        return;
    };
    let name = session
        .state
        .characters
        .first()
        .map(|character| character.name().to_ascii_uppercase())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "CONSTANTINE".to_string());
    let verb = match action {
        0 => "PICK UP",
        1 => "DROP",
        3 => "USE",
        9 => "GIVE",
        _ => "SELECT",
    };
    font.draw_text(frame, 10, 6, &format!("WHAT SHALL {name} {verb}?"), Some(0));
    let items: Vec<_> = session
        .state
        .items
        .iter()
        .filter(|item| !item.name().is_empty())
        .collect();
    if items.is_empty() {
        font.draw_text(frame, 12, 26, "NOTHING", Some(0));
        return;
    }
    let page = selected.min(items.len() - 1) / 8;
    for row in 0..8 {
        let index = page * 8 + row;
        let Some(item) = items.get(index) else {
            break;
        };
        let marker = if index == selected { "*" } else { " " };
        font.draw_text(
            frame,
            12,
            24 + row as i32 * 8,
            &format!("{marker}{}", item.name().to_ascii_uppercase()),
            Some(0),
        );
    }
}

fn draw_saves(frame: &mut IndexedFrame, session: &Session, files: &[String], selected: usize) {
    fill_rect(frame, 0, 0, 320, 200, 0);
    let Some(font) = session.font() else {
        return;
    };
    font.draw_text(frame, 8, 6, "SAVE / LOAD", Some(8));
    font.draw_text(frame, 8, 20, "ENTER LOAD  F5 QUICK SAVE  S CLOSE", Some(8));
    if files.is_empty() {
        font.draw_text(frame, 8, 40, "NO .SAV FILES", Some(8));
        return;
    }
    let selected = selected.min(files.len() - 1);
    let page = selected / 13;
    for row in 0..13 {
        let index = page * 13 + row;
        let Some(name) = files.get(index) else {
            break;
        };
        let marker = if index == selected { "*" } else { " " };
        font.draw_text(
            frame,
            8,
            36 + row as i32 * 12,
            &format!("{marker}{name}"),
            Some(8),
        );
    }
    font.draw_text(
        frame,
        220,
        188,
        &format!("PAGE {}/{}", page + 1, files.len().div_ceil(13)),
        Some(8),
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

fn draw_menu(frame: &mut IndexedFrame, session: &Session, panel: Option<&soe_video::PlanarImage>) {
    fill_rect(frame, 0, 0, 320, 200, 0);
    if let Some(panel) = panel {
        draw_panel(frame, panel, 20, 40, 280, 96);
    }
    let Some(font) = session.font() else {
        return;
    };
    font.draw_text(frame, 52, 58, "SPIRIT OF EXCALIBUR", Some(8));
    font.draw_text(frame, 58, 78, "ENTER CONTINUE", Some(0));
    font.draw_text(frame, 70, 92, "ESC QUIT", Some(0));
}

fn draw_game_lost(frame: &mut IndexedFrame, session: &Session) {
    fill_rect(frame, 0, 0, 320, 200, 0);
    let Some(font) = session.font() else {
        return;
    };
    font.draw_text(frame, 126, 90, "GAMELOST", Some(8));
    font.draw_text(frame, 104, 110, "PRESS ENTER TO QUIT", Some(7));
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
    let music = MusicLibrary::load(game_dir.join("MUSIC.RES")).ok();
    let audio = audio::AudioPlayer::new();
    if let (Some(player), Some(library)) = (&audio, &music) {
        if let Some(song) = library.map() {
            player.play_score(&song.score);
        }
    }
    let mut runtime = Runtime::new(session, scene_ids, start, game_dir.clone(), exe.clone())
        .with_assets(map, panel, story);
    if start_map {
        runtime.open_screen(Screen::Map);
    }
    if start_status {
        runtime.open_screen(Screen::Status);
    }
    if args.iter().any(|arg| arg == "--story") {
        runtime.open_screen(Screen::Story);
    }
    if args.iter().any(|arg| arg == "--travel") {
        runtime.open_screen(Screen::Travel);
    }
    if args.iter().any(|arg| arg == "--saves") {
        runtime.open_screen(Screen::Save);
    }
    if args.iter().any(|arg| arg == "--actions") {
        runtime.open_screen(Screen::Action);
    }
    if args.iter().any(|arg| arg == "--items") {
        runtime.open_item(9);
    }
    if args.iter().any(|arg| arg == "--targets") {
        runtime.open_target(0);
    }
    if args.iter().any(|arg| arg == "--paused") {
        runtime.open_screen(Screen::Pause);
    }

    let event_loop = EventLoop::new().context("creating event loop")?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::new(runtime);
    app.audio = audio;
    event_loop.run_app(&mut app).context("running window")?;
    Ok(())
}
