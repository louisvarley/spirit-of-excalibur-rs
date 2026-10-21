//! Event/NPC interaction: room character lists, dialogue triggers and choices.
//!
//! Ported from the `1ba8` interaction helpers:
//! - `FUN_1ba8_175a` / `FUN_2a6e_01e7` build the selectable room list from
//!   character presence flags and follower chains.
//! - `FUN_1ba8_22ef` (Talk) picks a target and calls `FUN_26c5_071a`.
//! - `FUN_26c5_071a` scans the 80 event records at file `0x1A4F` for the
//!   target actor and resolves the line through `HINT.RES` (`%R`).
//!
//! Give/Drop/Pickup/Use themselves live in [`crate::economy`], which mirrors
//! `FUN_1ba8_0431`, `FUN_1ba8_01ff`, `FUN_1ba8_1005`, `FUN_1ba8_1df0` and
//! `FUN_1ba8_2355`.

#![forbid(unsafe_code)]

use crate::save::{EventRecord, GameState};
use crate::story::HintBook;

/// One selectable answer in a dialogue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChoiceKind {
    /// Plain acknowledgement; no persistent effect.
    Continue,
    /// Accept a `0x4000` join offer.
    JoinParty,
    /// Decline a join offer.
    StayHere,
    /// Dismiss a `0x2000` plea.
    IgnorePlea,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogueChoice {
    pub label: String,
    pub kind: ChoiceKind,
}

/// An open dialogue panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialogue {
    /// 0-based character index of the speaker (the NPC).
    pub speaker: usize,
    /// 0-based character index of the listener (the current actor).
    pub listener: usize,
    /// Event table slot that produced the line.
    pub event: usize,
    /// 1-based `HINT.RES` record id.
    pub line_id: u16,
    /// Spoken text, without the speaker/listener header.
    pub text: String,
    pub choices: Vec<DialogueChoice>,
    pub selected: usize,
}

impl GameState {
    /// Best event record for speaking to `target` (0-based character index).
    ///
    /// The original picks the lowest `flags & 0x70` priority among records whose
    /// actor matches. Records with no line are ignored.
    #[must_use]
    pub fn event_for(&self, target: usize) -> Option<(usize, &EventRecord)> {
        let actor = u8::try_from(target + 1).ok()?;
        self.events
            .iter()
            .enumerate()
            .filter(|(_, event)| event.actor() == actor && event.line() != 0)
            .min_by_key(|(_, event)| event.priority())
    }

    /// Characters selectable in the current room.
    ///
    /// Runtime presence is character `+0x16` bit 0x08 (file `+0x20`). The
    /// current actor is always included even if the capture predates the
    /// presence bit being set; a named fallback keeps headless fixtures usable.
    #[must_use]
    pub fn room_characters(&self, actor: usize) -> Vec<usize> {
        let mut out = Vec::new();
        for (index, character) in self.characters.iter().enumerate() {
            if character.name().is_empty() {
                continue;
            }
            if index == actor || character.is_present() {
                out.push(index);
            }
        }
        if out.is_empty() {
            out.extend(
                self.characters
                    .iter()
                    .enumerate()
                    .filter(|(_, character)| !character.name().is_empty())
                    .map(|(index, _)| index)
                    .take(16),
            );
        }
        if actor < self.characters.len()
            && !self.characters[actor].name().is_empty()
            && !out.contains(&actor)
        {
            out.insert(0, actor);
        }
        out
    }

    /// The spoken line for `target`, if the event table has one.
    #[must_use]
    pub fn event_line(&self, target: usize) -> Option<u16> {
        self.event_for(target).map(|(_, event)| event.line())
    }
}

impl Dialogue {
    /// Build the Talk panel for `listener` talking to `speaker`.
    #[must_use]
    pub fn talk(
        state: &GameState,
        story: Option<&HintBook>,
        listener: usize,
        speaker: usize,
    ) -> Option<Self> {
        let (event_index, event) = state.event_for(speaker)?;
        let line_id = event.line();
        let text = story
            .and_then(|book| book.line(line_id))
            .map(str::to_owned)
            .unwrap_or_else(|| fallback_line(state, speaker));
        let mut choices = Vec::new();
        if event.offers_to_join() {
            choices.push(DialogueChoice {
                label: "Welcome them".into(),
                kind: ChoiceKind::JoinParty,
            });
            choices.push(DialogueChoice {
                label: "Stay here".into(),
                kind: ChoiceKind::StayHere,
            });
        } else if event.is_plea() {
            choices.push(DialogueChoice {
                label: "Hear them out".into(),
                kind: ChoiceKind::Continue,
            });
            choices.push(DialogueChoice {
                label: "Ignore plea".into(),
                kind: ChoiceKind::IgnorePlea,
            });
        }
        Some(Self {
            speaker,
            listener,
            event: event_index,
            line_id,
            text,
            choices,
            selected: 0,
        })
    }

    #[must_use]
    pub fn has_choices(&self) -> bool {
        !self.choices.is_empty()
    }

    pub fn move_selection(&mut self, delta: isize) {
        if self.choices.is_empty() {
            return;
        }
        let len = self.choices.len() as isize;
        self.selected = (self.selected as isize + delta).rem_euclid(len) as usize;
    }

    /// Apply the highlighted choice. Returns a short result message.
    pub fn apply_selected(&mut self, state: &mut GameState) -> String {
        let Some(choice) = self.choices.get(self.selected).cloned() else {
            return String::new();
        };
        let speaker = character_name(state, self.speaker);
        let mut message = match choice.kind {
            ChoiceKind::Continue => {
                if let Some(event) = state.events.get_mut(self.event) {
                    event.set_flags(event.flags() | 0x2000);
                }
                format!("You hear {speaker} out.")
            }
            ChoiceKind::JoinParty => {
                if let Some(event) = state.events.get_mut(self.event) {
                    event.set_flags(event.flags() & !0x4000);
                }
                format!("{speaker} joins the party.")
            }
            ChoiceKind::StayHere => {
                if let Some(event) = state.events.get_mut(self.event) {
                    event.set_flags(event.flags() & !0x4000);
                }
                format!("{speaker} stays here.")
            }
            ChoiceKind::IgnorePlea => {
                if let Some(event) = state.events.get_mut(self.event) {
                    event.set_flags(event.flags() | 0x2000);
                }
                format!("You ignore {speaker}'s plea.")
            }
        };
        message.push_str(&format!(" (line {})", self.line_id));
        message
    }
}

fn fallback_line(state: &GameState, speaker: usize) -> String {
    let name = character_name(state, speaker);
    format!("{name} has nothing more to say.")
}

/// Display name for a character, or `"someone"`.
#[must_use]
pub fn character_name(state: &GameState, index: usize) -> String {
    state
        .characters
        .get(index)
        .map(crate::save::Character::name)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "someone".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn game() -> PathBuf {
        std::env::var_os("SOE_GAME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("../../game"))
    }

    fn example() -> Option<GameState> {
        let data = std::fs::read(game().join("EXAMPLE.SAV")).ok()?;
        GameState::parse(&data).ok()
    }

    fn book() -> Option<HintBook> {
        match HintBook::load(game().join("HINT.RES")) {
            Ok(book) => Some(book),
            Err(error) => {
                eprintln!("HINT.RES: {error}");
                None
            }
        }
    }

    #[test]
    fn event_table_picks_friar_welcome_line() {
        let Some(state) = example() else {
            eprintln!("SKIP: EXAMPLE.SAV absent");
            return;
        };
        // 45 is the 0-based friar record; the event table stores actor 46.
        let (slot, event) = state.event_for(45).expect("friar event");
        assert_eq!(state.characters[45].name(), "friar");
        assert_eq!(event.line(), 120);
        assert_eq!(slot, 18);
        assert!(!event.offers_to_join());
    }

    #[test]
    fn hint_lines_are_keyed_by_record_id() {
        let Some(book) = book() else {
            eprintln!("SKIP: HINT.RES absent");
            return;
        };
        assert!(book.line(120).unwrap().contains("we welcome you"));
        let day = book.line(184).unwrap();
        assert!(
            day.contains("day to you, Lord Constantine"),
            "line 184 = {day:?}"
        );
        assert!(book.line(107).unwrap().contains("Lupinus"));
    }

    #[test]
    fn talk_builds_the_right_friar_text() {
        let (Some(state), Some(book)) = (example(), book()) else {
            eprintln!("SKIP: EXAMPLE.SAV or HINT.RES absent");
            return;
        };
        let dialogue = Dialogue::talk(&state, Some(&book), 0, 45).expect("dialogue");
        assert_eq!(dialogue.speaker, 45);
        assert_eq!(dialogue.listener, 0);
        assert_eq!(dialogue.line_id, 120);
        assert!(dialogue.text.contains("St. Albans"));
        assert_eq!(dialogue.choices.len(), 0);
    }

    #[test]
    fn join_choice_changes_event_state() {
        let Some(mut state) = example() else {
            eprintln!("SKIP: EXAMPLE.SAV absent");
            return;
        };
        // Bellengerus (0-based 5) has the only 0x4000 join offer.
        let (slot, event) = state.event_for(5).expect("Bellengerus event");
        assert_eq!(slot, 43);
        assert!(event.offers_to_join());
        let mut dialogue = Dialogue::talk(&state, None, 0, 5).expect("dialogue");
        assert_eq!(dialogue.choices.len(), 2);
        dialogue.selected = 0;
        let message = dialogue.apply_selected(&mut state);
        assert!(message.contains("joins the party"));
        assert!(!state.events[slot].offers_to_join());
    }

    #[test]
    fn room_list_contains_actor_and_present_npcs() {
        let Some(state) = example() else {
            eprintln!("SKIP: EXAMPLE.SAV absent");
            return;
        };
        let room = state.room_characters(0);
        assert!(room.contains(&0), "actor present");
        assert!(room.contains(&45), "friar present");
        assert!(state.characters[45].is_present());
    }
}
