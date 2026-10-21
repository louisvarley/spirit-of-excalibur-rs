//! Main-loop state machine ported from `FUN_2d65_2cd9`.
//!
//! The original keeps a signed word at `DS:01EC` and treats every value `>=
//! -1` as an active frame. The negative values are the loop's own control
//! states (`-5` return to the menu, `-4` game lost, `-3` deferred screen
//! change, `-2` menu/idle, `-1` play). Everything the player sees is modelled
//! here as a [`Screen`], so the presentation layer never keeps its own mode
//! booleans.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use soe_video::{IndexedFrame, LOGICAL_WIDTH, PlanarImage};

use crate::economy::InventoryEntry;
use crate::interaction::{Dialogue, character_name};
use crate::session::Session;
use crate::story::HintBook;

/// One room animation frame (18.2 Hz).
pub const TICK: Duration = Duration::from_micros(54_945);
/// Screen-change fade duration used by the playable build.
pub const TRANSITION: Duration = Duration::from_millis(250);

/// Every screen the loop can present.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Screen {
    /// Main menu / idle path (original loop state `-2`).
    Menu,
    /// Playable room scene.
    Room,
    /// World map.
    Map,
    /// Round Table / inventory status.
    Status,
    /// Chronicle / hint story reader.
    Story,
    /// Location travel list.
    Travel,
    /// Save/load browser.
    Save,
    /// Pause panel.
    Pause,
    /// Room action menu.
    Action,
    /// Item selection panel.
    Item,
    /// Prepared-spell selection panel.
    Spell,
    /// Buy/sell shop panel.
    Shop,
    /// Target character selection panel.
    Target,
    /// Dialogue/event text with optional choices.
    Dialogue,
    /// `GAMELOST` screen (original loop state `-4`).
    GameLost,
    /// The loop has been asked to terminate.
    Quit,
}

impl Screen {
    /// Short label used by window titles and diagnostics.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Menu => "MENU",
            Self::Room => "ROOM",
            Self::Map => "MAP",
            Self::Status => "STATUS",
            Self::Story => "CHRONICLE",
            Self::Travel => "TRAVEL",
            Self::Save => "SAVES",
            Self::Pause => "PAUSED",
            Self::Action => "ACTIONS",
            Self::Item => "ITEMS",
            Self::Spell => "SPELLS",
            Self::Shop => "TRADE",
            Self::Target => "SELECT TARGET",
            Self::Dialogue => "DIALOGUE",
            Self::GameLost => "GAME LOST",
            Self::Quit => "QUIT",
        }
    }
}

/// Negative control states of the original loop word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LoopState {
    /// `-5`: close the active view, then fall through to the menu.
    ExitToMenu,
    /// `-4`: show `GAMELOST`.
    GameLost,
    /// `-3`: finish a deferred screen change / map action.
    ScreenChange,
    /// `-2`: menu or idle path.
    Menu,
    /// `-1`: normal play frame.
    Playing,
}

impl LoopState {
    /// The original `DS:01EC` value for this state.
    #[must_use]
    pub const fn code(self) -> i16 {
        match self {
            Self::ExitToMenu => -5,
            Self::GameLost => -4,
            Self::ScreenChange => -3,
            Self::Menu => -2,
            Self::Playing => -1,
        }
    }

    /// Interpret an original loop word. Values `>= -1` are all play frames.
    #[must_use]
    pub const fn from_code(code: i16) -> Option<Self> {
        match code {
            -5 => Some(Self::ExitToMenu),
            -4 => Some(Self::GameLost),
            -3 => Some(Self::ScreenChange),
            -2 => Some(Self::Menu),
            -1..=i16::MAX => Some(Self::Playing),
            _ => None,
        }
    }
}

/// Logical input routed by the runtime. The platform layer translates its own
/// keyboard/mouse/gamepad events into these values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Confirm,
    Cancel,
    Space,
    Map,
    Status,
    Story,
    Travel,
    Save,
    Pause,
    Action,
    QuickSave,
    QuickLoad,
    Mute,
}

/// Side effects the platform layer should act on after an input or tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEvent {
    Quit,
    MuteToggled,
    ScreenChanged(Screen),
    SceneChanged(u16),
    Saved(String),
    Loaded(String),
    GameLost,
    MenuEntered,
    Error(String),
}

/// The live game runtime: one session plus the original main-loop state.
pub struct Runtime {
    session: Session,
    screen: Screen,
    state: LoopState,
    scene_ids: Vec<u16>,
    scene_index: usize,
    game_dir: PathBuf,
    exe: PathBuf,
    map: Option<IndexedFrame>,
    panel: Option<PlanarImage>,
    story: Option<HintBook>,
    story_page: usize,
    travel_index: usize,
    save_files: Vec<String>,
    save_index: usize,
    action_index: usize,
    item_index: usize,
    item_action: usize,
    target_index: usize,
    target_action: usize,
    selected_character: usize,
    target_character: usize,
    shop_index: usize,
    shop_sell: bool,
    shop_seller: Option<usize>,
    notice: String,
    dialogue: Option<Dialogue>,
    transition_remaining: Option<Duration>,
    animation_accum: Duration,
}

impl Runtime {
    /// Wrap a loaded session in the main-loop state machine.
    #[must_use]
    pub fn new(
        session: Session,
        scene_ids: Vec<u16>,
        scene_index: usize,
        game_dir: impl Into<PathBuf>,
        exe: impl Into<PathBuf>,
    ) -> Self {
        let game_dir = game_dir.into();
        let selected_character = session.state.default_character();
        let mut runtime = Self {
            scene_ids,
            scene_index,
            session,
            screen: Screen::Room,
            state: LoopState::Playing,
            save_files: Vec::new(),
            game_dir,
            exe: exe.into(),
            map: None,
            panel: None,
            story: None,
            story_page: 0,
            travel_index: 0,
            save_index: 0,
            action_index: 0,
            item_index: 0,
            item_action: 0,
            target_index: 0,
            target_action: 0,
            selected_character,
            target_character: selected_character,
            shop_index: 0,
            shop_sell: false,
            shop_seller: None,
            notice: String::new(),
            dialogue: None,
            transition_remaining: None,
            animation_accum: Duration::ZERO,
        };
        runtime.save_files = list_saves(&runtime.game_dir);
        runtime.sync_scene_index();
        runtime
    }

    /// Empty runtime for headless state-machine tests.
    #[must_use]
    pub fn headless(scene_ids: Vec<u16>, scene_index: usize) -> Self {
        Self::new(
            Session::headless(),
            scene_ids,
            scene_index,
            PathBuf::new(),
            PathBuf::new(),
        )
    }

    /// Attach already-loaded presentation resources.
    #[must_use]
    pub fn with_assets(
        mut self,
        map: Option<IndexedFrame>,
        panel: Option<PlanarImage>,
        story: Option<HintBook>,
    ) -> Self {
        self.map = map;
        self.panel = panel;
        self.story = story;
        self
    }

    #[must_use]
    pub const fn screen(&self) -> Screen {
        self.screen
    }

    #[must_use]
    pub const fn state(&self) -> LoopState {
        self.state
    }

    /// The original loop word. Canonical for the five negative states; play
    /// frames report `-1`.
    #[must_use]
    pub const fn state_code(&self) -> i16 {
        self.state.code()
    }

    #[must_use]
    pub fn session(&self) -> &Session {
        &self.session
    }

    #[must_use]
    pub fn session_mut(&mut self) -> &mut Session {
        &mut self.session
    }

    #[must_use]
    pub fn map(&self) -> Option<&IndexedFrame> {
        self.map.as_ref()
    }

    #[must_use]
    pub fn panel(&self) -> Option<&PlanarImage> {
        self.panel.as_ref()
    }

    #[must_use]
    pub fn story(&self) -> Option<&HintBook> {
        self.story.as_ref()
    }

    #[must_use]
    pub const fn story_page(&self) -> usize {
        self.story_page
    }

    #[must_use]
    pub const fn travel_index(&self) -> usize {
        self.travel_index
    }

    #[must_use]
    pub fn save_files(&self) -> &[String] {
        &self.save_files
    }

    #[must_use]
    pub const fn save_index(&self) -> usize {
        self.save_index
    }

    #[must_use]
    pub const fn action_index(&self) -> usize {
        self.action_index
    }

    #[must_use]
    pub const fn item_index(&self) -> usize {
        self.item_index
    }

    #[must_use]
    pub const fn item_action(&self) -> usize {
        self.item_action
    }

    #[must_use]
    pub const fn target_index(&self) -> usize {
        self.target_index
    }

    #[must_use]
    pub const fn target_action(&self) -> usize {
        self.target_action
    }

    #[must_use]
    pub const fn selected_character(&self) -> usize {
        self.selected_character
    }

    #[must_use]
    pub const fn target_character(&self) -> usize {
        self.target_character
    }

    #[must_use]
    pub const fn shop_index(&self) -> usize {
        self.shop_index
    }

    #[must_use]
    pub const fn shop_sell(&self) -> bool {
        self.shop_sell
    }

    #[must_use]
    pub fn notice(&self) -> &str {
        &self.notice
    }

    #[must_use]
    pub fn dialogue(&self) -> Option<&Dialogue> {
        self.dialogue.as_ref()
    }

    /// Display name of the character whose action menu is open.
    #[must_use]
    pub fn actor_name(&self) -> String {
        character_name(&self.session.state, self.selected_character)
    }

    /// Characters selectable in the current room.
    #[must_use]
    pub fn room_characters(&self) -> Vec<usize> {
        self.session.state.room_characters(self.selected_character)
    }

    /// Names shown by the target panel.
    #[must_use]
    pub fn target_names(&self) -> Vec<String> {
        self.target_candidates()
            .into_iter()
            .map(|index| character_name(&self.session.state, index))
            .collect()
    }

    /// Names shown by the active item panel.
    #[must_use]
    pub fn item_names(&self) -> Vec<String> {
        self.item_entries()
            .into_iter()
            .map(|entry| entry.name)
            .collect()
    }

    /// Rows shown by the active item panel.
    #[must_use]
    pub fn item_entries(&self) -> Vec<InventoryEntry> {
        match self.item_action {
            0 => self.session.state.ground_items(self.session.scene_id),
            _ => self.session.state.inventory(self.selected_character),
        }
    }

    #[must_use]
    pub fn spell_entries(&self) -> Vec<InventoryEntry> {
        self.session.state.spells(self.selected_character)
    }

    /// Rows shown by the trade panel. Selling lists the player's inventory;
    /// buying lists merchant stock, falling back to the catalogue.
    #[must_use]
    pub fn shop_entries(&self) -> Vec<InventoryEntry> {
        if self.shop_sell {
            return self.session.state.inventory(self.selected_character);
        }
        let stock = self
            .shop_seller
            .map_or_else(Vec::new, |seller| self.session.state.shop_stock(seller));
        if stock.is_empty() {
            self.session.state.catalogue()
        } else {
            stock
        }
    }

    #[must_use]
    pub const fn scene_index(&self) -> usize {
        self.scene_index
    }

    #[must_use]
    pub const fn scene_count(&self) -> usize {
        self.scene_ids.len()
    }

    #[must_use]
    pub const fn scene_id(&self) -> u16 {
        self.session.scene_id
    }

    /// Fade progress for the presenter: `0.0` just after a change, `1.0` steady.
    #[must_use]
    pub fn fade(&self) -> f32 {
        self.transition_remaining.map_or(1.0, |remaining| {
            1.0 - remaining.as_secs_f32() / TRANSITION.as_secs_f32()
        })
    }

    /// Open a screen directly. `Menu`, `GameLost` and `Quit` select their
    /// matching loop state; every other screen is a play frame.
    pub fn open_screen(&mut self, screen: Screen) {
        match screen {
            Screen::Menu => self.show_menu(),
            Screen::GameLost => self.lose_game(),
            Screen::Quit => {
                self.state = LoopState::ExitToMenu;
                self.screen = Screen::Quit;
            }
            Screen::Room => {
                self.state = LoopState::Playing;
                self.screen = Screen::Room;
                self.start_transition();
            }
            Screen::Story => {
                self.state = LoopState::Playing;
                self.screen = Screen::Story;
                self.story_page = 0;
            }
            Screen::Travel => {
                self.state = LoopState::Playing;
                self.screen = Screen::Travel;
                self.clamp_travel_index();
            }
            Screen::Save => {
                self.state = LoopState::Playing;
                self.screen = Screen::Save;
                self.refresh_saves();
            }
            Screen::Action => {
                self.state = LoopState::Playing;
                self.screen = Screen::Action;
                self.action_index = 0;
            }
            Screen::Item => {
                self.open_item(0);
            }
            Screen::Spell => {
                self.open_spell();
            }
            Screen::Shop => {
                self.open_shop(None);
            }
            Screen::Target => {
                self.open_target(0);
            }
            _ => {
                self.state = LoopState::Playing;
                self.screen = screen;
            }
        }
    }

    pub fn open_item(&mut self, action: usize) {
        self.state = LoopState::Playing;
        self.screen = Screen::Item;
        self.item_index = 0;
        self.item_action = action;
        self.notice.clear();
        self.clamp_item_index();
    }

    pub fn open_spell(&mut self) {
        self.state = LoopState::Playing;
        self.screen = Screen::Spell;
        self.item_index = 0;
        self.notice.clear();
        self.clamp_item_index();
    }

    pub fn open_shop(&mut self, seller: Option<usize>) {
        self.state = LoopState::Playing;
        self.screen = Screen::Shop;
        self.shop_index = 0;
        self.shop_sell = false;
        self.shop_seller = seller.filter(|index| {
            *index < self.session.state.characters.len()
                && !self.session.state.shop_stock(*index).is_empty()
        });
        self.notice.clear();
        self.clamp_shop_index();
    }

    pub fn open_target(&mut self, action: usize) {
        self.state = LoopState::Playing;
        self.screen = Screen::Target;
        self.target_index = 0;
        self.target_action = action;
        self.update_target_character();
    }

    /// Select a room character and open their action menu (`FUN_1ba8_4604`).
    pub fn select_character(&mut self, index: usize) -> Vec<RuntimeEvent> {
        if index < self.session.state.characters.len() {
            self.selected_character = index;
            self.target_character = index;
        }
        self.open_screen(Screen::Action);
        vec![RuntimeEvent::ScreenChanged(Screen::Action)]
    }

    /// Start Talk (`FUN_1ba8_22ef` -> `FUN_26c5_071a`).
    pub fn talk_to(&mut self, target: usize) -> bool {
        let Some(dialogue) = Dialogue::talk(
            &self.session.state,
            self.story.as_ref(),
            self.selected_character,
            target,
        ) else {
            self.notice = format!(
                "{} has nothing to say.",
                character_name(&self.session.state, target)
            );
            return false;
        };
        self.target_character = target;
        self.dialogue = Some(dialogue);
        self.state = LoopState::Playing;
        self.screen = Screen::Dialogue;
        true
    }

    /// Original loop state `-2`: menu/idle path.
    pub fn show_menu(&mut self) {
        self.state = LoopState::Menu;
        self.screen = Screen::Menu;
        self.transition_remaining = None;
        self.animation_accum = Duration::ZERO;
    }

    /// Leave the menu and resume the current room.
    pub fn start_game(&mut self) {
        self.state = LoopState::Playing;
        self.screen = Screen::Room;
        self.transition_remaining = None;
        self.animation_accum = Duration::ZERO;
    }

    /// Original loop state `-5`: tear down the current view, then idle in the
    /// menu on the next frame.
    pub fn return_to_menu(&mut self) {
        self.state = LoopState::ExitToMenu;
        self.transition_remaining = None;
        self.animation_accum = Duration::ZERO;
    }

    /// Original loop state `-4`: show `GAMELOST`.
    pub fn lose_game(&mut self) {
        self.state = LoopState::GameLost;
        self.screen = Screen::GameLost;
        self.transition_remaining = None;
        self.animation_accum = Duration::ZERO;
    }

    /// Original loop state `-3`: deferred screen change (map/travel action).
    pub fn begin_screen_change(&mut self, screen: Screen) {
        self.state = LoopState::ScreenChange;
        self.screen = screen;
        self.start_transition();
    }

    fn start_transition(&mut self) {
        self.transition_remaining = Some(TRANSITION);
    }

    fn close_modal(&mut self) {
        self.state = LoopState::Playing;
        self.screen = Screen::Room;
        self.dialogue = None;
        self.transition_remaining = None;
    }

    /// Advance timers, resolve negative loop states, and animate the room.
    pub fn tick(&mut self, delta: Duration) -> Vec<RuntimeEvent> {
        let mut events = Vec::new();
        if let Some(remaining) = self.transition_remaining {
            let remaining = remaining.saturating_sub(delta);
            self.transition_remaining = (!remaining.is_zero()).then_some(remaining);
        }

        match self.state {
            LoopState::ExitToMenu => {
                self.state = LoopState::Menu;
                self.screen = Screen::Menu;
                self.transition_remaining = None;
                self.animation_accum = Duration::ZERO;
                events.push(RuntimeEvent::MenuEntered);
            }
            LoopState::ScreenChange => {
                if self.transition_remaining.is_none() {
                    self.state = LoopState::Playing;
                    events.push(RuntimeEvent::ScreenChanged(self.screen));
                }
            }
            LoopState::Playing => {
                if self.screen == Screen::Room && self.session.animation_count() > 0 {
                    self.animation_accum += delta;
                    let ticks = (self.animation_accum.as_micros() / TICK.as_micros())
                        .min(u128::from(u32::MAX)) as u32;
                    for _ in 0..ticks {
                        self.session.advance_animation_frame();
                    }
                    self.animation_accum = self.animation_accum.saturating_sub(TICK * ticks);
                } else {
                    self.animation_accum = Duration::ZERO;
                }
            }
            LoopState::GameLost | LoopState::Menu => {
                self.animation_accum = Duration::ZERO;
            }
        }
        events
    }

    /// Route one logical key to the active screen.
    pub fn handle_key(&mut self, key: Key) -> Vec<RuntimeEvent> {
        if key == Key::Mute {
            return vec![RuntimeEvent::MuteToggled];
        }
        match self.screen {
            Screen::Menu => match key {
                Key::Confirm | Key::Space => {
                    self.start_game();
                    vec![RuntimeEvent::ScreenChanged(Screen::Room)]
                }
                Key::Cancel => self.request_quit(),
                _ => Vec::new(),
            },
            Screen::Quit => self.request_quit(),
            Screen::GameLost => match key {
                Key::Confirm | Key::Cancel | Key::Space => {
                    self.state = LoopState::ExitToMenu;
                    self.screen = Screen::Quit;
                    vec![RuntimeEvent::Quit]
                }
                _ => Vec::new(),
            },
            Screen::Pause => {
                self.close_modal();
                vec![RuntimeEvent::ScreenChanged(Screen::Room)]
            }
            Screen::Map => match key {
                Key::Map | Key::Cancel | Key::Confirm => {
                    self.close_modal();
                    vec![RuntimeEvent::ScreenChanged(Screen::Room)]
                }
                _ => Vec::new(),
            },
            Screen::Status => match key {
                Key::Status | Key::Cancel => {
                    self.close_modal();
                    vec![RuntimeEvent::ScreenChanged(Screen::Room)]
                }
                Key::Up | Key::Left => {
                    self.move_character(-1);
                    Vec::new()
                }
                Key::Down | Key::Right => {
                    self.move_character(1);
                    Vec::new()
                }
                Key::Confirm | Key::Space => {
                    self.open_item(3);
                    vec![RuntimeEvent::ScreenChanged(Screen::Item)]
                }
                _ => Vec::new(),
            },
            Screen::Story => match key {
                Key::Story | Key::Cancel => {
                    self.close_modal();
                    vec![RuntimeEvent::ScreenChanged(Screen::Room)]
                }
                Key::Left | Key::Up => {
                    self.story_page = self.story_page.saturating_sub(1);
                    Vec::new()
                }
                Key::Right | Key::Down | Key::Space | Key::Confirm => {
                    self.story_page = self.story_page.wrapping_add(1);
                    Vec::new()
                }
                _ => Vec::new(),
            },
            Screen::Travel => match key {
                Key::Travel | Key::Cancel => {
                    self.close_modal();
                    vec![RuntimeEvent::ScreenChanged(Screen::Room)]
                }
                Key::Up | Key::Left => {
                    self.travel_index = self.travel_index.saturating_sub(1);
                    Vec::new()
                }
                Key::Down | Key::Right => {
                    self.travel_index = self.travel_index.saturating_add(1);
                    self.clamp_travel_index();
                    Vec::new()
                }
                Key::Confirm => self.travel_to_selected(),
                _ => Vec::new(),
            },
            Screen::Save => match key {
                Key::Save | Key::Cancel => {
                    self.close_modal();
                    vec![RuntimeEvent::ScreenChanged(Screen::Room)]
                }
                Key::Up => {
                    self.save_index = self.save_index.saturating_sub(1);
                    Vec::new()
                }
                Key::Down => {
                    if !self.save_files.is_empty() {
                        self.save_index = (self.save_index + 1).min(self.save_files.len() - 1);
                    }
                    Vec::new()
                }
                Key::Confirm => self.load_selected_save(),
                Key::QuickSave => self.save_quick(),
                _ => Vec::new(),
            },
            Screen::Action => match key {
                Key::Action | Key::Cancel => {
                    self.close_modal();
                    vec![RuntimeEvent::ScreenChanged(Screen::Room)]
                }
                Key::Left => {
                    self.action_index = self.action_index.saturating_sub(1);
                    Vec::new()
                }
                Key::Right => {
                    self.action_index = (self.action_index + 1).min(11);
                    Vec::new()
                }
                Key::Up => {
                    self.action_index = self.action_index.saturating_sub(4);
                    Vec::new()
                }
                Key::Down => {
                    self.action_index = (self.action_index + 4).min(11);
                    Vec::new()
                }
                Key::Confirm => {
                    if matches!(self.action_index, 2 | 5 | 9) {
                        self.open_target(self.action_index);
                        vec![RuntimeEvent::ScreenChanged(Screen::Target)]
                    } else if matches!(self.action_index, 0 | 1 | 3) {
                        self.open_item(self.action_index);
                        vec![RuntimeEvent::ScreenChanged(Screen::Item)]
                    } else if self.action_index == 7 {
                        self.open_spell();
                        vec![RuntimeEvent::ScreenChanged(Screen::Spell)]
                    } else {
                        self.close_modal();
                        vec![RuntimeEvent::ScreenChanged(Screen::Room)]
                    }
                }
                _ => Vec::new(),
            },
            Screen::Item => match key {
                Key::Cancel | Key::Action | Key::Save => {
                    self.close_modal();
                    vec![RuntimeEvent::ScreenChanged(Screen::Room)]
                }
                Key::Up | Key::Left => {
                    self.item_index = self.item_index.saturating_sub(1);
                    Vec::new()
                }
                Key::Down | Key::Right => {
                    self.item_index = self.item_index.saturating_add(1);
                    self.clamp_item_index();
                    Vec::new()
                }
                Key::Confirm | Key::Space => {
                    self.perform_item_action();
                    Vec::new()
                }
                _ => Vec::new(),
            },
            Screen::Spell => match key {
                Key::Cancel | Key::Action | Key::Save | Key::Status => {
                    self.close_modal();
                    vec![RuntimeEvent::ScreenChanged(Screen::Room)]
                }
                Key::Up | Key::Left => {
                    self.item_index = self.item_index.saturating_sub(1);
                    Vec::new()
                }
                Key::Down | Key::Right => {
                    self.item_index = self.item_index.saturating_add(1);
                    self.clamp_item_index();
                    Vec::new()
                }
                Key::Confirm | Key::Space => {
                    self.cast_selected_spell();
                    Vec::new()
                }
                _ => Vec::new(),
            },
            Screen::Shop => match key {
                Key::Cancel | Key::Action | Key::Save => {
                    self.close_modal();
                    vec![RuntimeEvent::ScreenChanged(Screen::Room)]
                }
                Key::Space | Key::Left | Key::Right => {
                    self.shop_sell = !self.shop_sell;
                    self.shop_index = 0;
                    self.clamp_shop_index();
                    Vec::new()
                }
                Key::Up => {
                    self.shop_index = self.shop_index.saturating_sub(1);
                    Vec::new()
                }
                Key::Down => {
                    self.shop_index = self.shop_index.saturating_add(1);
                    self.clamp_shop_index();
                    Vec::new()
                }
                Key::Confirm => {
                    self.perform_shop_action();
                    Vec::new()
                }
                _ => Vec::new(),
            },
            Screen::Dialogue => match key {
                Key::Cancel | Key::Action | Key::Story => {
                    self.close_modal();
                    vec![RuntimeEvent::ScreenChanged(Screen::Room)]
                }
                Key::Up | Key::Left => {
                    if let Some(dialogue) = self.dialogue.as_mut() {
                        dialogue.move_selection(-1);
                    }
                    Vec::new()
                }
                Key::Down | Key::Right => {
                    if let Some(dialogue) = self.dialogue.as_mut() {
                        dialogue.move_selection(1);
                    }
                    Vec::new()
                }
                Key::Confirm | Key::Space => {
                    if let Some(mut dialogue) = self.dialogue.take() {
                        if dialogue.has_choices() {
                            self.notice = dialogue.apply_selected(&mut self.session.state);
                        } else {
                            self.notice = dialogue.text;
                        }
                    }
                    self.close_modal();
                    vec![RuntimeEvent::ScreenChanged(Screen::Room)]
                }
                _ => Vec::new(),
            },
            Screen::Target => match key {
                Key::Cancel | Key::Action => {
                    self.close_modal();
                    vec![RuntimeEvent::ScreenChanged(Screen::Room)]
                }
                Key::Up | Key::Left => {
                    self.target_index = self.target_index.saturating_sub(1);
                    self.update_target_character();
                    Vec::new()
                }
                Key::Down | Key::Right => {
                    self.target_index = self.target_index.saturating_add(1);
                    self.update_target_character();
                    Vec::new()
                }
                Key::Confirm => {
                    if self.target_action == 2 {
                        if self.talk_to(self.target_character) {
                            vec![RuntimeEvent::ScreenChanged(Screen::Dialogue)]
                        } else {
                            self.close_modal();
                            vec![RuntimeEvent::ScreenChanged(Screen::Room)]
                        }
                    } else if self.target_action == 5 {
                        self.open_shop(Some(self.target_character));
                        vec![RuntimeEvent::ScreenChanged(Screen::Shop)]
                    } else if self.target_action == 9 {
                        self.open_item(9);
                        vec![RuntimeEvent::ScreenChanged(Screen::Item)]
                    } else {
                        self.close_modal();
                        vec![RuntimeEvent::ScreenChanged(Screen::Room)]
                    }
                }
                _ => Vec::new(),
            },
            Screen::Room => self.handle_room_key(key),
        }
    }

    fn handle_room_key(&mut self, key: Key) -> Vec<RuntimeEvent> {
        match key {
            Key::Cancel => self.request_quit(),
            Key::Map => {
                self.open_screen(Screen::Map);
                vec![RuntimeEvent::ScreenChanged(Screen::Map)]
            }
            Key::Status => {
                self.open_screen(Screen::Status);
                vec![RuntimeEvent::ScreenChanged(Screen::Status)]
            }
            Key::Story => {
                self.open_screen(Screen::Story);
                vec![RuntimeEvent::ScreenChanged(Screen::Story)]
            }
            Key::Travel => {
                self.open_screen(Screen::Travel);
                vec![RuntimeEvent::ScreenChanged(Screen::Travel)]
            }
            Key::Save => {
                self.open_screen(Screen::Save);
                vec![RuntimeEvent::ScreenChanged(Screen::Save)]
            }
            Key::Pause => {
                self.open_screen(Screen::Pause);
                vec![RuntimeEvent::ScreenChanged(Screen::Pause)]
            }
            Key::Action | Key::Confirm => {
                self.open_screen(Screen::Action);
                vec![RuntimeEvent::ScreenChanged(Screen::Action)]
            }
            Key::Left => self.change_scene(-1),
            Key::Right => self.change_scene(1),
            Key::Space => {
                self.session.advance_animation_frame();
                self.animation_accum = Duration::ZERO;
                Vec::new()
            }
            Key::QuickSave => self.save_quick(),
            Key::QuickLoad => self.load_named_save("QUICK.SAV"),
            _ => Vec::new(),
        }
    }

    /// Route a click in logical 320x200 coordinates.
    pub fn handle_click(&mut self, x: i16, y: i16) -> Vec<RuntimeEvent> {
        let left = i32::from(x) < i32::from(LOGICAL_WIDTH) / 2;
        match self.screen {
            Screen::Story => {
                if left {
                    self.story_page = self.story_page.saturating_sub(1);
                } else {
                    self.story_page = self.story_page.wrapping_add(1);
                }
                Vec::new()
            }
            Screen::Travel => {
                if left {
                    self.travel_index = self.travel_index.saturating_sub(1);
                } else {
                    self.travel_index = self.travel_index.saturating_add(1);
                    self.clamp_travel_index();
                }
                Vec::new()
            }
            Screen::Status => {
                self.move_character(if left { -1 } else { 1 });
                Vec::new()
            }
            Screen::Item | Screen::Spell => {
                if left {
                    self.item_index = self.item_index.saturating_sub(1);
                } else {
                    self.item_index = self.item_index.saturating_add(1);
                    self.clamp_item_index();
                }
                Vec::new()
            }
            Screen::Shop => {
                self.shop_sell = left;
                self.shop_index = 0;
                self.clamp_shop_index();
                Vec::new()
            }
            Screen::Target => {
                if left {
                    self.target_index = self.target_index.saturating_sub(1);
                } else {
                    self.target_index = self.target_index.saturating_add(1);
                }
                self.update_target_character();
                Vec::new()
            }
            Screen::Room => {
                if let Some(object) = self.session.object_at(x, y) {
                    let room = self.room_characters();
                    if !room.is_empty() {
                        let character = room[object % room.len()];
                        return self.select_character(character);
                    }
                }
                self.change_scene(if left { -1 } else { 1 })
            }
            _ => Vec::new(),
        }
    }

    fn character_indices(&self) -> Vec<usize> {
        self.session
            .state
            .characters
            .iter()
            .enumerate()
            .filter(|(_, character)| !character.name().is_empty())
            .map(|(index, _)| index)
            .collect()
    }

    fn move_character(&mut self, delta: isize) {
        let indices = self.character_indices();
        if indices.is_empty() {
            return;
        }
        let position = indices
            .iter()
            .position(|index| *index == self.selected_character)
            .unwrap_or(0);
        let next = (position as isize + delta).rem_euclid(indices.len() as isize) as usize;
        self.selected_character = indices[next];
        self.target_character = self.selected_character;
    }

    fn target_candidates(&self) -> Vec<usize> {
        let room: Vec<usize> = self
            .session
            .state
            .room_characters(self.selected_character)
            .into_iter()
            .filter(|index| *index != self.selected_character)
            .collect();
        if !room.is_empty() {
            return room;
        }
        self.character_indices()
            .into_iter()
            .filter(|index| *index != self.selected_character)
            .collect()
    }

    fn update_target_character(&mut self) {
        let candidates = self.target_candidates();
        if candidates.is_empty() {
            self.target_index = 0;
            self.target_character = self.selected_character;
            return;
        }
        self.target_index = self.target_index.min(candidates.len() - 1);
        self.target_character = candidates[self.target_index];
    }

    fn clamp_item_index(&mut self) {
        let len = match self.screen {
            Screen::Spell => self.spell_entries().len(),
            _ => self.item_entries().len(),
        };
        if len == 0 {
            self.item_index = 0;
        } else {
            self.item_index = self.item_index.min(len - 1);
        }
    }

    fn clamp_shop_index(&mut self) {
        let len = self.shop_entries().len();
        if len == 0 {
            self.shop_index = 0;
        } else {
            self.shop_index = self.shop_index.min(len - 1);
        }
    }

    fn perform_item_action(&mut self) {
        let entries = self.item_entries();
        let Some(entry) = entries.get(self.item_index).cloned() else {
            self.notice = "Nothing here.".to_string();
            return;
        };
        let result = match self.item_action {
            0 => self.session.state.pickup_item(
                self.selected_character,
                entry.slot,
                self.session.scene_id,
            ),
            1 => self
                .session
                .state
                .drop_item(self.selected_character, entry.def, self.session.scene_id)
                .map(|_| ()),
            3 => self
                .session
                .state
                .use_item(self.selected_character, entry.def, self.selected_character)
                .map(|outcome| {
                    self.notice = outcome.message;
                }),
            9 => self
                .session
                .state
                .give_item(self.selected_character, self.target_character, entry.def, 1)
                .map(|()| {
                    let name = self
                        .session
                        .state
                        .characters
                        .get(self.target_character)
                        .map_or_else(String::new, crate::save::Character::name);
                    self.notice = format!("Gave {} to {name}.", entry.name);
                }),
            _ => Ok(()),
        };
        match result {
            Ok(()) => {
                if self.item_action == 0 {
                    self.notice = format!("Picked up {}.", entry.name);
                } else if self.item_action == 1 {
                    self.notice = format!("Dropped {}.", entry.name);
                }
            }
            Err(error) => self.notice = error.to_string(),
        }
        self.clamp_item_index();
    }

    fn cast_selected_spell(&mut self) {
        let entries = self.spell_entries();
        let Some(entry) = entries.get(self.item_index).cloned() else {
            self.notice = "No magic prepared.".to_string();
            return;
        };
        match self.session.state.cast_spell(
            self.selected_character,
            entry.def,
            self.selected_character,
        ) {
            Ok(outcome) => self.notice = outcome.message,
            Err(error) => self.notice = error.to_string(),
        }
        self.clamp_item_index();
    }

    fn perform_shop_action(&mut self) {
        let entries = self.shop_entries();
        let Some(entry) = entries.get(self.shop_index).cloned() else {
            self.notice = "No stock.".to_string();
            return;
        };
        let result = if self.shop_sell {
            self.session
                .state
                .sell(self.selected_character, self.shop_seller, entry.def)
                .map(|price| format!("Sold {} for {price} gold.", entry.name))
        } else {
            self.session
                .state
                .buy(self.selected_character, self.shop_seller, entry.def)
                .map(|price| format!("Bought {} for {price} gold.", entry.name))
        };
        self.notice = result.unwrap_or_else(|error| error.to_string());
        self.clamp_shop_index();
    }

    fn request_quit(&mut self) -> Vec<RuntimeEvent> {
        self.state = LoopState::ExitToMenu;
        self.screen = Screen::Quit;
        vec![RuntimeEvent::Quit]
    }

    fn change_scene(&mut self, delta: isize) -> Vec<RuntimeEvent> {
        if self.scene_ids.is_empty() {
            return Vec::new();
        }
        let len = self.scene_ids.len() as isize;
        let next = (self.scene_index as isize + delta).rem_euclid(len) as usize;
        let id = self.scene_ids[next];
        match self.session.enter_scene(id) {
            Ok(()) => {
                self.scene_index = next;
                self.animation_accum = Duration::ZERO;
                self.start_transition();
                vec![RuntimeEvent::SceneChanged(id)]
            }
            Err(error) => vec![RuntimeEvent::Error(format!("scene {id}: {error}"))],
        }
    }

    fn sync_scene_index(&mut self) {
        if let Some(index) = self
            .scene_ids
            .iter()
            .position(|id| *id == self.session.scene_id)
        {
            self.scene_index = index;
        } else if self.scene_index >= self.scene_ids.len() {
            self.scene_index = self.scene_ids.len().saturating_sub(1);
        }
    }

    fn clamp_travel_index(&mut self) {
        let count = self.travel_locations().len();
        if count == 0 {
            self.travel_index = 0;
        } else {
            self.travel_index = self.travel_index.min(count - 1);
        }
    }

    fn travel_locations(&self) -> Vec<Option<u16>> {
        self.session
            .state
            .locations
            .iter()
            .filter(|location| !location.name().is_empty())
            .map(crate::save::Location::scene_id)
            .collect()
    }

    fn travel_to_selected(&mut self) -> Vec<RuntimeEvent> {
        let locations = self.travel_locations();
        let Some(Some(scene_id)) = locations.get(self.travel_index).copied() else {
            return Vec::new();
        };
        match self.session.enter_scene(scene_id) {
            Ok(()) => {
                if let Some(index) = self.scene_ids.iter().position(|id| *id == scene_id) {
                    self.scene_index = index;
                }
                self.animation_accum = Duration::ZERO;
                self.begin_screen_change(Screen::Room);
                vec![RuntimeEvent::SceneChanged(scene_id)]
            }
            Err(error) => vec![RuntimeEvent::Error(format!(
                "travel to scene {scene_id}: {error}"
            ))],
        }
    }

    pub fn refresh_saves(&mut self) {
        self.save_files = list_saves(&self.game_dir);
        if self.save_files.is_empty() {
            self.save_index = 0;
        } else {
            self.save_index = self.save_index.min(self.save_files.len() - 1);
        }
    }

    fn load_selected_save(&mut self) -> Vec<RuntimeEvent> {
        let Some(name) = self.save_files.get(self.save_index).cloned() else {
            return Vec::new();
        };
        self.load_named_save(&name)
    }

    fn load_named_save(&mut self, name: &str) -> Vec<RuntimeEvent> {
        let scene_id = self.session.scene_id;
        match Session::load_save(&self.game_dir, &self.exe, scene_id, name) {
            Ok(session) => {
                self.session = session;
                self.selected_character = self.session.state.default_character();
                self.target_character = self.selected_character;
                self.item_index = 0;
                self.shop_index = 0;
                self.shop_seller = None;
                self.notice.clear();
                self.dialogue = None;
                self.sync_scene_index();
                self.screen = Screen::Room;
                self.state = LoopState::Playing;
                self.animation_accum = Duration::ZERO;
                self.start_transition();
                vec![RuntimeEvent::Loaded(name.to_string())]
            }
            Err(error) => vec![RuntimeEvent::Error(format!("load {name}: {error}"))],
        }
    }

    fn save_quick(&mut self) -> Vec<RuntimeEvent> {
        let name = "QUICK.SAV";
        match self.session.save_to(&self.game_dir, name) {
            Ok(()) => {
                self.refresh_saves();
                if let Some(index) = self.save_files.iter().position(|file| file == name) {
                    self.save_index = index;
                }
                vec![RuntimeEvent::Saved(name.to_string())]
            }
            Err(error) => vec![RuntimeEvent::Error(format!("quick save: {error}"))],
        }
    }
}

fn list_saves(game_dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(game_dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.to_ascii_lowercase().ends_with(".sav"))
        .map(|name| name.to_ascii_uppercase())
        .collect();
    names.sort();
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_runtime() -> Runtime {
        Runtime::headless(vec![5, 6, 7], 0)
    }

    #[test]
    fn original_negative_states_map_to_loop_words() {
        assert_eq!(LoopState::ExitToMenu.code(), -5);
        assert_eq!(LoopState::GameLost.code(), -4);
        assert_eq!(LoopState::ScreenChange.code(), -3);
        assert_eq!(LoopState::Menu.code(), -2);
        assert_eq!(LoopState::Playing.code(), -1);
        assert_eq!(LoopState::from_code(-5), Some(LoopState::ExitToMenu));
        assert_eq!(LoopState::from_code(-4), Some(LoopState::GameLost));
        assert_eq!(LoopState::from_code(-3), Some(LoopState::ScreenChange));
        assert_eq!(LoopState::from_code(-2), Some(LoopState::Menu));
        assert_eq!(LoopState::from_code(-1), Some(LoopState::Playing));
        assert_eq!(LoopState::from_code(0), Some(LoopState::Playing));
        assert_eq!(LoopState::from_code(-6), None);
    }

    #[test]
    fn room_map_room_round_trip() {
        let mut runtime = make_runtime();
        assert_eq!(runtime.screen(), Screen::Room);
        assert_eq!(runtime.state_code(), -1);

        let events = runtime.handle_key(Key::Map);
        assert_eq!(events, vec![RuntimeEvent::ScreenChanged(Screen::Map)]);
        assert_eq!(runtime.screen(), Screen::Map);

        let events = runtime.handle_key(Key::Map);
        assert_eq!(events, vec![RuntimeEvent::ScreenChanged(Screen::Room)]);
        assert_eq!(runtime.screen(), Screen::Room);
        assert_eq!(runtime.state_code(), -1);
    }

    #[test]
    fn room_modal_screens_return_to_room() {
        for key in [
            Key::Status,
            Key::Story,
            Key::Travel,
            Key::Save,
            Key::Pause,
            Key::Action,
        ] {
            let mut runtime = make_runtime();
            let events = runtime.handle_key(key);
            assert_eq!(events.len(), 1);
            assert_ne!(runtime.screen(), Screen::Room);
            runtime.handle_key(Key::Cancel);
            assert_eq!(runtime.screen(), Screen::Room, "after {key:?}");
        }
    }

    #[test]
    fn action_routes_to_target_story_and_item() {
        let mut runtime = make_runtime();
        runtime.handle_key(Key::Action);
        runtime.handle_key(Key::Right);
        runtime.handle_key(Key::Right);
        assert_eq!(runtime.action_index(), 2);
        runtime.handle_key(Key::Confirm);
        assert_eq!(runtime.screen(), Screen::Target);
        runtime.handle_key(Key::Confirm);
        assert_eq!(runtime.screen(), Screen::Room);

        let mut runtime = make_runtime();
        runtime.handle_key(Key::Action);
        assert_eq!(runtime.action_index(), 0);
        runtime.handle_key(Key::Confirm);
        assert_eq!(runtime.screen(), Screen::Item);
        let events = runtime.handle_key(Key::Confirm);
        assert!(events.is_empty());
        assert_eq!(runtime.screen(), Screen::Item);
        assert_eq!(runtime.notice(), "Nothing here.");
        runtime.handle_key(Key::Cancel);
        assert_eq!(runtime.screen(), Screen::Room);
    }

    #[test]
    fn action_routes_to_spell_and_trade() {
        let mut runtime = make_runtime();
        runtime.handle_key(Key::Action);
        for _ in 0..7 {
            runtime.handle_key(Key::Right);
        }
        assert_eq!(runtime.action_index(), 7);
        runtime.handle_key(Key::Confirm);
        assert_eq!(runtime.screen(), Screen::Spell);
        runtime.handle_key(Key::Cancel);

        let mut runtime = make_runtime();
        runtime.handle_key(Key::Action);
        for _ in 0..5 {
            runtime.handle_key(Key::Right);
        }
        assert_eq!(runtime.action_index(), 5);
        runtime.handle_key(Key::Confirm);
        assert_eq!(runtime.screen(), Screen::Target);
        runtime.handle_key(Key::Confirm);
        assert_eq!(runtime.screen(), Screen::Shop);
        runtime.handle_key(Key::Space);
        assert!(runtime.shop_sell());
        runtime.handle_key(Key::Cancel);
        assert_eq!(runtime.screen(), Screen::Room);
    }

    #[test]
    fn status_is_interactive_and_keep_status_to_close() {
        let mut runtime = make_runtime();
        runtime.handle_key(Key::Status);
        assert_eq!(runtime.screen(), Screen::Status);
        runtime.handle_key(Key::Confirm);
        assert_eq!(runtime.screen(), Screen::Item);
        runtime.handle_key(Key::Cancel);
        assert_eq!(runtime.screen(), Screen::Room);
    }

    #[test]
    fn story_and_travel_click_routing() {
        let mut runtime = make_runtime();
        runtime.handle_key(Key::Story);
        assert_eq!(runtime.story_page(), 0);
        runtime.handle_click(20, 100);
        assert_eq!(runtime.story_page(), 0);
        runtime.handle_click(300, 100);
        assert_eq!(runtime.story_page(), 1);
        runtime.handle_key(Key::Left);
        assert_eq!(runtime.story_page(), 0);

        let mut runtime = make_runtime();
        runtime.handle_key(Key::Travel);
        runtime.handle_click(300, 100);
        assert_eq!(runtime.travel_index(), 0);
        runtime.handle_key(Key::Cancel);
        assert_eq!(runtime.screen(), Screen::Room);
    }

    #[test]
    fn negative_states_resolve_in_order() {
        let mut runtime = make_runtime();

        runtime.begin_screen_change(Screen::Room);
        assert_eq!(runtime.state_code(), -3);
        runtime.tick(TRANSITION);
        assert_eq!(runtime.state_code(), -1);

        runtime.return_to_menu();
        assert_eq!(runtime.state_code(), -5);
        runtime.tick(Duration::ZERO);
        assert_eq!(runtime.state_code(), -2);
        assert_eq!(runtime.screen(), Screen::Menu);

        runtime.handle_key(Key::Confirm);
        assert_eq!(runtime.state_code(), -1);
        assert_eq!(runtime.screen(), Screen::Room);

        runtime.lose_game();
        assert_eq!(runtime.state_code(), -4);
        assert_eq!(runtime.screen(), Screen::GameLost);
        let events = runtime.handle_key(Key::Confirm);
        assert_eq!(events, vec![RuntimeEvent::Quit]);
        assert_eq!(runtime.screen(), Screen::Quit);
    }
}
