//! Army battle model.
//!
//! Live army records are 0x38 bytes and are referenced by the battle lists at
//! `DS:9DC2` / `DS:9DD6`. The list stores 1-based character record indices, so
//! battle record `n` is the name field of character record `n - 1`:
//!
//! - `0x0A..0x15`: NUL-padded name
//! - `0x00..0x02`: character combat/health fields (see [`crate::save`])
//! - `0x26`: maximum/starting strength (word)
//! - `0x28`: current strength (word)
//! - `0x2A`: primary battle sprite image id
//! - `0x2C`: secondary image id
//! - `0x35`: defence field used by `FUN_20b5_1dff`
//! - `0x36`: current battle hit points (name-relative `+0x2C`)
//! - `0x37`: attack skill (name-relative `+0x2D`)
//!
//! `FUN_20b5_085b` draws the Round Table/Foes (or Defender/Challenger) screen.
//! `FUN_20b5_1dff` is the per-unit round reducer: it rolls against
//! `attack = max((unit[+0x2A] - 10) / 2, unit[+0x2D])`, subtracts the target's
//! `+0x2C`/`+0x2B` defence fields, and applies 0, 1 or 2 hit points. Casualties
//! in the save files are the word at `+0x28`; the 2449 helpers (notably
//! `FUN_2449_0faa`) sum and compare those strengths to decide the battle.
//!
//! This module keeps the same record interpretation and the verified hit roll,
//! then folds the presentation-only pathing/formation code into a headless
//! round engine. Damage is deterministic per seed so round and outcome
//! transitions can be tested.

#![forbid(unsafe_code)]

use crate::save::{Character, GameState};

/// Size of one live character/army record.
pub const RECORD_SIZE: usize = 0x38;
/// Maximum/starting strength word, relative to the record base.
pub const MAX_STRENGTH: usize = 0x26;
/// Current strength word, relative to the record base.
pub const CURRENT_STRENGTH: usize = 0x28;
/// Battle hit points used by `FUN_20b5_1dff` (`name + 0x2C`).
pub const BATTLE_HP: usize = 0x36;
/// Battle defence field used by `FUN_20b5_1dff` (`name + 0x2B`).
pub const BATTLE_DEFENCE: usize = 0x35;
/// Battle attack skill used by `FUN_20b5_1dff` (`name + 0x2D`).
pub const BATTLE_SKILL: usize = 0x37;

/// The original stops automatically derived rounds after this many passes.
pub const MAX_ROUNDS: u32 = 30;

/// Which army a unit fights for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Player,
    Foe,
}

impl Side {
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Player => Self::Foe,
            Self::Foe => Self::Player,
        }
    }
}

/// Display category derived from the record name and strength.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnitKind {
    Leader,
    Knight,
    Soldier,
    Archer,
    Other,
}

impl UnitKind {
    fn from_name(name: &str, max_strength: u16) -> Self {
        let lower = name.to_ascii_lowercase();
        if lower.contains("knight") {
            Self::Knight
        } else if lower.contains("soldier") {
            Self::Soldier
        } else if lower.contains("archer") {
            Self::Archer
        } else if max_strength <= 1 {
            Self::Leader
        } else {
            Self::Other
        }
    }

    #[must_use]
    pub const fn plural(self) -> &'static str {
        match self {
            Self::Leader => "LEADER",
            Self::Knight => "KNIGHTS",
            Self::Soldier => "SOLDIERS",
            Self::Archer => "ARCHERS",
            Self::Other => "TROOPS",
        }
    }
}

/// What a side does with its next pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Order {
    Charge,
    Engage,
    Flank,
    Retreat,
    DoRound,
    Ignore,
    /// Alternate (non-defender) menu: normal attack.
    Attack,
    /// Alternate menu: hold and take half casualties this pass.
    Defend,
    /// Alternate menu label for [`Order::Retreat`].
    Withdraw,
    /// Alternate menu: resolve passes until the battle ends.
    Automatic,
}

impl Order {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Charge => "CHARGE",
            Self::Engage => "ENGAGE",
            Self::Flank => "FLANK",
            Self::Retreat => "RETREAT",
            Self::DoRound => "DO ROUND",
            Self::Ignore => "IGNORE",
            Self::Attack => "ATTACK",
            Self::Defend => "DEFEND",
            Self::Withdraw => "WITHDRAW",
            Self::Automatic => "AUTOMATIC",
        }
    }

    #[must_use]
    pub const fn is_retreat(self) -> bool {
        matches!(self, Self::Retreat | Self::Withdraw)
    }

    #[must_use]
    pub const fn is_round(self) -> bool {
        matches!(
            self,
            Self::Charge | Self::Engage | Self::Flank | Self::DoRound | Self::Attack | Self::Defend
        )
    }

    #[must_use]
    pub const fn is_automatic(self) -> bool {
        matches!(self, Self::Automatic)
    }

    const fn attack_bonus(self) -> i32 {
        match self {
            Self::Charge => 20,
            Self::Flank => 10,
            Self::Defend => -10,
            _ => 0,
        }
    }

    const fn incoming_percent(self) -> u16 {
        match self {
            Self::Defend => 50,
            Self::Retreat | Self::Withdraw => 75,
            Self::Charge => 125,
            _ => 100,
        }
    }
}

/// Original battle menus. The six-entry menu matches the supplied captures.
pub const BATTLE_ORDERS: [Order; 6] = [
    Order::Charge,
    Order::Engage,
    Order::Flank,
    Order::Retreat,
    Order::DoRound,
    Order::Ignore,
];

/// Alternate menu used when the screen is opened in defender/challenger mode.
pub const ALTERNATE_ORDERS: [Order; 4] = [
    Order::Attack,
    Order::Defend,
    Order::Withdraw,
    Order::Automatic,
];

/// The three terminal results of a battle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BattleOutcome {
    Victory,
    Defeat,
    Withdrawn,
}

impl BattleOutcome {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Victory => "VICTORY",
            Self::Defeat => "DEFEAT",
            Self::Withdrawn => "WITHDRAWN",
        }
    }

    #[must_use]
    pub const fn won(self) -> bool {
        matches!(self, Self::Victory)
    }
}

/// One army/stack on the battlefield.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BattleUnit {
    /// 0-based character record index, when built from a [`GameState`].
    pub record_index: Option<usize>,
    pub side: Side,
    pub name: String,
    pub kind: UnitKind,
    pub stats: CombatFields,
    pub strength: u16,
    pub max_strength: u16,
    pub hp: u8,
    pub max_hp: u8,
    pub attack: u8,
    pub defence: u8,
    pub image: u16,
    pub icon: u16,
    pub routed: bool,
}

/// The first three record bytes, kept from the original unit model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CombatFields {
    pub armour: u8,
    pub attack: u8,
    pub defence: u8,
}

impl BattleUnit {
    /// Build a unit from a 0x38-byte live character/army record.
    #[must_use]
    pub fn from_record(record: &[u8]) -> Option<Self> {
        if record.len() < RECORD_SIZE {
            return None;
        }
        let name_bytes = &record[0x0a..0x15];
        let end = name_bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(name_bytes.len());
        if end == 0 {
            return None;
        }
        let name = String::from_utf8_lossy(&name_bytes[..end]).into_owned();
        let max_strength = u16::from_le_bytes([record[MAX_STRENGTH], record[MAX_STRENGTH + 1]]);
        let strength = u16::from_le_bytes([record[CURRENT_STRENGTH], record[CURRENT_STRENGTH + 1]]);
        let hp = record[BATTLE_HP];
        Some(Self {
            record_index: None,
            side: Side::Player,
            kind: UnitKind::from_name(&name, max_strength),
            name,
            stats: CombatFields {
                armour: record[0],
                attack: record[1],
                defence: record[2],
            },
            max_strength,
            strength,
            hp,
            max_hp: hp.max(1),
            attack: record[BATTLE_SKILL].max(1),
            defence: record[BATTLE_DEFENCE],
            image: u16::from_le_bytes([record[0x2a], record[0x2b]]),
            icon: u16::from_le_bytes([record[0x2c], record[0x2d]]),
            routed: strength == 0 || hp == 0,
        })
    }

    /// Build a battle unit from a save-state character record.
    #[must_use]
    pub fn from_character(index: usize, side: Side, character: &Character) -> Option<Self> {
        let name = character.name();
        if name.is_empty() {
            return None;
        }
        let max_strength = character.max_strength();
        let strength = character.current_strength();
        let hp = character.battle_hp();
        let mut unit = Self::from_record(&character.raw)?;
        unit.record_index = Some(index);
        unit.side = side;
        unit.name = name;
        unit.kind = UnitKind::from_name(&unit.name, max_strength);
        unit.max_strength = max_strength;
        unit.strength = strength;
        unit.hp = hp;
        unit.max_hp = hp.max(1);
        unit.attack = character.battle_skill().max(1);
        unit.defence = character.battle_defence();
        unit.routed = strength == 0 || hp == 0;
        Some(unit)
    }

    /// Construct a synthetic unit (tests and scripted encounters).
    #[must_use]
    pub fn new(
        side: Side,
        name: impl Into<String>,
        strength: u16,
        hp: u8,
        attack: u8,
        defence: u8,
    ) -> Self {
        let name = name.into();
        let hp = hp.max(1);
        Self {
            record_index: None,
            side,
            kind: UnitKind::from_name(&name, strength),
            name,
            stats: CombatFields {
                armour: defence,
                attack,
                defence,
            },
            strength,
            max_strength: strength,
            hp,
            max_hp: hp,
            attack: attack.max(1),
            defence,
            image: 0,
            icon: 0,
            routed: strength == 0,
        }
    }

    #[must_use]
    pub fn is_leader(&self) -> bool {
        self.kind == UnitKind::Leader || self.max_strength <= 1
    }

    #[must_use]
    pub fn is_routed(&self) -> bool {
        self.routed || self.strength == 0 || self.hp == 0
    }

    /// One display row, e.g. `CONSTANTINE` or `38 KNIGHTS`.
    #[must_use]
    pub fn display(&self) -> String {
        if self.max_strength > 1 {
            let label = if self.kind == UnitKind::Other {
                self.name.to_ascii_uppercase()
            } else {
                self.kind.plural().to_string()
            };
            format!("{} {label}", self.strength)
        } else {
            self.name.to_ascii_uppercase()
        }
    }
}

/// Result of one resolved pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoundReport {
    pub round: u32,
    pub messages: Vec<String>,
    pub player_losses: u16,
    pub foe_losses: u16,
    pub outcome: Option<BattleOutcome>,
}

/// Small deterministic generator used by round resolution.
#[derive(Debug, Clone)]
struct BattleRng {
    state: u32,
}

impl BattleRng {
    fn new(seed: u32) -> Self {
        Self {
            state: if seed == 0 { 0x9e37_79b9 } else { seed },
        }
    }

    fn next(&mut self) -> u32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        x
    }

    fn roll(&mut self, limit: u32) -> u32 {
        self.next() % limit.max(1)
    }
}

/// One complete battle.
#[derive(Debug, Clone)]
pub struct Battle {
    pub round: u32,
    pub player: Vec<BattleUnit>,
    pub foe: Vec<BattleUnit>,
    pub outcome: Option<BattleOutcome>,
    pub messages: Vec<String>,
    pub player_order: Order,
    pub foe_order: Order,
    pub alternate_orders: bool,
    rng: BattleRng,
}

impl Battle {
    /// Start a battle from two prepared armies. `seed` makes resolution
    /// reproducible; normal play passes a time-derived seed.
    #[must_use]
    pub fn new(player: Vec<BattleUnit>, foe: Vec<BattleUnit>, seed: u32) -> Self {
        let mut battle = Self {
            round: 0,
            player,
            foe,
            outcome: None,
            messages: Vec::new(),
            player_order: Order::Engage,
            foe_order: Order::Engage,
            alternate_orders: false,
            rng: BattleRng::new(seed),
        };
        battle.check_outcome();
        battle
    }

    /// Build a battle from save-state character indices.
    #[must_use]
    pub fn from_state(
        state: &GameState,
        player_indices: &[usize],
        foe_indices: &[usize],
        seed: u32,
    ) -> Option<Self> {
        let player = player_indices
            .iter()
            .filter_map(|index| {
                state.characters.get(*index).and_then(|character| {
                    BattleUnit::from_character(*index, Side::Player, character)
                })
            })
            .collect::<Vec<_>>();
        let foe = foe_indices
            .iter()
            .filter_map(|index| {
                state
                    .characters
                    .get(*index)
                    .and_then(|character| BattleUnit::from_character(*index, Side::Foe, character))
            })
            .collect::<Vec<_>>();
        if player.is_empty() || foe.is_empty() {
            return None;
        }
        Some(Self::new(player, foe, seed))
    }

    /// Use the defender/challenger order menu instead of Round Table/Foes.
    #[must_use]
    pub fn with_alternate_orders(mut self) -> Self {
        self.alternate_orders = true;
        self
    }

    #[must_use]
    pub fn orders(&self) -> &'static [Order] {
        if self.alternate_orders {
            &ALTERNATE_ORDERS
        } else {
            &BATTLE_ORDERS
        }
    }

    #[must_use]
    pub fn units(&self, side: Side) -> &[BattleUnit] {
        match side {
            Side::Player => &self.player,
            Side::Foe => &self.foe,
        }
    }

    fn units_mut(&mut self, side: Side) -> &mut [BattleUnit] {
        match side {
            Side::Player => &mut self.player,
            Side::Foe => &mut self.foe,
        }
    }

    /// Sum of current strengths, mirroring `FUN_2449_0faa`.
    #[must_use]
    pub fn total_strength(&self, side: Side) -> u32 {
        self.units(side)
            .iter()
            .map(|unit| u32::from(unit.strength))
            .sum()
    }

    #[must_use]
    pub fn living_count(&self, side: Side) -> usize {
        self.units(side)
            .iter()
            .filter(|unit| !unit.is_routed())
            .count()
    }

    #[must_use]
    pub fn is_over(&self) -> bool {
        self.outcome.is_some()
    }

    pub fn set_player_order(&mut self, order: Order) {
        self.player_order = order;
    }

    pub fn set_foe_order(&mut self, order: Order) {
        self.foe_order = order;
    }

    /// Leave the field. The caller decides whether the battle returns to the
    /// map or to a game-over path.
    pub fn withdraw(&mut self) {
        if self.outcome.is_none() {
            self.outcome = Some(BattleOutcome::Withdrawn);
            self.push_message("THE ARMY WITHDRAWS FROM THE FIELD.");
        }
    }

    /// Resolve one full pass: player units attack, then surviving foes attack.
    pub fn resolve_round(&mut self) -> RoundReport {
        if self.outcome.is_some() {
            return RoundReport {
                round: self.round,
                messages: Vec::new(),
                player_losses: 0,
                foe_losses: 0,
                outcome: self.outcome,
            };
        }
        if self.player_order.is_retreat() {
            self.withdraw();
            return RoundReport {
                round: self.round,
                messages: vec!["THE ARMY WITHDRAWS FROM THE FIELD.".to_string()],
                player_losses: 0,
                foe_losses: 0,
                outcome: self.outcome,
            };
        }

        self.round += 1;
        let player_before = self.total_strength(Side::Player);
        let foe_before = self.total_strength(Side::Foe);
        let player_order = self.player_order;
        let foe_order = self.foe_order;
        let mut messages = Vec::new();

        let player_count = self.player.len();
        for index in 0..player_count {
            if self.player[index].is_routed() {
                continue;
            }
            let Some(target) = self.first_living(Side::Foe) else {
                break;
            };
            if let Some(message) = self.attack(
                Side::Player,
                index,
                Side::Foe,
                target,
                player_order,
                foe_order,
            ) {
                messages.push(message);
            }
        }
        self.check_outcome();

        if self.outcome.is_none() {
            let foe_count = self.foe.len();
            for index in 0..foe_count {
                if self.foe[index].is_routed() {
                    continue;
                }
                let Some(target) = self.first_living(Side::Player) else {
                    break;
                };
                if let Some(message) = self.attack(
                    Side::Foe,
                    index,
                    Side::Player,
                    target,
                    foe_order,
                    player_order,
                ) {
                    messages.push(message);
                }
            }
            self.check_outcome();
        }

        let player_losses =
            u16::try_from(player_before - self.total_strength(Side::Player)).unwrap_or(u16::MAX);
        let foe_losses =
            u16::try_from(foe_before - self.total_strength(Side::Foe)).unwrap_or(u16::MAX);
        for message in &messages {
            self.push_message(message);
        }
        RoundReport {
            round: self.round,
            messages,
            player_losses,
            foe_losses,
            outcome: self.outcome,
        }
    }

    /// Resolve passes until an outcome or the safety limit.
    pub fn auto_resolve(&mut self) -> RoundReport {
        let mut messages = Vec::new();
        let round = self.round;
        let player_before = self.total_strength(Side::Player);
        let foe_before = self.total_strength(Side::Foe);
        while self.outcome.is_none() && self.round < MAX_ROUNDS {
            let report = self.resolve_round();
            messages.extend(report.messages);
            if self.outcome.is_some() {
                break;
            }
        }
        RoundReport {
            round: self.round.max(round),
            messages,
            player_losses: u16::try_from(player_before - self.total_strength(Side::Player))
                .unwrap_or(u16::MAX),
            foe_losses: u16::try_from(foe_before - self.total_strength(Side::Foe))
                .unwrap_or(u16::MAX),
            outcome: self.outcome,
        }
    }

    fn first_living(&self, side: Side) -> Option<usize> {
        self.units(side).iter().position(|unit| !unit.is_routed())
    }

    fn attack(
        &mut self,
        attacker_side: Side,
        attacker_index: usize,
        target_side: Side,
        target_index: usize,
        order: Order,
        target_order: Order,
    ) -> Option<String> {
        let (attacker_name, attacker_strength, attacker_attack) = {
            let attacker = &self.units(attacker_side)[attacker_index];
            (attacker.name.clone(), attacker.strength, attacker.attack)
        };
        let (target_name, target_defence, target_max_hp, target_strength) = {
            let target = &self.units(target_side)[target_index];
            (
                target.display(),
                target.defence,
                target.max_hp,
                target.strength,
            )
        };

        // `FUN_20b5_1dff` roll: attacker skill/mass versus target defence.
        let attack_power =
            i32::from(attacker_attack) + i32::from(attacker_strength / 100) + order.attack_bonus();
        let target_defence =
            i32::from(target_defence) + i32::from(target_max_hp) + i32::from(target_strength / 200);
        let score = (attack_power - target_defence + 45).clamp(5, 95);
        let roll = i32::try_from(self.rng.roll(100)).unwrap_or(0);
        if roll >= score {
            return None;
        }

        // Original: 2 hit points on a strong roll, otherwise 1.
        let mut hp_damage = if roll < score / 4 { 2 } else { 1 };
        if target_order == Order::Defend {
            hp_damage = 1;
        }
        let mut casualties = (attacker_strength / 20 + 1).saturating_mul(u16::from(hp_damage));
        if target_order.incoming_percent() != 100 {
            casualties = (casualties * target_order.incoming_percent() / 100).max(1);
        }
        if order == Order::Flank {
            casualties = casualties.saturating_add(casualties / 4);
        }

        let target = &mut self.units_mut(target_side)[target_index];
        target.hp = target.hp.saturating_sub(hp_damage);
        target.strength = target.strength.saturating_sub(casualties);
        if target.hp == 0 || target.strength == 0 {
            target.hp = 0;
            target.strength = 0;
            target.routed = true;
            Some(format!("{target_name} IS ROUTED."))
        } else {
            Some(format!(
                "{} HITS {} FOR {casualties} MEN.",
                attacker_name.to_ascii_uppercase(),
                target_name
            ))
        }
    }

    fn check_outcome(&mut self) -> Option<BattleOutcome> {
        if self.outcome.is_some() {
            return self.outcome;
        }
        let player_alive = self.living_count(Side::Player) > 0;
        let foe_alive = self.living_count(Side::Foe) > 0;
        if !foe_alive {
            self.outcome = Some(BattleOutcome::Victory);
            self.push_message("THE FOE IS ROUTED. THE FIELD IS WON.");
        } else if !player_alive {
            self.outcome = Some(BattleOutcome::Defeat);
            self.push_message("THE ARMY IS ROUTED. THE FIELD IS LOST.");
        } else if self.round >= MAX_ROUNDS {
            let player = self.total_strength(Side::Player);
            let foe = self.total_strength(Side::Foe);
            self.outcome = Some(if player >= foe {
                BattleOutcome::Victory
            } else {
                BattleOutcome::Defeat
            });
            self.push_message("THE BATTLE ENDS AT NIGHTFALL.");
        }
        self.outcome
    }

    fn push_message(&mut self, message: impl Into<String>) {
        self.messages.push(message.into());
        if self.messages.len() > 8 {
            self.messages.remove(0);
        }
    }

    /// Copy casualties and battle hit points back into the save-state records.
    pub fn apply_to_state(&self, state: &mut GameState) {
        for unit in self.player.iter().chain(self.foe.iter()) {
            let Some(index) = unit.record_index else {
                continue;
            };
            let Some(character) = state.characters.get_mut(index) else {
                continue;
            };
            character.set_current_strength(unit.strength);
            character.set_battle_hp(unit.hp);
            if unit.is_routed() {
                // `FUN_20b5_1dff` clears flag 0x0008 (the "in scene" bit) when
                // the unit is removed from the field.
                let flags = character.presence_flags() & !0x0008;
                character.set_presence_flags(flags);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with(units: &[(usize, &str, u16, u16, u8, u8, u8)]) -> GameState {
        let mut state = GameState::parse(&[0u8; crate::save::FILE_SIZE]).unwrap();
        for (index, name, max, current, hp, attack, defence) in units {
            let character = &mut state.characters[*index];
            character.set_name(name);
            character.raw[MAX_STRENGTH..MAX_STRENGTH + 2].copy_from_slice(&max.to_le_bytes());
            character.raw[CURRENT_STRENGTH..CURRENT_STRENGTH + 2]
                .copy_from_slice(&current.to_le_bytes());
            character.raw[BATTLE_HP] = *hp;
            character.raw[BATTLE_SKILL] = *attack;
            character.raw[BATTLE_DEFENCE] = *defence;
        }
        state
    }

    #[test]
    fn parses_live_record_shape() {
        let mut record = [0u8; 0x38];
        record[0x00] = 40;
        record[0x01] = 5;
        record[0x02] = 0;
        record[0x0a..0x11].copy_from_slice(b"knight\0");
        record[0x26..0x28].copy_from_slice(&100u16.to_le_bytes());
        record[0x28..0x2a].copy_from_slice(&100u16.to_le_bytes());
        record[0x2a..0x2c].copy_from_slice(&677u16.to_le_bytes());
        record[0x2c..0x2e].copy_from_slice(&700u16.to_le_bytes());
        record[BATTLE_HP] = 2;
        record[BATTLE_SKILL] = 5;
        record[BATTLE_DEFENCE] = 2;
        let unit = BattleUnit::from_record(&record).unwrap();
        assert_eq!(unit.name, "knight");
        assert_eq!(unit.stats.armour, 40);
        assert_eq!((unit.strength, unit.max_strength), (100, 100));
        assert_eq!((unit.image, unit.icon), (677, 700));
        assert_eq!(
            (unit.hp, unit.max_hp, unit.attack, unit.defence),
            (2, 2, 5, 2)
        );
        assert!(!unit.is_routed());
    }

    #[test]
    fn round_damage_reduces_enemy_strength_and_hp() {
        let player = BattleUnit::new(Side::Player, "Constantine", 100, 5, 100, 5);
        let foe = BattleUnit::new(Side::Foe, "soldier", 825, 2, 5, 1);
        let mut battle = Battle::new(vec![player], vec![foe], 7);
        let before = battle.total_strength(Side::Foe);
        let report = battle.resolve_round();
        assert_eq!(report.round, 1);
        assert!(!report.messages.is_empty(), "a hit should be logged");
        assert!(
            battle.total_strength(Side::Foe) < before,
            "foe strength must fall"
        );
        assert!(
            battle.foe[0].hp < 2 || battle.foe[0].is_routed(),
            "foe hit points must fall"
        );
        assert!(report.foe_losses > 0);
    }

    #[test]
    fn battle_transitions_to_victory() {
        let player = BattleUnit::new(Side::Player, "Constantine", 100, 5, 100, 5);
        let foe = BattleUnit::new(Side::Foe, "soldier", 5, 1, 1, 0);
        let mut battle = Battle::new(vec![player], vec![foe], 11);
        for _ in 0..10 {
            if battle.is_over() {
                break;
            }
            battle.resolve_round();
        }
        assert_eq!(battle.outcome, Some(BattleOutcome::Victory));
        assert!(battle.foe.iter().all(BattleUnit::is_routed));
        assert_eq!(battle.total_strength(Side::Foe), 0);
    }

    #[test]
    fn battle_transitions_to_defeat() {
        let player = BattleUnit::new(Side::Player, "Constantine", 5, 1, 1, 0);
        let foe = BattleUnit::new(Side::Foe, "soldier", 100, 5, 100, 5);
        let mut battle = Battle::new(vec![player], vec![foe], 13);
        for _ in 0..10 {
            if battle.is_over() {
                break;
            }
            battle.resolve_round();
        }
        assert_eq!(battle.outcome, Some(BattleOutcome::Defeat));
        assert!(battle.player.iter().all(BattleUnit::is_routed));
    }

    #[test]
    fn withdraw_ends_the_battle_without_round_damage() {
        let player = BattleUnit::new(Side::Player, "Constantine", 100, 5, 10, 5);
        let foe = BattleUnit::new(Side::Foe, "soldier", 100, 2, 10, 1);
        let mut battle = Battle::new(vec![player], vec![foe], 3);
        battle.set_player_order(Order::Retreat);
        let report = battle.resolve_round();
        assert_eq!(battle.outcome, Some(BattleOutcome::Withdrawn));
        assert_eq!(report.outcome, Some(BattleOutcome::Withdrawn));
        assert_eq!(battle.total_strength(Side::Player), 100);
        assert_eq!(battle.total_strength(Side::Foe), 100);
    }

    #[test]
    fn casualties_are_written_back_to_character_records() {
        let mut state = state_with(&[
            (0, "Constantine", 100, 100, 5, 100, 5),
            (70, "soldier", 825, 825, 2, 5, 1),
        ]);
        let mut battle = Battle::from_state(&state, &[0], &[70], 17).expect("battle");
        let before = state.characters[70].current_strength();
        let before_hp = state.characters[70].battle_hp();
        for _ in 0..10 {
            if battle.is_over() {
                break;
            }
            battle.resolve_round();
        }
        battle.apply_to_state(&mut state);
        assert!(state.characters[70].current_strength() < before);
        assert!(state.characters[70].battle_hp() < before_hp || before_hp == 0);
        assert_eq!(
            state.characters[70].current_strength(),
            battle.foe[0].strength
        );
    }

    #[test]
    fn alternate_orders_match_defender_capture() {
        let battle = Battle::new(
            vec![BattleUnit::new(Side::Player, "Constantine", 1, 1, 1, 1)],
            vec![BattleUnit::new(Side::Foe, "soldier", 1, 1, 1, 1)],
            1,
        )
        .with_alternate_orders();
        assert_eq!(
            battle.orders(),
            &[
                Order::Attack,
                Order::Defend,
                Order::Withdraw,
                Order::Automatic
            ]
        );
    }
}
