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

use soe_video::{LOGICAL_WIDTH, PlanarImage};

use crate::battle::{Battle, BattleOutcome, Order};
use crate::economy::InventoryEntry;
use crate::encounter::EncounterDialog;
use crate::mapsim::{MapEvent, MapSim};
use crate::interaction::{Dialogue, character_name};
use crate::session::Session;
use crate::story::HintBook;
use crate::worldmap::{self, LocationMarker, WorldMap};

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
    /// Army battle screen.
    Battle,
    /// Dialogue/event text with optional choices.
    Dialogue,
    /// Map encounter dialog (`FUN_2920_0a12`).
    Encounter,
    /// Map message window (arrivals, burnings); any key continues.
    MapMessage,
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
            Self::Battle => "BATTLE",
            Self::Dialogue => "DIALOGUE",
            Self::Encounter => "ENCOUNTER",
            Self::MapMessage => "MAP",
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
    BattleEnded(BattleOutcome),
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
    map: Option<WorldMap>,
    panel: Option<PlanarImage>,
    story: Option<HintBook>,
    story_page: usize,
    /// Index into the filtered marker list shown on the map.
    travel_index: usize,
    /// 0-based row in the save location table.
    current_location: Option<usize>,
    map_camera: (i32, i32),
    map_region: u16,
    save_files: Vec<String>,
    save_index: usize,
    action_index: usize,
    item_index: usize,
    item_action: usize,
    target_index: usize,
    target_action: usize,
    selected_character: usize,
    target_character: usize,
    battle: Option<Battle>,
    battle_order_index: usize,
    last_battle_outcome: Option<BattleOutcome>,
    shop_index: usize,
    shop_sell: bool,
    shop_seller: Option<usize>,
    notice: String,
    dialogue: Option<Dialogue>,
    transition_remaining: Option<Duration>,
    animation_accum: Duration,
    /// Live regional map while the map is open.
    mapsim: Option<MapSim>,
    map_accum: Duration,
    encounter: Option<EncounterDialog>,
    encounter_index: usize,
    /// Messages waiting to be shown on the map, oldest first.
    map_messages: std::collections::VecDeque<String>,
    /// What to do once the messages are read: enter this marker / lose.
    pending_enter: Option<usize>,
    pending_lost: bool,
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
        let markers = worldmap::markers(&session.state);
        let party_position = session
            .state
            .characters
            .get(selected_character)
            .map_or((0, 0), |character| {
                (character.world_x(), character.world_y())
            });
        let current_location =
            worldmap::nearest_location(&markers, party_position.0, party_position.1);
        let travel_index = current_location
            .and_then(|location| {
                markers
                    .iter()
                    .position(|marker| marker.location_index == location)
            })
            .unwrap_or(0);
        let map_camera = markers.get(travel_index).map_or((0, 0), |marker| {
            worldmap::camera_for(marker.world_x, marker.world_y)
        });
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
            travel_index,
            current_location,
            map_camera,
            map_region: 1,
            save_index: 0,
            action_index: 0,
            item_index: 0,
            item_action: 0,
            target_index: 0,
            target_action: 0,
            selected_character,
            target_character: selected_character,
            battle: None,
            battle_order_index: 0,
            last_battle_outcome: None,
            shop_index: 0,
            shop_sell: false,
            shop_seller: None,
            notice: String::new(),
            dialogue: None,
            transition_remaining: None,
            animation_accum: Duration::ZERO,
            mapsim: None,
            map_accum: Duration::ZERO,
            encounter: None,
            encounter_index: 0,
            map_messages: std::collections::VecDeque::new(),
            pending_enter: None,
            pending_lost: false,
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
        map: Option<WorldMap>,
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
    pub fn map(&self) -> Option<&WorldMap> {
        self.map.as_ref()
    }

    /// Travel destinations from the location table, in save-file order.
    #[must_use]
    pub fn map_markers(&self) -> Vec<LocationMarker> {
        worldmap::markers(&self.session.state)
    }

    /// Camera top-left in world pixels.
    #[must_use]
    pub const fn map_camera(&self) -> (i32, i32) {
        self.map_camera
    }

    /// 0-based row of the party's current location in the save table.
    #[must_use]
    pub const fn current_location(&self) -> Option<usize> {
        self.current_location
    }

    #[must_use]
    pub fn current_location_name(&self) -> Option<String> {
        self.current_location
            .and_then(|index| self.session.state.locations.get(index))
            .map(crate::save::Location::name)
            .filter(|name| !name.is_empty())
    }

    /// Location row highlighted on the map/travel screen.
    #[must_use]
    pub fn selected_location(&self) -> Option<usize> {
        self.map_markers()
            .get(self.travel_index)
            .map(|marker| marker.location_index)
    }

    #[must_use]
    pub fn selected_location_name(&self) -> Option<String> {
        self.map_markers()
            .get(self.travel_index)
            .map(|marker| marker.name.clone())
    }

    /// Region prefix used by the original scene loader (`region * 100 + n`).
    pub fn set_map_region(&mut self, region: u16) {
        self.map_region = region.max(1);
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

    #[must_use]
    pub fn battle(&self) -> Option<&Battle> {
        self.battle.as_ref()
    }

    #[must_use]
    pub fn battle_order_index(&self) -> usize {
        self.battle_order_index
    }

    /// Orders shown by the battle screen.
    #[must_use]
    pub fn battle_orders(&self) -> &'static [Order] {
        self.battle
            .as_ref()
            .map_or(&crate::battle::BATTLE_ORDERS, Battle::orders)
    }

    #[must_use]
    pub const fn last_battle_outcome(&self) -> Option<BattleOutcome> {
        self.last_battle_outcome
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

    /// Names shown by the active item panel. Duplicate instances were grouped
    /// by the `1ba8` inventory list, so show their count and equipped state.
    #[must_use]
    pub fn item_names(&self) -> Vec<String> {
        self.item_entries()
            .into_iter()
            .map(|entry| {
                let mut name = entry.name;
                if entry.quantity > 1 {
                    name.push_str(&format!(" x{}", entry.quantity));
                }
                if entry.equipped {
                    name.push_str(" [equipped]");
                }
                name
            })
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
            Screen::Map => self.open_map(Screen::Map),
            Screen::Travel => self.open_map(Screen::Travel),
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
            Screen::Battle => {
                if self.battle.is_none() {
                    let _ = self.start_demo_battle();
                } else {
                    self.state = LoopState::Playing;
                    self.screen = Screen::Battle;
                    self.battle_order_index = 0;
                }
            }
            _ => {
                self.state = LoopState::Playing;
                self.screen = screen;
            }
        }
    }

    /// Open the regional map/travel view centred on the current location.
    pub fn open_map(&mut self, screen: Screen) {
        self.state = LoopState::Playing;
        self.screen = screen;
        self.select_current_location();
        self.center_map_on_selected();
        self.ensure_mapsim();
    }

    /// Build the live map from the save state (`FUN_2d65_0d39`).
    fn ensure_mapsim(&mut self) {
        if self.mapsim.is_some() {
            return;
        }
        let (Some(ds), Some(map)) = (self.session.dgroup(), &self.map) else {
            return;
        };
        let save = self.session.state.to_bytes();
        self.mapsim = MapSim::new(ds.to_vec(), &save, map.tilemap(), worldmap::TILES_W as u16);
        self.map_accum = Duration::ZERO;
    }

    /// Write the live map (positions, targets, clock) back into the save
    /// state and drop it.
    fn commit_mapsim(&mut self) {
        let Some(sim) = self.mapsim.take() else {
            return;
        };
        let mut bytes = self.session.state.to_bytes();
        sim.write_back(&mut bytes);
        if let Ok(state) = crate::save::GameState::parse(&bytes) {
            self.session.state = state;
        }
    }

    /// Runtime id of the party's map entity: the selected character when it
    /// is on the map, otherwise the first party-side (flag 0x200) entity.
    fn party_id(&self) -> u8 {
        let selected = u8::try_from(self.selected_character + 1).unwrap_or(1);
        match &self.mapsim {
            Some(sim) if sim.entity(selected).is_none() => sim.party().unwrap_or(selected),
            _ => selected,
        }
    }

    /// Map entities to draw: `(icon, world x, world y)`.
    #[must_use]
    pub fn map_entities(&self) -> Vec<(u16, i16, i16)> {
        self.mapsim.as_ref().map_or_else(Vec::new, |sim| {
            sim.entities()
                .iter()
                .filter(|e| e.0[0x26] & 0x80 == 0)
                .map(|e| (sim.icon(e.id() as u8), e.x(), e.y()))
                .collect()
        })
    }

    /// The open encounter dialog and the highlighted button.
    #[must_use]
    pub fn encounter(&self) -> Option<(EncounterDialog, usize)> {
        self.encounter.map(|d| (d, self.encounter_index))
    }

    /// Button labels of the open encounter, in order.
    #[must_use]
    pub fn encounter_buttons(&self) -> Vec<String> {
        let (Some(d), Some(sim)) = (self.encounter, &self.mapsim) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if d.talk {
            out.push(crate::names::talk_label(sim.ds(), d.second));
        }
        if d.join {
            out.push("Join Forces".to_string());
        }
        if d.choice {
            out.push("Continue on".to_string());
        }
        out
    }

    /// Encounter sentence, as the original formats it.
    #[must_use]
    pub fn encounter_text(&self) -> String {
        match (self.encounter, &self.mapsim) {
            (Some(d), Some(sim)) => crate::names::encounter_sentence(sim.ds(), &d),
            _ => String::new(),
        }
    }

    fn advance_map(&mut self, delta: Duration) -> Vec<RuntimeEvent> {
        const ROUND: Duration = Duration::from_millis(40);
        let id = self.party_id();
        let Some(sim) = self.mapsim.as_ref() else {
            return Vec::new();
        };
        if !sim.moving(id) {
            self.map_accum = Duration::ZERO;
            return Vec::new();
        }
        self.map_accum += delta;
        let mut out = Vec::new();
        while self.map_accum >= ROUND {
            self.map_accum -= ROUND;
            let Some(sim) = self.mapsim.as_mut() else {
                break;
            };
            let events = sim.round();
            if let Some(e) = sim.entity(id) {
                self.map_camera = worldmap::camera_for(e.x() as u16, e.y() as u16);
            }
            let mut stop = false;
            for event in events {
                match event {
                    MapEvent::Arrived {
                        id: who,
                        location,
                        messages,
                        game_lost,
                    } => {
                        self.map_messages.extend(messages);
                        self.pending_lost |= game_lost;
                        if who == id {
                            stop = true;
                            let row = usize::from(location).wrapping_sub(1);
                            self.pending_enter = self
                                .map_markers()
                                .iter()
                                .position(|m| location != 0 && m.location_index == row);
                        }
                    }
                    MapEvent::Encounter(dialog) => {
                        self.encounter = Some(dialog);
                        self.encounter_index = 0;
                        self.screen = Screen::Encounter;
                        self.map_accum = Duration::ZERO;
                        out.push(RuntimeEvent::ScreenChanged(Screen::Encounter));
                        return out;
                    }
                }
            }
            if !self.map_messages.is_empty() {
                self.map_accum = Duration::ZERO;
                self.screen = Screen::MapMessage;
                out.push(RuntimeEvent::ScreenChanged(Screen::MapMessage));
                return out;
            }
            if stop || self.pending_lost {
                self.map_accum = Duration::ZERO;
                out.extend(self.finish_map_messages());
                return out;
            }
        }
        out
    }

    /// The message shown by the map message window.
    #[must_use]
    pub fn map_message(&self) -> Option<&str> {
        self.map_messages.front().map(String::as_str)
    }

    /// Dismiss the current map message; after the last one, do what the
    /// arrival left pending.
    fn next_map_message(&mut self) -> Vec<RuntimeEvent> {
        self.map_messages.pop_front();
        if self.map_messages.is_empty() {
            self.screen = Screen::Map;
            let mut out = vec![RuntimeEvent::ScreenChanged(Screen::Map)];
            out.extend(self.finish_map_messages());
            out
        } else {
            Vec::new()
        }
    }

    fn finish_map_messages(&mut self) -> Vec<RuntimeEvent> {
        if self.pending_lost {
            self.pending_lost = false;
            self.commit_mapsim();
            self.lose_game();
            return vec![RuntimeEvent::GameLost];
        }
        if let Some(index) = self.pending_enter.take() {
            self.travel_index = index;
            return self.travel_to_selected();
        }
        Vec::new()
    }

    /// Send the party to the selected marker; enter it at once when the party
    /// is already there or the live map is unavailable.
    fn travel_request(&mut self) -> Vec<RuntimeEvent> {
        let markers = self.map_markers();
        let Some(marker) = markers.get(self.travel_index).cloned() else {
            return Vec::new();
        };
        if self.current_location == Some(marker.location_index) || self.mapsim.is_none() {
            return self.travel_to_selected();
        }
        let id = self.party_id();
        let started = self
            .mapsim
            .as_mut()
            .is_some_and(|sim| sim.set_target(id, marker.world_x as i16, marker.world_y as i16));
        if started {
            self.current_location = None;
            Vec::new()
        } else {
            self.travel_to_selected()
        }
    }

    fn handle_encounter_key(&mut self, key: Key) -> Vec<RuntimeEvent> {
        let count = self.encounter_buttons().len();
        match key {
            Key::Up | Key::Left => {
                self.encounter_index = self.encounter_index.saturating_sub(1);
                Vec::new()
            }
            Key::Down | Key::Right => {
                self.encounter_index = (self.encounter_index + 1).min(count.saturating_sub(1));
                Vec::new()
            }
            Key::Confirm | Key::Space => self.activate_encounter(self.encounter_index),
            Key::Cancel => self.activate_encounter(count.saturating_sub(1)),
            _ => Vec::new(),
        }
    }

    /// Logical layout of the encounter buttons: `(x, y, width)` per row.
    #[must_use]
    pub fn encounter_button_rects(&self) -> Vec<(i16, i16, i16)> {
        (0..self.encounter_buttons().len())
            .map(|i| (40, 34 + 16 * i as i16, 200))
            .collect()
    }

    fn activate_encounter(&mut self, index: usize) -> Vec<RuntimeEvent> {
        let Some(dialog) = self.encounter.take() else {
            return Vec::new();
        };
        let mut kinds = Vec::new();
        if dialog.talk {
            kinds.push("talk");
        }
        if dialog.join {
            kinds.push("join");
        }
        if dialog.choice {
            kinds.push("continue");
        }
        self.screen = Screen::Map;
        match kinds.get(index).copied() {
            Some("talk") => {
                self.commit_mapsim();
                let target = usize::from(dialog.second).saturating_sub(1);
                if self.talk_to(target) {
                    return vec![RuntimeEvent::ScreenChanged(Screen::Dialogue)];
                }
                self.ensure_mapsim();
            }
            Some("join") => self.notice = "Join Forces is not ported yet.".to_string(),
            _ => {}
        }
        vec![RuntimeEvent::ScreenChanged(Screen::Map)]
    }

    fn select_current_location(&mut self) {
        let markers = self.map_markers();
        if markers.is_empty() {
            self.travel_index = 0;
            return;
        }
        if let Some(current) = self.current_location {
            if let Some(index) = markers
                .iter()
                .position(|marker| marker.location_index == current)
            {
                self.travel_index = index;
                return;
            }
        }
        self.travel_index = self.travel_index.min(markers.len() - 1);
    }

    fn center_map_on_selected(&mut self) {
        let markers = self.map_markers();
        if let Some(marker) = markers.get(self.travel_index) {
            self.map_camera = worldmap::camera_for(marker.world_x, marker.world_y);
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

    /// Start a battle from save-state character indices (0-based). Returns
    /// false when either side has no named records.
    pub fn start_battle(&mut self, player: &[usize], foe: &[usize]) -> bool {
        self.start_battle_with_seed(player, foe, battle_seed())
    }

    /// Start a battle with an explicit RNG seed for reproducible tests.
    pub fn start_battle_with_seed(&mut self, player: &[usize], foe: &[usize], seed: u32) -> bool {
        let Some(battle) = Battle::from_state(&self.session.state, player, foe, seed) else {
            self.notice = "No army is ready for battle.".to_string();
            return false;
        };
        self.battle = Some(battle);
        self.battle_order_index = 0;
        self.last_battle_outcome = None;
        self.notice.clear();
        self.state = LoopState::Playing;
        self.screen = Screen::Battle;
        self.transition_remaining = None;
        true
    }

    /// Open the captured Round Table/Foes battle with the live save state.
    /// This is the temporary entry point until the encounter/event table is
    /// reversed; the battle engine itself only needs record indices.
    pub fn start_demo_battle(&mut self) -> bool {
        let characters = &self.session.state.characters;
        let mut foe = Vec::new();
        if let Some(start) = characters
            .iter()
            .position(|character| character.name().eq_ignore_ascii_case("Melehan"))
        {
            foe.push(start);
            for (index, character) in characters.iter().enumerate().skip(start + 1) {
                if foe.len() >= 4 {
                    break;
                }
                if !character.name().is_empty()
                    && character.max_strength() > 1
                    && character.current_strength() > 0
                {
                    foe.push(index);
                }
            }
        } else {
            for (index, character) in characters.iter().enumerate().rev() {
                if foe.len() >= 4 {
                    break;
                }
                if !character.name().is_empty()
                    && character.max_strength() > 1
                    && character.current_strength() > 0
                {
                    foe.push(index);
                }
            }
        }
        if foe.is_empty() {
            for (index, character) in characters.iter().enumerate().rev() {
                if foe.len() >= 4 {
                    break;
                }
                if !character.name().is_empty() {
                    foe.push(index);
                }
            }
        }

        let mut player = Vec::new();
        for (index, character) in characters.iter().enumerate() {
            if player.len() >= 10 {
                break;
            }
            if character.name().is_empty() || foe.contains(&index) {
                continue;
            }
            if character.max_strength() <= 1 && (character.is_present() || index == 0) {
                player.push(index);
            }
        }
        for (index, character) in characters.iter().enumerate() {
            if player.len() >= 18 {
                break;
            }
            if character.name().is_empty()
                || character.max_strength() <= 1
                || character.current_strength() == 0
                || foe.contains(&index)
                || player.contains(&index)
            {
                continue;
            }
            player.push(index);
        }
        if player.is_empty() {
            if let Some(index) = characters
                .iter()
                .position(|character| !character.name().is_empty())
            {
                player.push(index);
            }
        }
        self.start_battle_with_seed(&player, &foe, battle_seed())
    }

    fn handle_battle_key(&mut self, key: Key) -> Vec<RuntimeEvent> {
        let Some(order_count) = self.battle.as_ref().map(|battle| battle.orders().len()) else {
            self.screen = Screen::Room;
            return vec![RuntimeEvent::ScreenChanged(Screen::Room)];
        };
        if order_count == 0 {
            return Vec::new();
        }
        match key {
            Key::Up | Key::Left => {
                self.battle_order_index = (self.battle_order_index + order_count - 1) % order_count;
                Vec::new()
            }
            Key::Down | Key::Right => {
                self.battle_order_index = (self.battle_order_index + 1) % order_count;
                Vec::new()
            }
            Key::Confirm | Key::Space | Key::Cancel => {
                let order = if key == Key::Cancel {
                    Order::Retreat
                } else {
                    let Some(battle) = self.battle.as_ref() else {
                        return Vec::new();
                    };
                    battle.orders()[self.battle_order_index.min(order_count - 1)]
                };
                let finished = {
                    let Some(battle) = self.battle.as_mut() else {
                        return Vec::new();
                    };
                    match order {
                        Order::Retreat | Order::Withdraw => battle.withdraw(),
                        Order::Automatic => {
                            battle.set_player_order(Order::Engage);
                            battle.auto_resolve();
                        }
                        Order::Ignore => {}
                        _ => {
                            battle.set_player_order(order);
                            battle.resolve_round();
                        }
                    }
                    battle.is_over()
                };
                if finished {
                    let mut events = Vec::new();
                    self.finish_battle(&mut events);
                    events
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
        }
    }

    fn finish_battle(&mut self, events: &mut Vec<RuntimeEvent>) {
        let Some(battle) = self.battle.take() else {
            return;
        };
        let outcome = battle.outcome.unwrap_or(BattleOutcome::Withdrawn);
        battle.apply_to_state(&mut self.session.state);
        self.last_battle_outcome = Some(outcome);
        self.notice = match outcome {
            BattleOutcome::Victory => "The foe is routed. The army returns to the map.".to_string(),
            BattleOutcome::Defeat => "The army is routed and falls back to the map.".to_string(),
            BattleOutcome::Withdrawn => "The army withdraws to the map.".to_string(),
        };
        self.battle_order_index = 0;
        self.screen = Screen::Map;
        self.state = LoopState::Playing;
        self.refresh_location_from_party();
        self.start_transition();
        events.push(RuntimeEvent::BattleEnded(outcome));
        events.push(RuntimeEvent::ScreenChanged(Screen::Map));
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
                if matches!(self.screen, Screen::Map | Screen::Travel) {
                    events.extend(self.advance_map(delta));
                }
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
            Screen::Map | Screen::Travel => self.handle_map_key(key),
            Screen::Encounter => self.handle_encounter_key(key),
            Screen::MapMessage => self.next_map_message(),
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
            Screen::Battle => self.handle_battle_key(key),
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

    fn handle_map_key(&mut self, key: Key) -> Vec<RuntimeEvent> {
        match key {
            Key::Map | Key::Travel | Key::Cancel => {
                self.commit_mapsim();
                self.close_modal();
                vec![RuntimeEvent::ScreenChanged(Screen::Room)]
            }
            Key::Up | Key::Left => {
                self.move_travel(-1);
                Vec::new()
            }
            Key::Down | Key::Right => {
                self.move_travel(1);
                Vec::new()
            }
            Key::Confirm | Key::Space => self.travel_request(),
            _ => Vec::new(),
        }
    }

    fn move_travel(&mut self, delta: isize) {
        let count = self.map_markers().len();
        if count == 0 {
            self.travel_index = 0;
            return;
        }
        self.travel_index =
            (self.travel_index as isize + delta).rem_euclid(count as isize) as usize;
        self.center_map_on_selected();
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
            Screen::Map | Screen::Travel => self.click_map(x, y),
            Screen::MapMessage => self.next_map_message(),
            Screen::Encounter => {
                let hit = self.encounter_button_rects().iter().position(|&(bx, by, bw)| {
                    (bx..bx + bw).contains(&x) && (by..by + 12).contains(&y)
                });
                match hit {
                    Some(index) => self.activate_encounter(index),
                    None => Vec::new(),
                }
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
            Screen::Battle => {
                let count = self.battle_orders().len();
                if count > 0 {
                    let row = usize::try_from((i32::from(y) - 18).max(0) / 10).unwrap_or(0);
                    self.battle_order_index = row.min(count - 1);
                }
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

    fn click_map(&mut self, x: i16, y: i16) -> Vec<RuntimeEvent> {
        let markers = self.map_markers();
        let hit = if let Some(map) = &self.map {
            map.marker_at(self.map_camera, &markers, x, y)
        } else {
            let px = i32::from(x);
            let py = i32::from(y);
            markers
                .iter()
                .min_by_key(|marker| {
                    let dx = worldmap::VIEW_X + i32::from(marker.world_x) - self.map_camera.0 - px;
                    let dy = worldmap::VIEW_Y + i32::from(marker.world_y) - self.map_camera.1 - py;
                    dx * dx + dy * dy
                })
                .filter(|marker| {
                    let dx = worldmap::VIEW_X + i32::from(marker.world_x) - self.map_camera.0 - px;
                    let dy = worldmap::VIEW_Y + i32::from(marker.world_y) - self.map_camera.1 - py;
                    dx * dx + dy * dy <= 24 * 24
                })
                .map(|marker| marker.location_index)
        };
        let Some(location) = hit else {
            return Vec::new();
        };
        if let Some(index) = markers
            .iter()
            .position(|marker| marker.location_index == location)
        {
            self.travel_index = index;
            return self.travel_request();
        }
        Vec::new()
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
        match self.session.state.cast_spell_slot(
            self.selected_character,
            entry.slot,
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

    fn refresh_location_from_party(&mut self) {
        let markers = self.map_markers();
        let position = self
            .session
            .state
            .characters
            .get(self.selected_character)
            .map_or((0, 0), |character| {
                (character.world_x(), character.world_y())
            });
        self.current_location = worldmap::nearest_location(&markers, position.0, position.1);
        if let Some(current) = self.current_location {
            if let Some(index) = markers
                .iter()
                .position(|marker| marker.location_index == current)
            {
                self.travel_index = index;
                self.map_camera =
                    worldmap::camera_for(markers[index].world_x, markers[index].world_y);
            }
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

    fn travel_to_selected(&mut self) -> Vec<RuntimeEvent> {
        self.commit_mapsim();
        let markers = self.map_markers();
        let Some(marker) = markers.get(self.travel_index).cloned() else {
            return Vec::new();
        };
        let Some(scene_id) = worldmap::resolve_scene_id(
            marker.location_index,
            marker.icon_id,
            self.map_region,
            |id| self.scene_ids.contains(&id),
        ) else {
            return vec![RuntimeEvent::Error(format!(
                "travel to {}: no scene record",
                marker.name
            ))];
        };
        match self.session.enter_scene(scene_id) {
            Ok(()) => {
                if let Some(index) = self.scene_ids.iter().position(|id| *id == scene_id) {
                    self.scene_index = index;
                }
                self.current_location = Some(marker.location_index);
                self.map_camera = worldmap::camera_for(marker.world_x, marker.world_y);
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
                self.refresh_location_from_party();
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

fn battle_seed() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(1, |duration| {
            u32::try_from(duration.as_nanos() & u128::from(u32::MAX)).unwrap_or(1) | 1
        })
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
    use crate::save::{CHAR_SIZE, Character, ITEM_SIZE, Item};

    fn make_runtime() -> Runtime {
        Runtime::headless(vec![5, 6, 7], 0)
    }

    /// Headless runtime with two named characters and the item/spell
    /// definitions used by the interactive screen tests.
    fn economy_runtime() -> Runtime {
        let mut runtime = Runtime::headless(vec![5], 0);
        runtime.session.scene_id = 5;
        let state = &mut runtime.session.state;
        let mut hero = Character {
            raw: [0; CHAR_SIZE],
        };
        hero.set_name("Hero");
        hero.set_stat(Character::HEALTH, 10);
        hero.set_stat(Character::HITS, 12);
        hero.set_stat(Character::COMBAT, 7);
        hero.set_stat(Character::ARMOR, 5);
        hero.set_stat(Character::MAGIC, 9);
        hero.set_gold(100);
        state.characters[0] = hero;
        let mut friend = Character {
            raw: [0; CHAR_SIZE],
        };
        friend.set_name("Friend");
        friend.set_stat(Character::HEALTH, 20);
        state.characters[1] = friend;

        state.items[0] = Item {
            raw: [0; ITEM_SIZE],
        };
        state.items[0].set_name("gold besant");
        state.items[0].raw[Item::KIND] = 49;
        state.items[1] = Item {
            raw: [0; ITEM_SIZE],
        };
        state.items[1].set_name("healing balm");
        state.items[1].raw[Item::KIND] = 33;
        state.items[1].raw[Item::POWER] = 1;
        state.items[1].raw[Item::EFFECT] = 3;
        state.items[1].raw[Item::EFFECT_ID] = 44;
        state.items[2] = Item {
            raw: [0; ITEM_SIZE],
        };
        state.items[2].set_name("sword");
        state.items[2].raw[Item::KIND] = 56;
        state.items[2].raw[Item::POWER] = 5;
        state.items[2].raw[Item::EFFECT] = 5;
        state.items[2].raw[Item::EFFECT_ID] = 45;
        state.items[3] = Item {
            raw: [0; ITEM_SIZE],
        };
        state.items[3].set_name("helm");
        state.items[3].raw[Item::KIND] = 59;
        state.items[3].raw[Item::EFFECT] = 4;
        state.items[3].raw[Item::EFFECT_ID] = 46;
        state.items[4] = Item {
            raw: [0; ITEM_SIZE],
        };
        state.items[4].set_name("Heal");
        state.items[4].raw[Item::EFFECT] = 10;
        state.items[4].raw[Item::EFFECT_ID] = 44;
        state.items[5] = Item {
            raw: [0; ITEM_SIZE],
        };
        state.items[5].set_name("Shield");
        state.items[5].raw[Item::EFFECT] = 10;
        state.items[5].raw[Item::EFFECT_ID] = 46;
        runtime
    }

    fn select_def(runtime: &mut Runtime, def: usize) {
        runtime.item_index = runtime
            .item_entries()
            .iter()
            .position(|entry| entry.def == def)
            .expect("entry in active list");
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

    #[test]
    fn status_item_panel_uses_and_equips_objects() {
        let mut runtime = economy_runtime();
        runtime.session.state.add_item(0, 2, 1).unwrap();
        runtime.session.state.add_item(0, 3, 1).unwrap();

        runtime.handle_key(Key::Status);
        runtime.handle_key(Key::Confirm);
        assert_eq!(runtime.screen(), Screen::Item);
        assert_eq!(runtime.item_action(), 3);

        let before = runtime.session().state.to_bytes();
        select_def(&mut runtime, 2);
        runtime.handle_key(Key::Confirm);
        assert_eq!(runtime.session().state.characters[0].health(), 14);
        assert!(
            runtime
                .session()
                .state
                .inventory(0)
                .iter()
                .all(|entry| entry.def != 2),
            "balm was consumed"
        );
        assert_ne!(runtime.session().state.to_bytes(), before);

        let before = runtime.session().state.to_bytes();
        select_def(&mut runtime, 3);
        runtime.handle_key(Key::Confirm);
        assert!(
            runtime
                .session()
                .state
                .inventory(0)
                .iter()
                .any(|entry| entry.def == 3 && entry.equipped),
            "sword is equipped"
        );
        assert_ne!(runtime.session().state.to_bytes(), before);
    }

    #[test]
    fn item_panel_drops_and_picks_up_a_carried_object() {
        let mut runtime = economy_runtime();
        runtime.session.state.add_item(0, 2, 1).unwrap();

        runtime.open_item(1);
        let before = runtime.session().state.to_bytes();
        runtime.handle_key(Key::Confirm);
        assert!(runtime.session().state.inventory(0).is_empty());
        assert_eq!(runtime.session().state.ground_items(5).len(), 1);
        assert_ne!(runtime.session().state.to_bytes(), before);

        runtime.open_item(0);
        assert_eq!(runtime.item_entries().len(), 1);
        let before = runtime.session().state.to_bytes();
        runtime.handle_key(Key::Confirm);
        assert_eq!(runtime.session().state.inventory(0).len(), 1);
        assert!(runtime.session().state.ground_items(5).is_empty());
        assert_ne!(runtime.session().state.to_bytes(), before);
    }

    #[test]
    fn give_target_panel_moves_an_object_to_the_target() {
        let mut runtime = economy_runtime();
        runtime.session.state.add_item(0, 2, 1).unwrap();

        runtime.open_target(9);
        assert_eq!(runtime.target_character(), 1);
        runtime.open_item(9);
        let before = runtime.session().state.to_bytes();
        runtime.handle_key(Key::Confirm);

        assert!(runtime.session().state.inventory(0).is_empty());
        assert_eq!(runtime.session().state.inventory(1).len(), 1);
        assert_ne!(runtime.session().state.to_bytes(), before);
    }

    #[test]
    fn shop_panel_buys_and_sells_for_gold() {
        let mut runtime = economy_runtime();
        runtime.session.state.add_item(1, 3, 1).unwrap();

        runtime.open_shop(Some(1));
        assert_eq!(runtime.screen(), Screen::Shop);
        let before = runtime.session().state.to_bytes();
        runtime.handle_key(Key::Confirm);
        assert_eq!(runtime.session().state.gold(0), 95);
        assert!(
            runtime
                .session()
                .state
                .inventory(0)
                .iter()
                .any(|entry| entry.def == 3)
        );
        assert!(runtime.session().state.inventory(1).is_empty());
        assert_ne!(runtime.session().state.to_bytes(), before);

        runtime.handle_key(Key::Space);
        assert!(runtime.shop_sell());
        let before = runtime.session().state.to_bytes();
        runtime.handle_key(Key::Confirm);
        assert_eq!(runtime.session().state.gold(0), 97);
        assert!(
            runtime
                .session()
                .state
                .inventory(0)
                .iter()
                .all(|entry| entry.def != 3)
        );
        assert_eq!(runtime.session().state.inventory(1).len(), 1);
        assert_ne!(runtime.session().state.to_bytes(), before);
    }

    #[test]
    fn spell_panel_casts_and_consumes_the_selected_spell() {
        let mut runtime = economy_runtime();
        runtime.session.state.add_item(0, 5, 1).unwrap();
        runtime.session.state.add_item(0, 6, 1).unwrap();

        runtime.open_spell();
        assert_eq!(runtime.screen(), Screen::Spell);
        assert_eq!(runtime.spell_entries().len(), 2);

        let before = runtime.session().state.to_bytes();
        runtime.item_index = runtime
            .spell_entries()
            .iter()
            .position(|entry| entry.def == 5)
            .expect("Heal");
        runtime.handle_key(Key::Confirm);
        assert_eq!(runtime.session().state.characters[0].health(), 20);
        assert!(
            runtime
                .session()
                .state
                .spells(0)
                .iter()
                .all(|entry| entry.def != 5),
            "Heal was consumed"
        );
        assert_ne!(runtime.session().state.to_bytes(), before);

        let before = runtime.session().state.to_bytes();
        runtime.item_index = runtime
            .spell_entries()
            .iter()
            .position(|entry| entry.def == 6)
            .expect("Shield");
        runtime.handle_key(Key::Confirm);
        assert_eq!(runtime.session().state.characters[0].armor(), 15);
        assert!(runtime.session().state.spells(0).is_empty());
        assert_ne!(runtime.session().state.to_bytes(), before);
    }
}
