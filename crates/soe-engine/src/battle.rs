//! The outcome of an army battle.
//!
//! The battle itself runs on the ported engine in [`crate::army`]
//! (`FUN_2449_134e`); this headless round model was replaced by it.

#![forbid(unsafe_code)]

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
