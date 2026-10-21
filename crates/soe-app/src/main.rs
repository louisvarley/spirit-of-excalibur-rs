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
use soe_engine::battle::{Battle, Order};
use soe_engine::runtime::{Key, Runtime, RuntimeEvent, Screen};
use soe_engine::session::Session;
use soe_engine::smus::MusicLibrary;
use soe_engine::story::HintBook;
use soe_engine::window::{self as window, WindowOp};
use soe_engine::worldmap::WorldMap;
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
                | RuntimeEvent::BattleEnded(_)
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

    /// Build the logical frame for the current screen and the palette to
    /// present it with.
    fn compose_frame(&self) -> (IndexedFrame, Vec<[u8; 3]>) {
        let screen = self.runtime.screen();

        let mut display = if matches!(screen, Screen::Map | Screen::Travel) {
            let palette = &self.runtime.session().palettes().regional_map();
            let markers = self.runtime.map_markers();
            self.runtime.map().and_then(|map| {
                map.render(
                    palette,
                    self.runtime.map_camera(),
                    &markers,
                    self.runtime.selected_location(),
                    self.runtime.current_location(),
                )
                .ok()
            })
        } else {
            None
        };
        match screen {
            Screen::Status => {
                let mut copy = display
                    .take()
                    .unwrap_or_else(|| self.runtime.session().frame.clone());
                draw_status(
                    &mut copy,
                    self.runtime.session(),
                    self.runtime.selected_character(),
                    self.runtime.notice(),
                );
                display = Some(copy);
            }
            Screen::Pause => {
                let mut copy = display
                    .take()
                    .unwrap_or_else(|| self.runtime.session().frame.clone());
                if let Some(font) = self.runtime.session().font() {
                    if let Some(panel) = self.runtime.panel() {
                        draw_panel(&mut copy, self.runtime.session(), panel, 0, 72, 320, 56);
                    }
                    font.draw_text(&mut copy, 14, 82, "THE GAME IS PAUSED.", None);
                    font.draw_text(&mut copy, 14, 96, "TO CONTINUE, PRESS A KEY.", None);
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
            Screen::Map | Screen::Travel => {
                let mut copy = display
                    .take()
                    .unwrap_or_else(|| self.runtime.session().frame.clone());
                draw_world_map(
                    &mut copy,
                    self.runtime.session(),
                    self.runtime.selected_location_name().as_deref(),
                    self.runtime.current_location_name().as_deref(),
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
                    &self.runtime.actor_name(),
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
                    &self.runtime.item_names(),
                    &self.runtime.actor_name(),
                    self.runtime.notice(),
                );
                display = Some(copy);
            }
            Screen::Spell => {
                let mut copy = display
                    .take()
                    .unwrap_or_else(|| self.runtime.session().frame.clone());
                draw_name_list(
                    &mut copy,
                    self.runtime.session(),
                    self.runtime.panel(),
                    &format!(
                        "WHAT SPELL SHALL {} CAST?",
                        self.runtime.actor_name().to_ascii_uppercase()
                    ),
                    &self
                        .runtime
                        .spell_entries()
                        .into_iter()
                        .map(|entry| entry.name)
                        .collect::<Vec<_>>(),
                    self.runtime.item_index(),
                );
                draw_notice(&mut copy, self.runtime.session(), self.runtime.notice());
                display = Some(copy);
            }
            Screen::Shop => {
                let mut copy = display
                    .take()
                    .unwrap_or_else(|| self.runtime.session().frame.clone());
                let gold = self
                    .runtime
                    .session()
                    .state
                    .gold(self.runtime.selected_character());
                let title = if self.runtime.shop_sell() {
                    format!("SELL  GOLD {gold}  SPACE TO BUY")
                } else {
                    format!("BUY  GOLD {gold}  SPACE TO SELL")
                };
                draw_name_list(
                    &mut copy,
                    self.runtime.session(),
                    self.runtime.panel(),
                    &title,
                    &self
                        .runtime
                        .shop_entries()
                        .into_iter()
                        .map(|entry| format!("{}  {}g", entry.name, entry.price))
                        .collect::<Vec<_>>(),
                    self.runtime.shop_index(),
                );
                draw_notice(&mut copy, self.runtime.session(), self.runtime.notice());
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
                    &self.runtime.target_names(),
                );
                display = Some(copy);
            }
            Screen::Battle => {
                let mut copy = self.runtime.session().frame.clone();
                draw_battle(
                    &mut copy,
                    self.runtime.session(),
                    self.runtime.battle(),
                    self.runtime.battle_order_index(),
                    self.runtime.battle_orders(),
                );
                display = Some(copy);
            }
            Screen::Dialogue => {
                let mut copy = display
                    .take()
                    .unwrap_or_else(|| self.runtime.session().frame.clone());
                if let Some(dialogue) = self.runtime.dialogue() {
                    draw_dialogue(
                        &mut copy,
                        self.runtime.session(),
                        self.runtime.panel(),
                        &self.runtime.actor_name(),
                        dialogue,
                    );
                }
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
            Screen::Room | Screen::Quit => {}
        }

        let session = self.runtime.session();
        let frame = display.unwrap_or_else(|| session.frame.clone());
        let palette = if matches!(screen, Screen::Map | Screen::Travel) {
            session.palettes().regional_map().colors
        } else {
            session.scene_palette().colors.clone()
        };
        (frame, palette)
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
        let (frame, palette) = self.compose_frame();
        let src = &frame;

        let Some(surface) = &mut self.surface else {
            return;
        };
        let Ok(mut buffer) = surface.buffer_mut() else {
            return;
        };
        let out_w = size.width as usize;
        let out_h = size.height as usize;
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

/// Full-width stone window for list/text screens, or black without the
/// sheet; returns the x where text starts.
fn panel_or_fill(
    frame: &mut IndexedFrame,
    session: &Session,
    panel: Option<&soe_video::PlanarImage>,
    height: i32,
) -> i32 {
    match panel {
        Some(panel) => draw_panel(frame, session, panel, 0, 0, 320, height).0,
        None => {
            fill_rect(frame, 0, 0, 320, height as u16, 0);
            10
        }
    }
}

fn draw_story(
    frame: &mut IndexedFrame,
    session: &Session,
    story: Option<&HintBook>,
    panel: Option<&soe_video::PlanarImage>,
    page: usize,
) {
    let ox = panel_or_fill(frame, session, panel, 96);
    let Some(font) = session.font() else {
        return;
    };
    // The original blits glyphs in their atlas colours (`14b0:0af9`, flags
    // 0x46, no remap); a solid colour is only used on the black fallback.
    let color = if panel.is_some() { None } else { Some(7) };
    font.draw_text(frame, ox + 2, 8, "CHRONICLE  H CLOSE  ARROWS PAGE", color);
    let Some(story) = story else {
        font.draw_text(frame, ox + 2, 24, "NO HINT DATA", color);
        return;
    };
    if story.is_empty() {
        font.draw_text(frame, ox + 2, 24, "NO HINT DATA", color);
        return;
    }
    let index = page % story.paragraphs.len();
    let lines = wrap_text(font, &story.paragraphs[index].replace('\n', " "), 250);
    for (line, text) in lines.iter().take(7).enumerate() {
        font.draw_text(frame, ox + 2, 22 + line as i32 * 10, text, color);
    }
    font.draw_text(
        frame,
        240,
        8,
        &format!("{}/{}", index + 1, story.paragraphs.len()),
        color,
    );
}

/// Room action menu (`FUN_1ba8_4604`): a 250-wide window at (10, 5) with
/// four columns of buttons drawn by `FUN_15b8_0461`/`FUN_1772_01ec`.
fn draw_actions(
    frame: &mut IndexedFrame,
    session: &Session,
    panel: Option<&soe_video::PlanarImage>,
    selected: usize,
    actor: &str,
) {
    const ACTIONS: [&str; 12] = [
        "Pickup", "Drop", "Talk", "Use", "Search", "Trade", "Request", "Magic", "Seize", "Give",
        "Bribe", "Attack",
    ];
    const COLUMNS: [i32; 4] = [3, 0x3a, 0x71, 0xa8];
    let Some(font) = session.font() else {
        return;
    };
    let line = i32::from(font.height);
    let rows = [line + 6, 2 * line + 14, 3 * line + 22];
    let height = rows[2] + line + 0xd;
    let (ox, oy) = match panel {
        Some(panel) => draw_panel(frame, session, panel, 8, 5, 0xfa + 0x20, height),
        None => {
            fill_rect(frame, 0, 0, 320, height as u16 + 5, 0);
            (8 + 0x1f, 10)
        }
    };
    font.draw_text(frame, ox, oy, &format!("Select an action for {actor}:"), None);
    let label_width = ACTIONS
        .iter()
        .map(|label| font.text_width(label))
        .max()
        .unwrap_or(0) as u16;
    for (index, action) in ACTIONS.iter().enumerate() {
        let x = ox + COLUMNS[index % 4];
        let y = oy + rows[index / 4];
        font.draw_text(frame, x, y, action, None);
        if let (Some(style), Some(sheet)) = (session.window_style(), panel) {
            let ops = style.button(x as i16, y as i16, label_width, index == selected);
            draw_window_ops(frame, sheet, 0, 0, &ops);
        }
    }
}

fn draw_targets(
    frame: &mut IndexedFrame,
    session: &Session,
    panel: Option<&soe_video::PlanarImage>,
    selected: usize,
    targets: &[String],
) {
    let ox = panel_or_fill(frame, session, panel, 96);
    let Some(font) = session.font() else {
        return;
    };
    font.draw_text(frame, ox, 6, "PLEASE SELECT:", None);
    if targets.is_empty() {
        font.draw_text(frame, ox + 2, 26, "NO TARGETS", None);
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
            ox + 2,
            24 + row as i32 * 8,
            &format!("{marker}{}", target.to_ascii_uppercase()),
            None,
        );
    }
}

fn draw_items(
    frame: &mut IndexedFrame,
    session: &Session,
    panel: Option<&soe_video::PlanarImage>,
    selected: usize,
    action: usize,
    items: &[String],
    actor: &str,
    notice: &str,
) {
    let ox = panel_or_fill(frame, session, panel, 96);
    let Some(font) = session.font() else {
        return;
    };
    let name = actor.to_ascii_uppercase();
    let verb = match action {
        0 => "PICK UP",
        1 => "DROP",
        3 => "USE / EQUIP",
        9 => "GIVE",
        _ => "SELECT",
    };
    font.draw_text(frame, ox, 6, &format!("WHAT SHALL {name} {verb}?"), None);
    if items.is_empty() {
        font.draw_text(frame, ox + 2, 26, "NOTHING", None);
        if !notice.is_empty() {
            font.draw_text(frame, ox + 2, 82, &truncate(notice, 44), None);
        }
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
            ox + 2,
            24 + row as i32 * 8,
            &format!("{marker}{}", item.to_ascii_uppercase()),
            None,
        );
    }
    if !notice.is_empty() {
        font.draw_text(frame, ox + 2, 86, &truncate(notice, 44), None);
    }
}

fn draw_name_list(
    frame: &mut IndexedFrame,
    session: &Session,
    panel: Option<&soe_video::PlanarImage>,
    title: &str,
    names: &[String],
    selected: usize,
) {
    let ox = panel_or_fill(frame, session, panel, 96);
    let Some(font) = session.font() else {
        return;
    };
    font.draw_text(frame, ox, 6, title, None);
    if names.is_empty() {
        font.draw_text(frame, ox + 2, 26, "NOTHING", None);
        return;
    }
    let page = selected.min(names.len() - 1) / 8;
    for row in 0..8 {
        let index = page * 8 + row;
        let Some(name) = names.get(index) else {
            break;
        };
        let marker = if index == selected { "*" } else { " " };
        font.draw_text(
            frame,
            ox + 2,
            24 + row as i32 * 8,
            &format!("{marker}{}", name.to_ascii_uppercase()),
            None,
        );
    }
}

fn draw_notice(frame: &mut IndexedFrame, session: &Session, notice: &str) {
    if notice.is_empty() {
        return;
    }
    let Some(font) = session.font() else {
        return;
    };
    fill_rect(frame, 0, 96, 320, 16, 0);
    font.draw_text(frame, 10, 99, &truncate(notice, 44), Some(7));
}

fn draw_dialogue(
    frame: &mut IndexedFrame,
    session: &Session,
    panel: Option<&soe_video::PlanarImage>,
    actor: &str,
    dialogue: &soe_engine::interaction::Dialogue,
) {
    let ox = panel_or_fill(frame, session, panel, 112);
    let Some(font) = session.font() else {
        return;
    };
    let speaker = soe_engine::interaction::character_name(&session.state, dialogue.speaker);
    let header = format!("{speaker} speaks to {actor}.").to_ascii_uppercase();
    font.draw_text(frame, ox + 2, 8, &header, None);
    let lines = wrap_text(font, &dialogue.text, 250);
    for (line, text) in lines.iter().take(7).enumerate() {
        font.draw_text(frame, ox + 2, 22 + line as i32 * 10, text, None);
    }
    if dialogue.has_choices() {
        let y = 94;
        for (index, choice) in dialogue.choices.iter().take(2).enumerate() {
            let marker = if index == dialogue.selected { "*" } else { " " };
            font.draw_text(
                frame,
                ox + 4 + index as i32 * 120,
                y,
                &format!("{marker}{}", choice.label.to_ascii_uppercase()),
                None,
            );
        }
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

fn draw_world_map(
    frame: &mut IndexedFrame,
    session: &Session,
    selected: Option<&str>,
    current: Option<&str>,
) {
    let Some(font) = session.font() else {
        return;
    };
    let selected = selected
        .filter(|name| !name.is_empty())
        .map_or_else(|| "NO LOCATIONS".to_string(), str::to_ascii_uppercase);
    font.draw_text(frame, 20, 18, "ENTER TRAVEL   T CLOSE   ARROWS", None);
    let banner = match current.filter(|name| !name.is_empty()) {
        Some(current) => format!("{}   AT {}", selected, current.to_ascii_uppercase()),
        None => selected,
    };
    let width = u32::try_from(banner.len()).unwrap_or(0) * 8;
    let x = 160i32 - i32::try_from(width / 2).unwrap_or(0);
    font.draw_text(frame, x.clamp(18, 250), 183, &banner, Some(7));
}

/// Stone dialog window (`FUN_2036_0009`) drawn at `(x, y)` covering roughly
/// `width` x `height`; returns the window's content origin in frame space.
fn draw_panel(
    frame: &mut IndexedFrame,
    session: &Session,
    sheet: &soe_video::PlanarImage,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> (i32, i32) {
    let Some(style) = session.window_style() else {
        fill_rect(frame, x as u16, y as u16, width as u16, height as u16, 5);
        return (x + 0x1f, y + 5);
    };
    // FUN_1772_000e passes `(width + 7) & 0xfff8` for the inner width; the
    // builder adds 0x20 for the two side columns.
    let inner = ((width - 0x20).max(8) as u16 + 7) & 0xfff8;
    let layout = style.layout(inner, height as u16, (x as u32) << 16 | y as u32);
    draw_window_ops(frame, sheet, x, y, &layout.ops);
    (x + i32::from(layout.content_x), y + i32::from(layout.content_y))
}

/// Execute window drawing operations against the `TITLE.RES#300` sheet.
fn draw_window_ops(
    frame: &mut IndexedFrame,
    sheet: &soe_video::PlanarImage,
    x: i32,
    y: i32,
    ops: &[WindowOp],
) {
    for op in ops {
        match *op {
            WindowOp::Fill {
                x: fx,
                y: fy,
                width,
                height,
                color,
            } => {
                for py in 0..i32::from(height) {
                    for px in 0..i32::from(width) {
                        let (dx, dy) = (x + i32::from(fx) + px, y + i32::from(fy) + py);
                        if dx >= 0 && dy >= 0 {
                            frame.set_pixel(dx as u16, dy as u16, color);
                        }
                    }
                }
            }
            WindowOp::Piece {
                src_x,
                src_y,
                width,
                height,
                x: px,
                y: py,
                mirror,
            } => {
                let sw = i32::from(sheet.width);
                for row in 0..i32::from(height) {
                    let sy = i32::from(src_y) + row;
                    if sy >= i32::from(sheet.height) {
                        break;
                    }
                    for col in 0..i32::from(width) {
                        let sx = if mirror {
                            i32::from(src_x) + i32::from(width) - 1 - col
                        } else {
                            i32::from(src_x) + col
                        };
                        if !(0..sw).contains(&sx) {
                            continue;
                        }
                        let (dx, dy) = (x + i32::from(px) + col, y + i32::from(py) + row);
                        if dx >= 0 && dy >= 0 {
                            frame.set_pixel(dx as u16, dy as u16, sheet.indices[(sy * sw + sx) as usize]);
                        }
                    }
                }
            }
        }
    }
}

fn draw_menu(frame: &mut IndexedFrame, session: &Session, panel: Option<&soe_video::PlanarImage>) {
    fill_rect(frame, 0, 0, 320, 200, 0);
    if let Some(panel) = panel {
        draw_panel(frame, session, panel, 20, 40, 280, 96);
    }
    let Some(font) = session.font() else {
        return;
    };
    font.draw_text(frame, 52, 58, "SPIRIT OF EXCALIBUR", Some(8));
    font.draw_text(frame, 58, 78, "ENTER CONTINUE", None);
    font.draw_text(frame, 70, 92, "ESC QUIT", None);
}

fn draw_game_lost(frame: &mut IndexedFrame, session: &Session) {
    fill_rect(frame, 0, 0, 320, 200, 0);
    let Some(font) = session.font() else {
        return;
    };
    font.draw_text(frame, 126, 90, "GAMELOST", Some(8));
    font.draw_text(frame, 104, 110, "PRESS ENTER TO QUIT", Some(7));
}

fn draw_status(frame: &mut IndexedFrame, session: &Session, selected: usize, notice: &str) {
    fill_rect(frame, 10, 18, 300, 168, 0);
    fill_rect(frame, 10, 18, 300, 1, 7);
    fill_rect(frame, 10, 185, 300, 1, 7);
    fill_rect(frame, 10, 18, 1, 168, 7);
    fill_rect(frame, 309, 18, 1, 168, 7);
    let Some(font) = session.font() else {
        return;
    };
    font.draw_text(frame, 16, 22, "ROUND TABLE", Some(8));
    font.draw_text(frame, 190, 22, "UP/DN KNIGHT  ENTER ITEMS", Some(8));
    font.draw_text(frame, 16, 38, "NAME", Some(8));
    font.draw_text(frame, 112, 38, "HP HT MV COM ARM NOB FTH GOLD", Some(8));
    fill_rect(frame, 16, 47, 286, 1, 7);

    let indices: Vec<usize> = session
        .state
        .characters
        .iter()
        .enumerate()
        .filter(|(_, character)| !character.name().is_empty())
        .map(|(index, _)| index)
        .collect();
    if indices.is_empty() {
        font.draw_text(frame, 16, 56, "NO KNIGHTS", Some(8));
        return;
    }
    let position = indices
        .iter()
        .position(|index| *index == selected)
        .unwrap_or(0);
    let page = position / 8;
    for row in 0..8 {
        let index = page * 8 + row;
        let Some(&character) = indices.get(index) else {
            break;
        };
        let Some(sheet) = session.state.character_sheet(character) else {
            continue;
        };
        let marker = if index == position { "*" } else { " " };
        let y = 51 + row as i32 * 10;
        font.draw_text(
            frame,
            16,
            y,
            &format!("{marker}{}", sheet.name.to_ascii_uppercase()),
            Some(8),
        );
        font.draw_text(frame, 112, y, &format!("{:2}", sheet.health), Some(8));
        font.draw_text(frame, 134, y, &format!("{:2}", sheet.hits), Some(8));
        font.draw_text(frame, 152, y, &format!("{:2}", sheet.movement), Some(8));
        font.draw_text(
            frame,
            170,
            y,
            &format!("{:3}", sheet.effective_combat),
            Some(8),
        );
        font.draw_text(
            frame,
            194,
            y,
            &format!("{:3}", sheet.effective_armor),
            Some(8),
        );
        font.draw_text(frame, 220, y, &format!("{:3}", sheet.nobility), Some(8));
        font.draw_text(frame, 244, y, &format!("{:3}", sheet.faith), Some(8));
        font.draw_text(frame, 268, y, &format!("{:4}", sheet.gold), Some(8));
    }

    let sheet = session.state.character_sheet(selected);
    let (gear, spells) = sheet.map_or_else(
        || (String::new(), String::new()),
        |sheet| {
            let gear = sheet
                .equipment
                .iter()
                .map(|entry| entry.name.to_ascii_uppercase())
                .collect::<Vec<_>>()
                .join(", ");
            let spells = sheet
                .spells
                .iter()
                .map(|entry| entry.name.to_ascii_uppercase())
                .collect::<Vec<_>>()
                .join(", ");
            (gear, spells)
        },
    );
    font.draw_text(frame, 16, 136, "GEAR:", Some(8));
    font.draw_text(frame, 56, 136, &truncate(&gear, 34), Some(8));
    font.draw_text(frame, 16, 150, "MAGIC:", Some(8));
    font.draw_text(frame, 62, 150, &truncate(&spells, 33), Some(8));
    if !notice.is_empty() {
        font.draw_text(frame, 16, 168, &truncate(notice, 38), Some(7));
    }
}

fn draw_battle(
    frame: &mut IndexedFrame,
    session: &Session,
    battle: Option<&Battle>,
    selected: usize,
    orders: &[Order],
) {
    fill_rect(frame, 0, 0, 320, 200, 7);
    let Some(font) = session.font() else {
        return;
    };
    fill_rect(frame, 0, 0, 320, 13, 0);
    let Some(battle) = battle else {
        font.draw_text(frame, 118, 90, "NO BATTLE", None);
        return;
    };

    let (left_title, right_title) = if battle.alternate_orders {
        ("DEFENDER...", "CHALLENGER...")
    } else {
        ("ROUND TABLE...", "FOES...")
    };
    font.draw_text(frame, 4, 3, left_title, Some(7));
    font.draw_text(frame, 132, 3, &format!("ROUND {}", battle.round), Some(7));
    font.draw_text(frame, 232, 3, right_title, Some(7));

    for (row, unit) in battle.player.iter().take(12).enumerate() {
        let y = 18 + row as i32 * 8;
        let color = if unit.is_routed() { 4 } else { 0 };
        let marker = if unit.is_routed() { "x" } else { " " };
        font.draw_text(
            frame,
            4,
            y,
            &format!("{marker}{}", truncate(&unit.display(), 14)),
            Some(color),
        );
    }
    for (row, unit) in battle.foe.iter().take(12).enumerate() {
        let y = 18 + row as i32 * 8;
        let color = if unit.is_routed() { 4 } else { 0 };
        let marker = if unit.is_routed() { "x" } else { " " };
        font.draw_text(
            frame,
            226,
            y,
            &format!("{marker}{}", truncate(&unit.display(), 12)),
            Some(color),
        );
    }

    for (index, order) in orders.iter().enumerate() {
        let y = 18 + index as i32 * 10;
        if index == selected {
            fill_rect(frame, 118, (y - 1) as u16, 90, 9, 0);
            font.draw_text(frame, 122, y, order.label(), Some(7));
        } else {
            font.draw_text(frame, 122, y, order.label(), None);
        }
    }

    font.draw_text(
        frame,
        118,
        90,
        &format!(
            "MEN {}   FOE {}",
            battle.total_strength(soe_engine::battle::Side::Player),
            battle.total_strength(soe_engine::battle::Side::Foe)
        ),
        None,
    );

    fill_rect(frame, 118, 102, 196, 56, 2);
    fill_rect(frame, 118, 102, 196, 1, 0);
    fill_rect(frame, 118, 157, 196, 1, 0);
    fill_rect(frame, 118, 102, 1, 56, 0);
    fill_rect(frame, 313, 102, 1, 56, 0);
    for (index, unit) in battle.player.iter().take(12).enumerate() {
        if unit.is_routed() {
            continue;
        }
        let x = 126 + (index % 3) as u16 * 7;
        let y = 108 + (index / 3) as u16 * 11;
        fill_rect(frame, x, y, 2, 3, 8);
    }
    for (index, unit) in battle.foe.iter().take(12).enumerate() {
        if unit.is_routed() {
            continue;
        }
        let x = 300 - (index % 3) as u16 * 7;
        let y = 108 + (index / 3) as u16 * 11;
        fill_rect(frame, x, y, 2, 3, 12);
    }

    fill_rect(frame, 0, 163, 320, 37, 0);
    let start = battle.messages.len().saturating_sub(3);
    for (row, message) in battle.messages[start..].iter().enumerate() {
        font.draw_text(
            frame,
            6,
            167 + row as i32 * 10,
            &truncate(message, 50),
            Some(7),
        );
    }
    if let Some(outcome) = battle.outcome {
        font.draw_text(frame, 250, 3, outcome.label(), Some(12));
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let mut out: String = text.chars().take(max.saturating_sub(3)).collect();
        out.push_str("...");
        out
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

fn load_map(game_dir: &Path) -> Result<Option<WorldMap>> {
    let path = game_dir.join("RES32").join("INTF.RES");
    if !path.exists() {
        return Ok(None);
    }
    WorldMap::load(&path)
        .map(Some)
        .map_err(|error| anyhow::anyhow!(error))
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
    let map = load_map(&game_dir)?;
    let panel = load_record_image(
        &game_dir.join("RES32").join(window::SHEET_FILE),
        window::SHEET_ID,
    )
    .ok();
    let story = HintBook::load(game_dir.join("HINT.RES")).ok();
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
    if args.iter().any(|arg| arg == "--spells") {
        runtime.open_screen(Screen::Spell);
    }
    if args.iter().any(|arg| arg == "--shop") {
        runtime.open_screen(Screen::Shop);
    }
    if args.iter().any(|arg| arg == "--targets") {
        runtime.open_target(0);
    }
    if args.iter().any(|arg| arg == "--paused") {
        runtime.open_screen(Screen::Pause);
    }
    if args.iter().any(|arg| arg == "--battle") {
        if !runtime.start_demo_battle() {
            eprintln!("no army records available for a battle");
        }
    }

    if let Some(path) = args
        .windows(2)
        .find(|pair| pair[0] == "--screenshot")
        .map(|pair| PathBuf::from(&pair[1]))
    {
        let app = App::new(runtime);
        let (frame, palette) = app.compose_frame();
        return write_screenshot(&path, &frame, &palette);
    }

    let music = MusicLibrary::load(game_dir.join("MUSIC.RES")).ok();
    let audio = audio::AudioPlayer::new();
    if let (Some(player), Some(library)) = (&audio, &music) {
        if let Some(song) = library.map() {
            player.play_score(&song.score);
        }
    }

    let event_loop = EventLoop::new().context("creating event loop")?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::new(runtime);
    app.audio = audio;
    event_loop.run_app(&mut app).context("running window")?;
    Ok(())
}

/// Write one logical frame as an indexed PNG (headless `--screenshot`).
fn write_screenshot(path: &Path, frame: &IndexedFrame, palette: &[[u8; 3]]) -> Result<()> {
    let mut pal = vec![0u8; 768];
    for (index, color) in palette.iter().enumerate().take(256) {
        pal[index * 3..index * 3 + 3].copy_from_slice(color);
    }
    let file = std::fs::File::create(path)
        .with_context(|| format!("creating {}", path.display()))?;
    let mut encoder = png::Encoder::new(
        std::io::BufWriter::new(file),
        u32::from(frame.width),
        u32::from(frame.height),
    );
    encoder.set_color(png::ColorType::Indexed);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_palette(pal);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&frame.pixels)?;
    Ok(())
}
