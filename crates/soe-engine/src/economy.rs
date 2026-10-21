//! Inventory, equipment, money, trade and spell actions.
//!
//! Layout summary (see `notes/SAVESTATE.md`):
//!
//! * 49 object definitions at file `0x1C4A`, 27 bytes each. Definition indices
//!   are 1-based in the original (`DS:488B + index * 0x1B`).
//! * 85 live object instances at file `0x2175`, 4 bytes each:
//!   `{definition, owner, scene_or_amount, flags}`.
//! * Character record `+0x28` is the party purse. Definition 1 is money.
//!
//! The `1ba8` helpers confirm the important semantics: `FUN_1ba8_0431`
//! transfers instances by rewriting the owner byte, `FUN_1ba8_04fd` counts
//! carried instances with flags `0x18`, and `FUN_1ba8_078f` lists them.

#![forbid(unsafe_code)]

use crate::save::{GameState, Item, ObjectInstance};

pub const GOLD_DEF: usize = 1;
/// Carried in a character's inventory.
pub const INVENTORY_FLAG: u8 = 0x18;
/// Carried and equipped (`INVENTORY_FLAG | 1`).
pub const EQUIPPED_FLAG: u8 = 0x19;
/// Lying in the current scene.
pub const GROUND_FLAG: u8 = 0x04;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EconomyError {
    NoSuchCharacter(usize),
    NoSuchItem(usize),
    NotEnoughGold { need: u16, have: u16 },
    InventoryFull,
    NotOwned,
    NoStock,
    NotEquipment,
    ItemEquipped,
    NothingHappens,
}

impl std::fmt::Display for EconomyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSuchCharacter(index) => write!(f, "no character {index}"),
            Self::NoSuchItem(index) => write!(f, "no item {index}"),
            Self::NotEnoughGold { need, have } => {
                write!(f, "needs {need} gold, has {have}")
            }
            Self::InventoryFull => write!(f, "inventory is full"),
            Self::NotOwned => write!(f, "item is not carried"),
            Self::NoStock => write!(f, "no stock"),
            Self::NotEquipment => write!(f, "item cannot be equipped"),
            Self::ItemEquipped => write!(f, "item is equipped"),
            Self::NothingHappens => write!(f, "nothing happens"),
        }
    }
}

impl std::error::Error for EconomyError {}

/// An inventory row. `slot` is the instance index (0 for catalogue entries).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryEntry {
    pub slot: usize,
    pub def: usize,
    pub name: String,
    pub quantity: u16,
    pub equipped: bool,
    pub kind: u8,
    pub price: u16,
    pub power: u8,
    pub effect: u8,
}

/// Decoded view of one character, used by the status screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterSheet {
    pub index: usize,
    pub name: String,
    pub health: u8,
    pub hits: u8,
    pub movement: u8,
    pub combat: u8,
    pub armor: u8,
    pub nobility: u8,
    pub faith: u8,
    pub magic: u8,
    pub level: u8,
    pub gold: u16,
    pub effective_combat: u8,
    pub effective_armor: u8,
    pub equipment: Vec<InventoryEntry>,
    pub inventory: Vec<InventoryEntry>,
    pub spells: Vec<InventoryEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionOutcome {
    pub message: String,
    pub consumed: bool,
    pub equipped: bool,
}

impl GameState {
    /// Object definition by 1-based index.
    #[must_use]
    pub fn item_def(&self, def: usize) -> Option<&Item> {
        if def == 0 {
            None
        } else {
            self.items.get(def - 1)
        }
    }

    fn item_def_cloned(&self, def: usize) -> Result<Item, EconomyError> {
        self.item_def(def)
            .cloned()
            .ok_or(EconomyError::NoSuchItem(def))
    }

    fn character_ref(&self, index: usize) -> Result<&crate::save::Character, EconomyError> {
        self.characters
            .get(index)
            .ok_or(EconomyError::NoSuchCharacter(index))
    }

    fn character_mut_ref(
        &mut self,
        index: usize,
    ) -> Result<&mut crate::save::Character, EconomyError> {
        self.characters
            .get_mut(index)
            .ok_or(EconomyError::NoSuchCharacter(index))
    }

    /// First non-empty character, preferring one with health. This keeps the
    /// playable UI on a knight instead of the unused Constantine record.
    #[must_use]
    pub fn default_character(&self) -> usize {
        self.characters
            .iter()
            .position(|character| !character.name().is_empty() && character.health() > 0)
            .or_else(|| {
                self.characters
                    .iter()
                    .position(|character| !character.name().is_empty())
            })
            .unwrap_or(0)
    }

    #[must_use]
    pub fn gold(&self, character: usize) -> u16 {
        self.characters
            .get(character)
            .map_or(0, crate::save::Character::gold)
    }

    pub fn add_gold(&mut self, character: usize, amount: u16) -> Result<u16, EconomyError> {
        let record = self.character_mut_ref(character)?;
        let total = record.gold().saturating_add(amount);
        record.set_gold(total);
        Ok(total)
    }

    pub fn spend_gold(&mut self, character: usize, amount: u16) -> Result<u16, EconomyError> {
        let record = self.character_mut_ref(character)?;
        let have = record.gold();
        if have < amount {
            return Err(EconomyError::NotEnoughGold { need: amount, have });
        }
        let left = have - amount;
        record.set_gold(left);
        Ok(left)
    }

    fn entry(&self, slot: usize, instance: &ObjectInstance) -> Option<InventoryEntry> {
        let def = usize::from(instance.item());
        let item = self.item_def(def)?;
        Some(InventoryEntry {
            slot,
            def,
            name: item.name(),
            quantity: if def == GOLD_DEF {
                u16::from(instance.location())
            } else {
                1
            },
            equipped: instance.is_equipped(),
            kind: item.kind(),
            price: item.price().max(1),
            power: item.power(),
            effect: item.effect(),
        })
    }

    /// Carried objects for one 0-based character index. Money is exposed via
    /// [`GameState::gold`] instead of a row.
    ///
    /// This mirrors `FUN_1ba8_078f`: the original counts carried instances by
    /// definition and prints one row (`"3 healing balms"`), so duplicate
    /// instances are combined into one [`InventoryEntry`] with `quantity`.
    /// Equipped and unequipped copies stay separate so the status screen can
    /// still distinguish them.
    #[must_use]
    pub fn inventory(&self, character: usize) -> Vec<InventoryEntry> {
        let owner = character.saturating_add(1);
        if owner > u8::MAX as usize {
            return Vec::new();
        }
        let owner = owner as u8;
        let mut rows: Vec<InventoryEntry> = Vec::new();
        for (slot, instance) in self.instances.iter().enumerate() {
            if instance.item() == 0
                || instance.item() == GOLD_DEF as u8
                || instance.owner() != owner
                || !instance.is_carried()
            {
                continue;
            }
            let Some(entry) = self.entry(slot, instance) else {
                continue;
            };
            if let Some(existing) = rows
                .iter_mut()
                .find(|row| row.def == entry.def && row.equipped == entry.equipped)
            {
                existing.quantity = existing.quantity.saturating_add(entry.quantity);
            } else {
                rows.push(entry);
            }
        }
        rows
    }

    /// Prepared spells carried by a character.
    #[must_use]
    pub fn spells(&self, character: usize) -> Vec<InventoryEntry> {
        self.inventory(character)
            .into_iter()
            .filter(|entry| self.item_def(entry.def).is_some_and(Item::is_spell))
            .collect()
    }

    /// Objects lying in a scene (not carried by anyone).
    #[must_use]
    pub fn ground_items(&self, scene: u16) -> Vec<InventoryEntry> {
        let scene = scene as u8;
        self.instances
            .iter()
            .enumerate()
            .filter(|(_, instance)| {
                instance.item() != 0
                    && !instance.is_carried()
                    && instance.location() == scene
                    && instance.flags() & 0xe0 == 0
            })
            .filter_map(|(slot, instance)| self.entry(slot, instance))
            .collect()
    }

    /// Items sold by one character (spells and money excluded).
    #[must_use]
    pub fn shop_stock(&self, seller: usize) -> Vec<InventoryEntry> {
        self.inventory(seller)
            .into_iter()
            .filter(|entry| !self.item_def(entry.def).is_some_and(Item::is_spell))
            .collect()
    }

    /// Fallback catalogue used when no merchant stock is present. It keeps the
    /// economy loop usable on early episodes.
    #[must_use]
    pub fn catalogue(&self) -> Vec<InventoryEntry> {
        self.items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                if item.name().is_empty()
                    || item.is_spell()
                    || item.is_gold()
                    || index + 1 == GOLD_DEF
                {
                    return None;
                }
                Some(InventoryEntry {
                    slot: 0,
                    def: index + 1,
                    name: item.name(),
                    quantity: 1,
                    equipped: false,
                    kind: item.kind(),
                    price: item.price().max(1),
                    power: item.power(),
                    effect: item.effect(),
                })
            })
            .collect()
    }

    /// Find the first carried instance of `def`.
    fn find_carried(&self, character: usize, def: usize, equipped: Option<bool>) -> Option<usize> {
        let owner = character.saturating_add(1) as u8;
        self.instances
            .iter()
            .enumerate()
            .find_map(|(slot, instance)| {
                (instance.item() == def as u8
                    && instance.owner() == owner
                    && instance.is_carried()
                    && equipped.is_none_or(|wanted| instance.is_equipped() == wanted))
                .then_some(slot)
            })
    }

    fn empty_slot(&self) -> Option<usize> {
        self.instances.iter().position(ObjectInstance::is_empty)
    }

    /// Add one or more objects to a character, creating a new live instance for
    /// each. Money is routed to the purses word instead.
    pub fn add_item(
        &mut self,
        character: usize,
        def: usize,
        quantity: u16,
    ) -> Result<usize, EconomyError> {
        self.character_ref(character)?;
        self.item_def_cloned(def)?;
        if def == GOLD_DEF {
            self.add_gold(character, quantity)?;
            return Ok(0);
        }
        if quantity == 0 {
            return Err(EconomyError::NothingHappens);
        }
        let free = self.instances.iter().filter(|slot| slot.is_empty()).count();
        if free < usize::from(quantity) {
            return Err(EconomyError::InventoryFull);
        }
        let owner = (character + 1) as u8;
        let mut first = 0;
        for _ in 0..quantity {
            let slot = self.empty_slot().ok_or(EconomyError::InventoryFull)?;
            self.instances[slot] = ObjectInstance {
                raw: [def as u8, owner, 0, INVENTORY_FLAG],
            };
            if first == 0 {
                first = slot;
            }
        }
        Ok(first)
    }

    /// Remove carried instances. Equipped copies are used only after ordinary
    /// inventory copies, so selling/using prefers spare items.
    pub fn remove_item(
        &mut self,
        character: usize,
        def: usize,
        quantity: u16,
    ) -> Result<(), EconomyError> {
        if def == GOLD_DEF {
            self.spend_gold(character, quantity)?;
            return Ok(());
        }
        let mut left = quantity;
        while left > 0 {
            let slot = self
                .find_carried(character, def, Some(false))
                .or_else(|| self.find_carried(character, def, Some(true)))
                .ok_or(EconomyError::NotOwned)?;
            self.instances[slot] = ObjectInstance { raw: [0; 4] };
            left -= 1;
        }
        Ok(())
    }

    pub fn drop_item(
        &mut self,
        character: usize,
        def: usize,
        scene: u16,
    ) -> Result<usize, EconomyError> {
        if def == GOLD_DEF {
            return Err(EconomyError::NotEquipment);
        }
        let slot = self
            .find_carried(character, def, Some(false))
            .or_else(|| self.find_carried(character, def, Some(true)))
            .ok_or(EconomyError::NotOwned)?;
        let instance = &mut self.instances[slot];
        instance.set_owner(0);
        instance.set_location(scene as u8);
        instance.set_flags(GROUND_FLAG);
        Ok(slot)
    }

    pub fn pickup_item(
        &mut self,
        character: usize,
        slot: usize,
        scene: u16,
    ) -> Result<(), EconomyError> {
        self.character_ref(character)?;
        let instance = self.instances.get_mut(slot).ok_or(EconomyError::NoStock)?;
        if instance.item() == 0 || instance.is_carried() || instance.location() != scene as u8 {
            return Err(EconomyError::NoStock);
        }
        instance.set_owner((character + 1) as u8);
        instance.set_flags(INVENTORY_FLAG);
        Ok(())
    }

    pub fn give_item(
        &mut self,
        from: usize,
        to: usize,
        def: usize,
        quantity: u16,
    ) -> Result<(), EconomyError> {
        self.character_ref(to)?;
        if def == GOLD_DEF {
            self.spend_gold(from, quantity)?;
            self.add_gold(to, quantity)?;
            return Ok(());
        }
        let mut left = quantity;
        while left > 0 {
            let slot = self
                .find_carried(from, def, Some(false))
                .or_else(|| self.find_carried(from, def, Some(true)))
                .ok_or(EconomyError::NotOwned)?;
            let instance = &mut self.instances[slot];
            instance.set_owner((to + 1) as u8);
            instance.set_location(0);
            instance.set_flags(INVENTORY_FLAG);
            left -= 1;
        }
        Ok(())
    }

    pub fn equip_item(&mut self, character: usize, def: usize) -> Result<(), EconomyError> {
        let item = self.item_def_cloned(def)?;
        let slot = item.equipment_slot().ok_or(EconomyError::NotEquipment)?;
        let target = self
            .find_carried(character, def, None)
            .ok_or(EconomyError::NotOwned)?;

        // Unequip any other item in the same slot. Collect first so the
        // instance slice and the definition table are borrowed separately.
        let owner = (character + 1) as u8;
        let mut replace = Vec::new();
        for (index, instance) in self.instances.iter().enumerate() {
            if instance.owner() == owner && instance.is_equipped() {
                if let Some(other) = self.item_def(usize::from(instance.item())) {
                    if other.equipment_slot() == Some(slot) {
                        replace.push(index);
                    }
                }
            }
        }
        for index in replace {
            self.instances[index].set_flags(INVENTORY_FLAG);
        }
        self.instances[target].set_flags(EQUIPPED_FLAG);
        Ok(())
    }

    pub fn unequip_item(&mut self, character: usize, def: usize) -> Result<(), EconomyError> {
        let slot = self
            .find_carried(character, def, Some(true))
            .ok_or(EconomyError::NotOwned)?;
        self.instances[slot].set_flags(INVENTORY_FLAG);
        Ok(())
    }

    fn apply_effect(
        &mut self,
        target: usize,
        effect_id: u8,
        magnitude: u8,
        power: u8,
        name: &str,
    ) -> Result<String, EconomyError> {
        let record = self.character_mut_ref(target)?;
        let (stat, old, new) = match effect_id {
            4 => (
                "health",
                record.health(),
                record.health().saturating_add(magnitude.max(1)),
            ),
            43 | 44 => (
                "health",
                record.health(),
                record
                    .health()
                    .saturating_add(magnitude.max(1).saturating_add(power)),
            ),
            45 => (
                "combat",
                record.combat(),
                record.combat().saturating_add(power.max(1)),
            ),
            36 | 46 => (
                "armor",
                record.armor(),
                record.armor().saturating_add(magnitude.max(1)),
            ),
            64 => (
                "health",
                record.health(),
                record.health().saturating_add(magnitude.max(1)),
            ),
            _ if name.eq_ignore_ascii_case("Heal") => (
                "health",
                record.health(),
                record.health().saturating_add(10),
            ),
            _ if name.eq_ignore_ascii_case("Repletion") => (
                "health",
                record.health(),
                record.health().saturating_add(magnitude.max(1)),
            ),
            _ if name.eq_ignore_ascii_case("Defend") || name.eq_ignore_ascii_case("Shield") => (
                "armor",
                record.armor(),
                record.armor().saturating_add(magnitude.max(1)),
            ),
            _ if name.eq_ignore_ascii_case("Summon Undine") => (
                "health",
                record.health(),
                record.health().saturating_add(15),
            ),
            _ if name.eq_ignore_ascii_case("Charm") => {
                ("combat", record.combat(), record.combat().saturating_add(5))
            }
            _ => return Err(EconomyError::NothingHappens),
        };
        match stat {
            "health" => record.set_stat(crate::save::Character::HEALTH, new),
            "combat" => record.set_stat(crate::save::Character::COMBAT, new),
            "armor" => record.set_stat(crate::save::Character::ARMOR, new),
            _ => unreachable!(),
        }
        Ok(format!("{name}: {stat} {old} -> {new}"))
    }

    /// Use an object. Equipment is equipped, consumables are consumed and
    /// applied to `target` (the user when `target` is out of range).
    pub fn use_item(
        &mut self,
        character: usize,
        def: usize,
        target: usize,
    ) -> Result<ActionOutcome, EconomyError> {
        let item = self.item_def_cloned(def)?;
        if item.is_equipment() {
            self.equip_item(character, def)?;
            return Ok(ActionOutcome {
                message: format!("{} equipped", item.name()),
                consumed: false,
                equipped: true,
            });
        }
        if item.is_spell() {
            return Err(EconomyError::NothingHappens);
        }
        self.find_carried(character, def, None)
            .ok_or(EconomyError::NotOwned)?;
        let target = if target < self.characters.len() {
            target
        } else {
            character
        };
        let message = self.apply_effect(
            target,
            item.effect_id(),
            item.effect(),
            item.power(),
            &item.name(),
        )?;
        self.remove_item(character, def, 1)?;
        Ok(ActionOutcome {
            message,
            consumed: true,
            equipped: false,
        })
    }

    /// Cast the selected prepared-spell instance. Consuming the exact instance
    /// matters when a character carries two copies of the same spell
    /// (`FUN_1ba8_44f1` passes the packed slot/definition to
    /// `FUN_1ba8_2e27`).
    pub fn cast_spell_slot(
        &mut self,
        character: usize,
        slot: usize,
        def: usize,
        target: usize,
    ) -> Result<ActionOutcome, EconomyError> {
        let item = self.item_def_cloned(def)?;
        if !item.is_spell() {
            return Err(EconomyError::NothingHappens);
        }
        let owner = (character + 1) as u8;
        self.instances
            .get(slot)
            .filter(|instance| {
                instance.item() == def as u8 && instance.owner() == owner && instance.is_carried()
            })
            .ok_or(EconomyError::NotOwned)?;
        let target = if target < self.characters.len() {
            target
        } else {
            character
        };
        let message = self.apply_effect(
            target,
            item.effect_id(),
            item.effect(),
            item.power(),
            &item.name(),
        )?;
        self.instances[slot] = ObjectInstance { raw: [0; 4] };
        Ok(ActionOutcome {
            message,
            consumed: true,
            equipped: false,
        })
    }

    /// Cast a prepared spell. The first carried copy is consumed even if the
    /// target is already at full health.
    pub fn cast_spell(
        &mut self,
        character: usize,
        def: usize,
        target: usize,
    ) -> Result<ActionOutcome, EconomyError> {
        let item = self.item_def_cloned(def)?;
        if !item.is_spell() {
            return Err(EconomyError::NothingHappens);
        }
        let slot = self
            .find_carried(character, def, None)
            .ok_or(EconomyError::NotOwned)?;
        self.cast_spell_slot(character, slot, def, target)
    }

    /// Buy one object. With a seller, the stock instance itself is transferred
    /// (`FUN_1ba8_1005` calls `FUN_1ba8_0431`, which rewrites the owner byte);
    /// without one the catalogue is treated as unlimited stock.
    pub fn buy(
        &mut self,
        buyer: usize,
        seller: Option<usize>,
        def: usize,
    ) -> Result<u16, EconomyError> {
        self.character_ref(buyer)?;
        let item = self.item_def_cloned(def)?;
        if item.is_gold() || item.is_spell() {
            return Err(EconomyError::NoStock);
        }
        let price = item.price().max(1);
        if self.gold(buyer) < price {
            return Err(EconomyError::NotEnoughGold {
                need: price,
                have: self.gold(buyer),
            });
        }
        if let Some(seller) = seller {
            self.character_ref(seller)?;
            if seller == buyer {
                return Err(EconomyError::NoStock);
            }
            let slot = self
                .find_carried(seller, def, None)
                .ok_or(EconomyError::NoStock)?;
            self.spend_gold(buyer, price)?;
            let instance = &mut self.instances[slot];
            instance.set_owner((buyer + 1) as u8);
            instance.set_location(0);
            instance.set_flags(INVENTORY_FLAG);
        } else {
            // Catalogue stock is unlimited, so make sure the carried copy has
            // a free slot before taking the money.
            if self.empty_slot().is_none() {
                return Err(EconomyError::InventoryFull);
            }
            self.spend_gold(buyer, price)?;
            self.add_item(buyer, def, 1)?;
        }
        Ok(price)
    }

    /// Sell one object for half its value. When a buyer is supplied the sold
    /// instance is transferred to them; it is not destroyed and recreated.
    pub fn sell(
        &mut self,
        seller: usize,
        buyer: Option<usize>,
        def: usize,
    ) -> Result<u16, EconomyError> {
        self.character_ref(seller)?;
        let item = self.item_def_cloned(def)?;
        if item.is_gold() || item.is_spell() {
            return Err(EconomyError::NoStock);
        }
        let price = (item.price().max(1) / 2).max(1);
        if let Some(buyer) = buyer {
            self.character_ref(buyer)?;
            if buyer == seller {
                return Err(EconomyError::NoStock);
            }
            let slot = self
                .find_carried(seller, def, None)
                .ok_or(EconomyError::NotOwned)?;
            let instance = &mut self.instances[slot];
            instance.set_owner((buyer + 1) as u8);
            instance.set_location(0);
            instance.set_flags(INVENTORY_FLAG);
        } else {
            self.remove_item(seller, def, 1)?;
        }
        self.add_gold(seller, price)?;
        Ok(price)
    }

    /// Decode a character sheet including equipment bonuses. Combat adds the
    /// equipped weapon's power; armour adds the effect values of equipped
    /// armour pieces. Temporary spell effects live in the stat bytes.
    #[must_use]
    pub fn character_sheet(&self, character: usize) -> Option<CharacterSheet> {
        let record = self.characters.get(character)?;
        let inventory = self.inventory(character);
        let equipment: Vec<_> = inventory
            .iter()
            .filter(|entry| entry.equipped)
            .cloned()
            .collect();
        let spells = inventory
            .iter()
            .filter(|entry| self.item_def(entry.def).is_some_and(Item::is_spell))
            .cloned()
            .collect();
        let mut combat_bonus = 0u16;
        let mut armor_bonus = 0u16;
        for entry in &equipment {
            let Some(item) = self.item_def(entry.def) else {
                continue;
            };
            match item.kind() {
                55..=57 => combat_bonus += u16::from(item.power()),
                58..=62 => armor_bonus += u16::from(item.effect()),
                _ => {}
            }
        }
        Some(CharacterSheet {
            index: character,
            name: record.name(),
            health: record.health(),
            hits: record.hits(),
            movement: record.movement(),
            combat: record.combat(),
            armor: record.armor(),
            nobility: record.nobility(),
            faith: record.faith(),
            magic: record.magic(),
            level: record.level(),
            gold: record.gold(),
            effective_combat: record.combat().saturating_add(combat_bonus as u8),
            effective_armor: record.armor().saturating_add(armor_bonus as u8),
            equipment,
            inventory,
            spells,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save::{CHAR_SIZE, Character, FILE_SIZE, GameState, Item};

    fn state() -> GameState {
        let mut state = GameState::parse(&[0u8; FILE_SIZE]).unwrap();
        // Character 0: fighter with room to heal.
        let mut hero = Character {
            raw: [0; CHAR_SIZE],
        };
        hero.set_name("Hero");
        hero.set_stat(Character::HEALTH, 10);
        hero.set_stat(Character::HITS, 12);
        hero.set_stat(Character::MOVEMENT, 8);
        hero.set_stat(Character::COMBAT, 7);
        hero.set_stat(Character::ARMOR, 5);
        hero.set_stat(Character::NOBILITY, 4);
        hero.set_stat(Character::FAITH, 6);
        hero.set_stat(Character::MAGIC, 9);
        hero.set_gold(100);
        state.characters[0] = hero;
        // Character 1: recipient/merchant.
        let mut friend = Character {
            raw: [0; CHAR_SIZE],
        };
        friend.set_name("Friend");
        friend.set_stat(Character::HEALTH, 20);
        state.characters[1] = friend;

        state.items[0] = Item {
            raw: [0; crate::save::ITEM_SIZE],
        };
        state.items[0].set_name("gold besant");
        state.items[0].raw[Item::KIND] = 49;
        state.items[1] = Item {
            raw: [0; crate::save::ITEM_SIZE],
        };
        state.items[1].set_name("healing balm");
        state.items[1].raw[Item::KIND] = 33;
        state.items[1].raw[Item::POWER] = 1;
        state.items[1].raw[Item::EFFECT] = 3;
        state.items[1].raw[Item::EFFECT_ID] = 44;
        state.items[2] = Item {
            raw: [0; crate::save::ITEM_SIZE],
        };
        state.items[2].set_name("sword");
        state.items[2].raw[Item::KIND] = 56;
        state.items[2].raw[Item::POWER] = 5;
        state.items[2].raw[Item::EFFECT] = 5;
        state.items[2].raw[Item::EFFECT_ID] = 45;
        state.items[3] = Item {
            raw: [0; crate::save::ITEM_SIZE],
        };
        state.items[3].set_name("helm");
        state.items[3].raw[Item::KIND] = 59;
        state.items[3].raw[Item::POWER] = 2;
        state.items[3].raw[Item::EFFECT] = 4;
        state.items[3].raw[Item::EFFECT_ID] = 46;
        state.items[4] = Item {
            raw: [0; crate::save::ITEM_SIZE],
        };
        state.items[4].set_name("Heal");
        state.items[4].raw[Item::EFFECT] = 10;
        state.items[4].raw[Item::EFFECT_ID] = 44;
        state.items[5] = Item {
            raw: [0; crate::save::ITEM_SIZE],
        };
        state.items[5].set_name("Shield");
        state.items[5].raw[Item::EFFECT] = 10;
        state.items[5].raw[Item::EFFECT_ID] = 46;
        state
    }

    fn add(state: &mut GameState, character: usize, def: usize) {
        state.add_item(character, def, 1).unwrap();
    }

    #[test]
    fn decodes_character_stats_gold_and_equipment() {
        let mut state = state();
        add(&mut state, 0, 3);
        state.equip_item(0, 3).unwrap();
        add(&mut state, 0, 4);
        state.equip_item(0, 4).unwrap();
        let sheet = state.character_sheet(0).unwrap();
        assert_eq!(sheet.name, "Hero");
        assert_eq!(sheet.health, 10);
        assert_eq!(sheet.gold, 100);
        assert_eq!(sheet.equipment.len(), 2);
        assert_eq!(sheet.effective_combat, 12);
        assert_eq!(sheet.effective_armor, 9);
    }

    #[test]
    fn use_consumes_item_and_applies_effect() {
        let mut state = state();
        add(&mut state, 0, 2);
        let before = state.to_bytes();
        let outcome = state.use_item(0, 2, 0).unwrap();
        assert!(outcome.consumed);
        assert!(outcome.message.contains("health"));
        assert_eq!(state.characters[0].health(), 14);
        assert!(state.inventory(0).is_empty());
        assert_ne!(state.to_bytes(), before);
    }

    #[test]
    fn drop_moves_item_to_scene_and_pickup_returns_it() {
        let mut state = state();
        add(&mut state, 0, 2);
        let before = state.to_bytes();
        let slot = state.drop_item(0, 2, 7).unwrap();
        assert!(state.inventory(0).is_empty());
        assert!(!state.instances[slot].is_carried());
        assert_eq!(state.instances[slot].location(), 7);
        assert_eq!(state.ground_items(7).len(), 1);
        assert_ne!(state.to_bytes(), before);
        state.pickup_item(0, slot, 7).unwrap();
        assert_eq!(state.inventory(0).len(), 1);
        assert!(state.instances[slot].is_carried());
    }

    #[test]
    fn give_transfers_owner() {
        let mut state = state();
        add(&mut state, 0, 2);
        let before = state.to_bytes();
        state.give_item(0, 1, 2, 1).unwrap();
        assert!(state.inventory(0).is_empty());
        assert_eq!(state.inventory(1).len(), 1);
        assert_ne!(state.to_bytes(), before);
    }

    #[test]
    fn equip_changes_flags_and_replaces_same_slot() {
        let mut state = state();
        add(&mut state, 0, 3);
        add(&mut state, 0, 4);
        let before = state.to_bytes();
        state.equip_item(0, 3).unwrap();
        assert!(state.inventory(0).iter().any(|e| e.def == 3 && e.equipped));
        state.equip_item(0, 4).unwrap();
        let sheet = state.character_sheet(0).unwrap();
        assert_eq!(sheet.equipment.len(), 2);
        assert!(sheet.equipment.iter().any(|e| e.def == 4));
        assert!(sheet.equipment.iter().any(|e| e.def == 3));
        assert_ne!(state.to_bytes(), before);
    }

    #[test]
    fn buy_and_sell_change_gold_and_items() {
        let mut state = state();
        // Merchant (character 1) stocks a sword.
        add(&mut state, 1, 3);
        let buyer_gold = state.gold(0);
        let before = state.to_bytes();
        let price = state.buy(0, Some(1), 3).unwrap();
        assert_eq!(price, 5);
        assert_eq!(state.gold(0), buyer_gold - price);
        assert!(state.inventory(0).iter().any(|e| e.def == 3));
        assert!(state.inventory(1).is_empty());
        assert_ne!(state.to_bytes(), before);

        let before_sell = state.to_bytes();
        let earned = state.sell(0, Some(1), 3).unwrap();
        assert_eq!(earned, 2);
        assert_eq!(state.gold(0), buyer_gold - price + earned);
        assert!(state.inventory(0).is_empty());
        assert_eq!(state.inventory(1).len(), 1);
        assert_ne!(state.to_bytes(), before_sell);
    }

    #[test]
    fn buy_rejects_empty_purse_or_missing_stock() {
        let mut state = state();
        state.characters[0].set_gold(1);
        let err = state.buy(0, None, 3).unwrap_err();
        assert!(matches!(err, EconomyError::NotEnoughGold { .. }));

        state.characters[0].set_gold(100);
        let err = state.buy(0, Some(1), 3).unwrap_err();
        assert_eq!(err, EconomyError::NoStock);
    }

    #[test]
    fn casting_consumes_the_selected_spell_and_applies_effect() {
        let mut state = state();
        add(&mut state, 0, 5); // Heal
        add(&mut state, 0, 6); // Shield
        let before = state.to_bytes();
        let outcome = state.cast_spell(0, 5, 0).unwrap();
        assert!(outcome.consumed);
        assert_eq!(state.characters[0].health(), 20);
        assert!(state.inventory(0).iter().all(|entry| entry.def != 5));
        assert!(state.inventory(0).iter().any(|entry| entry.def == 6));
        assert_ne!(state.to_bytes(), before);

        let outcome = state.cast_spell(0, 6, 0).unwrap();
        assert!(outcome.message.contains("armor"));
        assert_eq!(state.characters[0].armor(), 15);
        assert!(state.inventory(0).is_empty());
    }

    #[test]
    fn casting_without_the_spell_fails_without_changes() {
        let mut state = state();
        let before = state.to_bytes();
        let err = state.cast_spell(0, 5, 0).unwrap_err();
        assert_eq!(err, EconomyError::NotOwned);
        assert_eq!(state.to_bytes(), before);
    }

    #[test]
    fn inventory_groups_duplicate_instances_like_1ba8_078f() {
        let mut state = state();
        state.add_item(0, 2, 3).unwrap();
        let rows = state.inventory(0);
        assert_eq!(rows.len(), 1, "three instances become one row");
        assert_eq!(rows[0].def, 2);
        assert_eq!(rows[0].quantity, 3);

        let before = state.to_bytes();
        state.use_item(0, 2, 0).unwrap();
        let rows = state.inventory(0);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].quantity, 2, "using one leaves two");
        assert_ne!(state.to_bytes(), before);
    }

    #[test]
    fn buy_from_a_seller_transfers_the_stock_instance() {
        let mut state = state();
        add(&mut state, 1, 3);
        let slot = state
            .instances
            .iter()
            .position(|instance| instance.item() == 3)
            .expect("merchant stock");
        let before = state.instances[slot];
        let gold = state.gold(0);
        let price = state.buy(0, Some(1), 3).unwrap();
        assert_eq!(state.gold(0), gold - price);
        assert_eq!(state.instances[slot].item(), before.item());
        assert_eq!(state.instances[slot].location(), before.location());
        assert_eq!(state.instances[slot].flags(), before.flags());
        assert_eq!(state.instances[slot].owner(), 1);
        assert!(state.instances[slot].is_carried());
        assert!(state.inventory(1).is_empty());
    }

    #[test]
    fn selling_to_a_buyer_transfers_the_instance_and_pays_half() {
        let mut state = state();
        add(&mut state, 0, 3);
        let slot = state
            .instances
            .iter()
            .position(|instance| instance.item() == 3)
            .expect("carried sword");
        let gold = state.gold(0);
        let earned = state.sell(0, Some(1), 3).unwrap();
        assert_eq!(earned, 2);
        assert_eq!(state.gold(0), gold + earned);
        assert_eq!(state.instances[slot].owner(), 2);
        assert!(state.instances[slot].is_carried());
        assert_eq!(state.inventory(1).len(), 1);
    }

    #[test]
    fn failed_catalogue_purchase_does_not_spend_gold() {
        let mut state = state();
        for instance in &mut state.instances {
            *instance = carried_instance(2);
        }
        let before = state.to_bytes();
        let err = state.buy(0, None, 3).unwrap_err();
        assert_eq!(err, EconomyError::InventoryFull);
        assert_eq!(state.gold(0), 100);
        assert_eq!(state.to_bytes(), before);
    }

    fn carried_instance(def: u8) -> ObjectInstance {
        ObjectInstance {
            raw: [def, 1, 0, INVENTORY_FLAG],
        }
    }

    #[test]
    fn casting_consumes_the_selected_duplicate_spell_instance() {
        let mut state = state();
        add(&mut state, 0, 5);
        add(&mut state, 0, 5);
        let slots: Vec<usize> = state
            .instances
            .iter()
            .enumerate()
            .filter(|(_, instance)| instance.item() == 5)
            .map(|(slot, _)| slot)
            .collect();
        assert_eq!(slots.len(), 2);
        let (first, second) = (slots[0], slots[1]);
        state.cast_spell_slot(0, second, 5, 0).unwrap();
        assert_eq!(state.instances[second].item(), 0, "selected copy consumed");
        assert_eq!(state.instances[first].item(), 5, "other copy remains");
        assert_eq!(state.characters[0].health(), 20);
    }

    #[test]
    fn real_episode_decodes_object_table_and_instances() {
        let path = std::path::Path::new("../../game/EPISODE1.DAT");
        let Ok(data) = std::fs::read(path) else {
            return;
        };
        let state = GameState::parse(&data).unwrap();
        assert_eq!(state.items.len(), 49);
        assert_eq!(state.instances.len(), 85);
        assert_eq!(state.item_def(1).unwrap().name(), "gold besant");
        assert_eq!(state.item_def(36).unwrap().name(), "Repletion");
        assert!(state.items[0].is_gold());
        assert!(state.item_def(36).unwrap().is_spell());
    }
}
