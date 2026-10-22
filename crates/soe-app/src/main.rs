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
mod gfx;

use anyhow::{Context, Result};
use gfx::{Gfx, UiAssets};
use soe_engine::army::{ArmyBattle, ArmyOrder};
use soe_engine::runtime::{Key, Runtime, RuntimeEvent, Screen};
use soe_engine::session::Session;
use soe_engine::smus::MusicLibrary;
use soe_engine::story::HintBook;
use soe_engine::window;
use soe_engine::worldmap::WorldMap;
use soe_video::{IndexedFrame, LOGICAL_HEIGHT, LOGICAL_WIDTH, decode_planar};
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
    music: Option<MusicLibrary>,
    ui: UiAssets,
}

impl App {
    fn new(runtime: Runtime, ui: UiAssets) -> Self {
        Self {
            window: None,
            context: None,
            surface: None,
            runtime,
            last_tick: Instant::now(),
            cursor: (0.0, 0.0),
            audio: None,
            music: None,
            ui,
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
                RuntimeEvent::Music(name) => {
                    if let (Some(audio), Some(song)) = (&self.audio, self.music.as_ref().and_then(|m| m.find(&name))) {
                        audio.play_score(&song.score);
                    }
                }
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

        let map_base = if matches!(
            screen,
            Screen::Map | Screen::Travel | Screen::Encounter | Screen::MapMessage
        ) {
            let palette = &self.runtime.session().palettes().regional_map();
            let markers = self.runtime.map_markers();
            let camera = self.runtime.map_camera();
            self.runtime.map().and_then(|map| {
                let mut frame = map
                    .render(
                        palette,
                        camera,
                        &markers,
                        self.runtime.selected_location(),
                        self.runtime.current_location(),
                    )
                    .ok()?;
                for (icon, x, y) in self.runtime.map_entities() {
                    map.draw_entity(&mut frame, camera, icon, x, y);
                }
                Some(frame)
            })
        } else {
            None
        };
        // Pick the engine-composed base frame exactly as before: map screens
        // draw over the regional map, the duel over its field scene, and the
        // rest over the current room.
        let session = self.runtime.session();
        let duel_frame = if screen == Screen::Duel {
            self.runtime.duel_frame()
        } else {
            None
        };
        let duel_base = duel_frame.is_some();
        let base = match screen {
            Screen::Battle => self.runtime.battle().map_or_else(|| session.frame.clone(), |b| session.compose_army(session.frame.clone(), b)),
            Screen::Menu | Screen::GameLost => session.frame.clone(),
            Screen::Duel => duel_frame.unwrap_or_else(|| session.frame.clone()),
            Screen::Intro => self
                .runtime
                .intro()
                .map_or_else(|| session.frame.clone(), soe_engine::intro::Intro::frame),
            _ => map_base.unwrap_or_else(|| session.frame.clone()),
        };
        let mut g = Gfx::new(&self.ui, &base);
        match screen {
            Screen::Status => {
                draw_status(
                    &mut g,
                    self.runtime.session(),
                    self.runtime.selected_character(),
                    self.runtime.notice(),
                );
            }
            Screen::Pause => {
                if g.has_font() {
                    if g.has_sheet() {
                        g.panel(self.runtime.session(), 0, 72, LOGICAL_WIDTH.into(), 56);
                    }
                    g.text(14, 82, "THE GAME IS PAUSED.", None);
                    g.text(14, 96, "TO CONTINUE, PRESS A KEY.", None);
                }
            }
            Screen::Story => {
                draw_story(
                    &mut g,
                    self.runtime.session(),
                    self.runtime.story(),
                    self.runtime.panel(),
                    self.runtime.story_page(),
                );
            }
            Screen::Map | Screen::Travel => {
                draw_world_map(
                    &mut g,
                    self.runtime.session(),
                    self.runtime.selected_location_name().as_deref(),
                    self.runtime.current_location_name().as_deref(),
                );
            }
            Screen::Save => {
                draw_saves(
                    &mut g,
                    self.runtime.session(),
                    self.runtime.save_files(),
                    self.runtime.save_index(),
                );
            }
            Screen::Action => {
                draw_actions(
                    &mut g,
                    self.runtime.session(),
                    self.runtime.panel(),
                    self.runtime.action_index(),
                    &self.runtime.actor_name(),
                );
            }
            Screen::Item => {
                draw_items(
                    &mut g,
                    self.runtime.session(),
                    self.runtime.panel(),
                    self.runtime.item_index(),
                    self.runtime.item_action(),
                    &self.runtime.item_names(),
                    &self.runtime.actor_name(),
                    self.runtime.notice(),
                );
            }
            Screen::Spell => {
                draw_name_list(
                    &mut g,
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
                draw_notice(&mut g, self.runtime.session(), self.runtime.notice());
            }
            Screen::Shop => {
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
                    &mut g,
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
                draw_notice(&mut g, self.runtime.session(), self.runtime.notice());
            }
            Screen::Target => {
                draw_targets(
                    &mut g,
                    self.runtime.session(),
                    self.runtime.panel(),
                    self.runtime.target_index(),
                    &self.runtime.target_names(),
                );
            }
            Screen::Battle => {
                draw_battle(
                    &mut g,
                    self.runtime.session(),
                    self.runtime.battle(),
                    self.runtime.battle_order_index(),
                    self.runtime.battle_unit(),
                );
            }
            Screen::Duel => {
                if duel_base {
                    draw_duel(
                        &mut g,
                        self.runtime.session(),
                        self.runtime.panel(),
                        self.runtime.duel(),
                    );
                }
            }
            Screen::Dialogue => {
                if let Some(dialogue) = self.runtime.dialogue() {
                    draw_dialogue(
                        &mut g,
                        self.runtime.session(),
                        self.runtime.panel(),
                        &self.runtime.actor_name(),
                        dialogue,
                    );
                }
            }
            Screen::Menu => {
                draw_menu(&mut g, self.runtime.session(), self.runtime.panel());
            }
            Screen::GameLost => {
                draw_game_lost(&mut g, self.runtime.session());
            }
            Screen::MapMessage => {
                draw_map_message(&mut g, &self.runtime);
            }
            Screen::Encounter => {
                draw_encounter(&mut g, &self.runtime);
            }
            // Room, Quit and screens without overlay drawing.
            _ => {}
        }

        let frame = g.into_frame();
        let palette = if let Some(intro) = self.runtime.intro() {
            intro.palette().colors
        } else if matches!(
            screen,
            Screen::Map | Screen::Travel | Screen::Encounter | Screen::MapMessage | Screen::Battle
        ) {
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
        soe_video::present_nearest_xrgb(
            src,
            &palette,
            fade,
            size.width as usize,
            size.height as usize,
            &mut buffer,
        );
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
                } else if self.runtime.screen() == Screen::Intro {
                    // Any key skips the intro, mapped or not.
                    self.runtime.skip_intro();
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

fn wrap_text(g: &Gfx, text: &str, max_width: u32) -> Vec<String> {
    let mut lines = Vec::new();
    for raw in text.split('\n') {
        let mut line = String::new();
        for word in raw.split_whitespace() {
            let candidate = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };
            if !line.is_empty() && g.text_width(&candidate) > max_width {
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
    g: &mut Gfx,
    session: &Session,
    panel: Option<&soe_video::PlanarImage>,
    height: i32,
) -> i32 {
    match panel {
        Some(_) => g.panel(session, 0, 0, LOGICAL_WIDTH.into(), height).0,
        None => {
            g.fill(0, 0, LOGICAL_WIDTH, height as u16, 0);
            10
        }
    }
}

fn draw_story(
    g: &mut Gfx,
    session: &Session,
    story: Option<&HintBook>,
    panel: Option<&soe_video::PlanarImage>,
    page: usize,
) {
    let ox = panel_or_fill(g, session, panel, 96);
    if !g.has_font() {
        return;
    }
    // The original blits glyphs in their atlas colours (`14b0:0af9`, flags
    // 0x46, no remap); a solid colour is only used on the black fallback.
    let color = if panel.is_some() { None } else { Some(7) };
    g.text(ox + 2, 8, "CHRONICLE  H CLOSE  ARROWS PAGE", color);
    let Some(story) = story else {
        g.text(ox + 2, 24, "NO HINT DATA", color);
        return;
    };
    if story.is_empty() {
        g.text(ox + 2, 24, "NO HINT DATA", color);
        return;
    }
    let index = page % story.paragraphs.len();
    let lines = wrap_text(g, &story.paragraphs[index].replace('\n', " "), 250);
    for (line, text) in lines.iter().take(7).enumerate() {
        g.text(ox + 2, 22 + line as i32 * 10, text, color);
    }
    g.text(
        240,
        8,
        &format!("{}/{}", index + 1, story.paragraphs.len()),
        color,
    );
}

/// Room action menu (`FUN_1ba8_4604`): a 250-wide window at (10, 5) with
/// four columns of buttons drawn by `FUN_15b8_0461`/`FUN_1772_01ec`.
fn draw_actions(
    g: &mut Gfx,
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
    if !g.has_font() {
        return;
    }
    let line = g.atlas_height();
    let rows = [line + 6, 2 * line + 14, 3 * line + 22];
    let height = rows[2] + line + 0xd;
    let (ox, oy) = match panel {
        Some(_) => g.panel(session, 8, 5, 0xfa + 0x20, height),
        None => {
            g.fill(0, 0, LOGICAL_WIDTH, height as u16 + 5, 0);
            (8 + 0x1f, 10)
        }
    };
    g.text(ox, oy, &format!("Select an action for {actor}:"), None);
    let label_width = ACTIONS
        .iter()
        .map(|label| g.text_width(label))
        .max()
        .unwrap_or(0) as u16;
    for (index, action) in ACTIONS.iter().enumerate() {
        let x = ox + COLUMNS[index % 4];
        let y = oy + rows[index / 4];
        g.text(x, y, action, None);
        if let (Some(style), Some(_)) = (session.window_style(), panel) {
            let ops = style.button(x as i16, y as i16, label_width, index == selected);
            g.window_ops(0, 0, &ops);
        }
    }
}

fn draw_targets(
    g: &mut Gfx,
    session: &Session,
    panel: Option<&soe_video::PlanarImage>,
    selected: usize,
    targets: &[String],
) {
    let ox = panel_or_fill(g, session, panel, 96);
    if !g.has_font() {
        return;
    }
    g.text(ox, 6, "PLEASE SELECT:", None);
    if targets.is_empty() {
        g.text(ox + 2, 26, "NO TARGETS", None);
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
        g.text(
            ox + 2,
            24 + row as i32 * 8,
            &format!("{marker}{}", target.to_ascii_uppercase()),
            None,
        );
    }
}

fn draw_items(
    g: &mut Gfx,
    session: &Session,
    panel: Option<&soe_video::PlanarImage>,
    selected: usize,
    action: usize,
    items: &[String],
    actor: &str,
    notice: &str,
) {
    let ox = panel_or_fill(g, session, panel, 96);
    if !g.has_font() {
        return;
    }
    let name = actor.to_ascii_uppercase();
    let verb = match action {
        0 => "PICK UP",
        1 => "DROP",
        3 => "USE / EQUIP",
        9 => "GIVE",
        _ => "SELECT",
    };
    g.text(ox, 6, &format!("WHAT SHALL {name} {verb}?"), None);
    if items.is_empty() {
        g.text(ox + 2, 26, "NOTHING", None);
        if !notice.is_empty() {
            g.text(ox + 2, 82, &truncate(notice, 44), None);
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
        g.text(
            ox + 2,
            24 + row as i32 * 8,
            &format!("{marker}{}", item.to_ascii_uppercase()),
            None,
        );
    }
    if !notice.is_empty() {
        g.text(ox + 2, 86, &truncate(notice, 44), None);
    }
}

fn draw_name_list(
    g: &mut Gfx,
    session: &Session,
    panel: Option<&soe_video::PlanarImage>,
    title: &str,
    names: &[String],
    selected: usize,
) {
    let ox = panel_or_fill(g, session, panel, 96);
    if !g.has_font() {
        return;
    }
    g.text(ox, 6, title, None);
    if names.is_empty() {
        g.text(ox + 2, 26, "NOTHING", None);
        return;
    }
    let page = selected.min(names.len() - 1) / 8;
    for row in 0..8 {
        let index = page * 8 + row;
        let Some(name) = names.get(index) else {
            break;
        };
        let marker = if index == selected { "*" } else { " " };
        g.text(
            ox + 2,
            24 + row as i32 * 8,
            &format!("{marker}{}", name.to_ascii_uppercase()),
            None,
        );
    }
}

fn draw_notice(g: &mut Gfx, _session: &Session, notice: &str) {
    if notice.is_empty() {
        return;
    }
    if !g.has_font() {
        return;
    }
    g.fill(0, 96, LOGICAL_WIDTH, 16, 0);
    g.text(10, 99, &truncate(notice, 44), Some(7));
}

fn draw_dialogue(
    g: &mut Gfx,
    session: &Session,
    panel: Option<&soe_video::PlanarImage>,
    actor: &str,
    dialogue: &soe_engine::interaction::Dialogue,
) {
    let ox = panel_or_fill(g, session, panel, 112);
    if !g.has_font() {
        return;
    }
    let speaker = soe_engine::interaction::character_name(&session.state, dialogue.speaker);
    let header = format!("{speaker} speaks to {actor}.").to_ascii_uppercase();
    g.text(ox + 2, 8, &header, None);
    let lines = wrap_text(g, &dialogue.text, 250);
    for (line, text) in lines.iter().take(7).enumerate() {
        g.text(ox + 2, 22 + line as i32 * 10, text, None);
    }
    if dialogue.has_choices() {
        let y = 94;
        for (index, choice) in dialogue.choices.iter().take(2).enumerate() {
            let marker = if index == dialogue.selected { "*" } else { " " };
            g.text(
                ox + 4 + index as i32 * 120,
                y,
                &format!("{marker}{}", choice.label.to_ascii_uppercase()),
                None,
            );
        }
    }
}

fn draw_saves(g: &mut Gfx, _session: &Session, files: &[String], selected: usize) {
    g.fill(0, 0, LOGICAL_WIDTH, LOGICAL_HEIGHT, 0);
    if !g.has_font() {
        return;
    }
    g.text(8, 6, "SAVE / LOAD", Some(8));
    g.text(8, 20, "ENTER LOAD  F5 QUICK SAVE  S CLOSE", Some(8));
    if files.is_empty() {
        g.text(8, 40, "NO .SAV FILES", Some(8));
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
        g.text(8, 36 + row as i32 * 12, &format!("{marker}{name}"), Some(8));
    }
    g.text(
        220,
        188,
        &format!("PAGE {}/{}", page + 1, files.len().div_ceil(13)),
        Some(8),
    );
}

/// Map message window (`FUN_1772_000e` from the arrival callback): stone
/// window at (10, 10) with the text; any key or click continues.
fn draw_map_message(g: &mut Gfx, runtime: &Runtime) {
    let session = runtime.session();
    let (true, Some(text)) = (g.has_font(), runtime.map_message()) else {
        return;
    };
    let lines: Vec<&str> = text.split('\n').collect();
    let height = 10 + 10 * lines.len() as i32;
    let (ox, oy) = match runtime.panel() {
        Some(_) => g.panel(session, 8, 10, 0x11d + 0x20, height),
        None => {
            g.fill(0, 0, LOGICAL_WIDTH, height as u16 + 10, 0);
            (16, 15)
        }
    };
    for (row, line) in lines.iter().enumerate() {
        g.text(ox, oy + row as i32 * 10, line, None);
    }
}

/// Map encounter dialog (`FUN_2920_0a12` -> `FUN_1772_000e`): stone window at
/// (10, 5), the sentence, then the outlined buttons.
fn draw_encounter(g: &mut Gfx, runtime: &Runtime) {
    let session = runtime.session();
    if !g.has_font() {
        return;
    }
    let buttons = runtime.encounter_buttons();
    let rects = runtime.encounter_button_rects();
    let selected = runtime.encounter_selection();
    let height = rects.last().map_or(40, |&(_, y, _)| i32::from(y) + 12);
    let (ox, oy) = match runtime.panel() {
        Some(_) => g.panel(session, 8, 5, 0x10e + 0x20, height),
        None => {
            g.fill(0, 0, LOGICAL_WIDTH, height as u16 + 5, 0);
            (16, 10)
        }
    };
    for (line, text) in runtime.encounter_text().split('\n').enumerate() {
        g.text(ox, oy + line as i32 * 10, text, None);
    }
    let label_width = buttons
        .iter()
        .map(|label| g.text_width(label))
        .max()
        .unwrap_or(0) as u16;
    for (index, (label, &(x, y, _))) in buttons.iter().zip(&rects).enumerate() {
        let (x, y) = (i32::from(x) + 3, i32::from(y) + 2);
        g.text(x, y, label, None);
        if let (Some(style), Some(_)) = (session.window_style(), runtime.panel()) {
            let ops = style.button(x as i16, y as i16, label_width, index == selected);
            g.window_ops(0, 0, &ops);
        }
    }
}

fn draw_world_map(g: &mut Gfx, _session: &Session, selected: Option<&str>, current: Option<&str>) {
    if !g.has_font() {
        return;
    }
    let selected = selected
        .filter(|name| !name.is_empty())
        .map_or_else(|| "NO LOCATIONS".to_string(), str::to_ascii_uppercase);
    // Keys are listed in the window title; the original map has no help line.
    let banner = match current.filter(|name| !name.is_empty()) {
        Some(current) => format!("{}   AT {}", selected, current.to_ascii_uppercase()),
        None => selected,
    };
    // The bottom strip carries the region name as part of the frame art
    // (INTF#501), so the port's location label sits on the empty top strip,
    // in the font's own colours like the original's frame text.
    let width = i32::try_from(g.text_width(&banner)).unwrap_or(0);
    g.text((160 - width / 2).clamp(24, 296), 3, &banner, None);
}

fn draw_menu(g: &mut Gfx, session: &Session, panel: Option<&soe_video::PlanarImage>) {
    g.fill(0, 0, LOGICAL_WIDTH, LOGICAL_HEIGHT, 0);
    if panel.is_some() {
        g.panel(session, 20, 40, 280, 96);
    }
    if !g.has_font() {
        return;
    }
    g.text(52, 58, "SPIRIT OF EXCALIBUR", Some(8));
    g.text(58, 78, "ENTER CONTINUE", None);
    g.text(70, 92, "ESC QUIT", None);
}

fn draw_game_lost(g: &mut Gfx, _session: &Session) {
    g.fill(0, 0, LOGICAL_WIDTH, LOGICAL_HEIGHT, 0);
    if !g.has_font() {
        return;
    }
    g.text(126, 90, "GAMELOST", Some(8));
    g.text(104, 110, "PRESS ENTER TO QUIT", Some(7));
}

fn draw_status(g: &mut Gfx, session: &Session, selected: usize, notice: &str) {
    g.fill(10, 18, 300, 168, 0);
    g.fill(10, 18, 300, 1, 7);
    g.fill(10, 185, 300, 1, 7);
    g.fill(10, 18, 1, 168, 7);
    g.fill(309, 18, 1, 168, 7);
    if !g.has_font() {
        return;
    }
    g.text(16, 22, "ROUND TABLE", Some(8));
    g.text(190, 22, "UP/DN KNIGHT  ENTER ITEMS", Some(8));
    g.text(16, 38, "NAME", Some(8));
    g.text(112, 38, "HP HT MV COM ARM NOB FTH GOLD", Some(8));
    g.fill(16, 47, 286, 1, 7);

    let indices: Vec<usize> = session
        .state
        .characters
        .iter()
        .enumerate()
        .filter(|(_, character)| !character.name().is_empty())
        .map(|(index, _)| index)
        .collect();
    if indices.is_empty() {
        g.text(16, 56, "NO KNIGHTS", Some(8));
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
        g.text(
            16,
            y,
            &format!("{marker}{}", sheet.name.to_ascii_uppercase()),
            Some(8),
        );
        g.text(112, y, &format!("{:2}", sheet.health), Some(8));
        g.text(134, y, &format!("{:2}", sheet.hits), Some(8));
        g.text(152, y, &format!("{:2}", sheet.movement), Some(8));
        g.text(170, y, &format!("{:3}", sheet.effective_combat), Some(8));
        g.text(194, y, &format!("{:3}", sheet.effective_armor), Some(8));
        g.text(220, y, &format!("{:3}", sheet.nobility), Some(8));
        g.text(244, y, &format!("{:3}", sheet.faith), Some(8));
        g.text(268, y, &format!("{:4}", sheet.gold), Some(8));
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
    g.text(16, 136, "GEAR:", Some(8));
    g.text(56, 136, &truncate(&gear, 34), Some(8));
    g.text(16, 150, "MAGIC:", Some(8));
    g.text(62, 150, &truncate(&spells, 33), Some(8));
    if !notice.is_empty() {
        g.text(16, 168, &truncate(notice, 38), Some(7));
    }
}

/// The duel's battle panel over the field scene, laid out like the
/// original's: the two sides' champions and the latest message.
fn draw_duel(
    g: &mut Gfx,
    session: &Session,
    panel: Option<&soe_video::PlanarImage>,
    duel: Option<&soe_engine::duel::Duel>,
) {
    let Some(duel) = duel else {
        return;
    };
    panel_or_fill(g, session, panel, 86);
    if !g.has_font() {
        return;
    }
    let name = |id: u8| {
        session
            .dgroup()
            .map(|_| soe_engine::names::display_name(duel.ds(), id))
            .unwrap_or_default()
    };
    g.text(78, 6, "ROUND TABLE...", None);
    g.text(84, 15, &name(duel.champions[0]), None);
    g.text(190, 6, "FOES...", None);
    g.text(196, 15, &name(duel.champions[1]), None);
    if let Some(message) = duel.messages.last() {
        g.text(24, 70, message, None);
    }
    if duel.finished() {
        g.text(112, 50, "PRESS ANY KEY", None);
    }
}

/// The army battle panel (`FUN_20b5_085b` with bit 0x10): unit rows
/// left (`DS:9C42`) and right (`DS:9C46`), the order column (`DS:9C3E`),
/// the latest message at the bottom, in stone windows around the field
/// (0x90, 0x50), which the session composes into the base frame.
fn draw_battle(g: &mut Gfx, session: &Session, battle: Option<&ArmyBattle>, order: usize, unit: usize) {
    g.panel(session, 0, 0, 0x90, 180);
    g.panel(session, 0x90, 0, LOGICAL_WIDTH as i32 - 0x90, 0x50);
    g.panel(session, 0, 180, LOGICAL_WIDTH as i32, 20);
    if !g.has_font() {
        return;
    }
    let Some(battle) = battle else {
        g.text(118, 90, "NO BATTLE", None);
        return;
    };
    let ds = battle.ds();
    let defending = u16::from_le_bytes([ds[0x9c56], ds[0x9c57]]) & 8 != 0;
    let (left, right) = if defending { ("DEFENDER...", "CHALLENGER...") } else { ("ROUND TABLE...", "FOES...") };
    g.text(6, 5, left, None);
    g.text(0xd4, 5, right, None);
    let row = |side: usize, (id, strength, slot): (u8, u16, u8)| {
        let rec = 0x2c5e + usize::from(id) * 0x38;
        let name: String = ds[rec..rec + 12].iter().take_while(|&&b| b != 0).map(|&b| char::from(b)).collect();
        let army = u16::from_le_bytes([ds[rec + 0x16], ds[rec + 0x17]]) & 0x8000 != 0;
        let text = if army {
            // "%d %S%c": the count, the unit and a plural s.
            format!("{strength} {name}{}", if strength == 1 { "" } else { "s" })
        } else if side == 0 || slot == 4 {
            soe_engine::names::display_name(ds, id)
        } else {
            name
        };
        (text, slot == 4)
    };
    let step = 10;
    for (i, u) in battle.units(0).into_iter().enumerate().take(16) {
        let (text, out) = row(0, u);
        let y = 15 + i as i32 * step;
        if i == unit {
            g.text(2, y, ">", None);
        }
        g.text(11, y, &truncate(&text, 20), if out { Some(4) } else { None });
    }
    for (i, u) in battle.units(1).into_iter().enumerate().take(6) {
        let (text, out) = row(1, u);
        g.text(0xd4 + 5, 15 + i as i32 * step, &truncate(&text, 16), if out { Some(4) } else { None });
    }
    for (i, o) in ArmyOrder::ALL.iter().enumerate() {
        let y = 5 + i as i32 * step;
        let label = o.label().to_ascii_uppercase();
        let colour = if battle.order_allowed(*o) { None } else { Some(8) };
        if i == order {
            g.text(0x90, y, ">", None);
        }
        g.text(0x96, y, &label, colour);
    }
    if let Some(message) = battle.messages.last() {
        g.text(6, 188, &truncate(&message.to_ascii_uppercase(), 52), None);
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

/// The intro's TITLE.RES images and the `DS:1FD9` table; `None` without them.
fn load_intro(game_dir: &Path, session: &Session) -> Option<soe_engine::intro::Intro> {
    let file = soe_res::ResFile::parse(std::fs::read(game_dir.join("RES32").join("TITLE.RES")).ok()?).ok()?;
    let mut images = std::collections::HashMap::new();
    for id in soe_engine::intro::image_ids() {
        let record = file.records().into_iter().find(|record| record.id == id)?;
        images.insert(id, decode_planar(&file.decode(record).ok()?).ok()?);
    }
    let table = session.palettes().table(soe_engine::palette::TITLE_DS)?;
    Some(soe_engine::intro::Intro::new(images, table, [0; 96]))
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
    let ui = UiAssets::new(
        load_record_image(&game_dir.join("RES32").join("TITLE.RES"), 200).ok(),
        std::fs::read(&exe)
            .ok()
            .and_then(|bytes| soe_engine::font::BitmapFont::metrics_from_excal(&bytes)),
        panel.clone(),
    );
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
    // The start-up intro (`FUN_2c74_0006`), unless a start screen is asked for.
    const START_FLAGS: [&str; 16] = [
        "--scene", "--map", "--status", "--story", "--travel", "--saves", "--actions", "--items",
        "--spells", "--shop", "--targets", "--paused", "--battle", "--duel", "--travel-to", "--no-intro",
    ];
    let mut start_events = Vec::new();
    if !args.iter().any(|arg| START_FLAGS.contains(&arg.as_str())) {
        if let Some(intro) = load_intro(&game_dir, runtime.session()) {
            start_events = runtime.start_intro(intro);
        }
    }
    if args.iter().any(|arg| arg == "--battle") {
        if !runtime.start_demo_battle() {
            eprintln!("no army records available for a battle");
        }
    }

    // Scripted travel for screenshots: select map marker K, start the
    // journey, then advance N 40 ms steps.
    let arg_value = |name: &str| {
        args.windows(2)
            .find(|pair| pair[0] == name)
            .and_then(|pair| pair[1].parse::<usize>().ok())
    };
    if let Some(marker) = arg_value("--travel-to") {
        for _ in 0..marker {
            runtime.handle_key(Key::Down);
        }
        runtime.handle_key(Key::Confirm);
    }
    // Single combat between two characters (1-based ids), e.g.
    // `--duel 1 62` for Constantine against Lupinus.
    if let Some(pair) = args.windows(3).find(|w| w[0] == "--duel") {
        if let (Ok(a), Ok(b)) = (pair[1].parse::<u8>(), pair[2].parse::<u8>()) {
            runtime.open_screen(Screen::Map);
            if !runtime.start_duel(a, b) {
                eprintln!("no live map for a duel");
            }
        }
    }
    if let Some(steps) = arg_value("--ticks") {
        for _ in 0..steps {
            for event in runtime.tick(Duration::from_millis(40)) {
                eprintln!("{event:?}");
            }
        }
    }

    if let Some(path) = args
        .windows(2)
        .find(|pair| pair[0] == "--screenshot")
        .map(|pair| PathBuf::from(&pair[1]))
    {
        let app = App::new(runtime, ui);
        let (frame, palette) = app.compose_frame();
        return write_screenshot(&path, &frame, &palette);
    }

    let music = MusicLibrary::load(game_dir.join("MUSIC.RES")).ok();
    let audio = audio::AudioPlayer::new();
    let song = start_events.iter().find_map(|event| match event {
        RuntimeEvent::Music(name) => music.as_ref().and_then(|m| m.find(name)),
        _ => None,
    });
    if let (Some(player), Some(song)) = (&audio, song.or_else(|| music.as_ref().and_then(MusicLibrary::map))) {
        player.play_score(&song.score);
    }

    let event_loop = EventLoop::new().context("creating event loop")?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::new(runtime, ui);
    app.audio = audio;
    app.music = music;
    event_loop.run_app(&mut app).context("running window")?;
    Ok(())
}

/// Write one logical frame as an indexed PNG (headless `--screenshot`).
fn write_screenshot(path: &Path, frame: &IndexedFrame, palette: &[[u8; 3]]) -> Result<()> {
    let mut pal = vec![0u8; 768];
    for (index, color) in palette.iter().enumerate().take(256) {
        pal[index * 3..index * 3 + 3].copy_from_slice(color);
    }
    let file =
        std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
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
