//! Sound boundary. The original uses SMUS music and driver-specific output;
//! the headless implementation records requests for tests.

#![forbid(unsafe_code)]

use soe_assets::ResourceId;

/// Opaque handle returned when music starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MusicHandle(pub u32);

/// The platform sound boundary.
pub trait Sound {
    /// Start an SMUS resource. Returns `None` if unsupported.
    fn play_music(&mut self, id: &ResourceId) -> Option<MusicHandle>;
    fn stop_music(&mut self);
    /// Play a one-shot effect/sample.
    fn play_effect(&mut self, id: &ResourceId);
    /// Master volume 0..=255.
    fn set_volume(&mut self, volume: u8);
    /// Advance the sequencer/OPL emulation by one tick.
    fn tick(&mut self);
}

/// Records sound requests without producing audio.
#[derive(Debug, Default, Clone)]
pub struct HeadlessSound {
    pub music: Vec<ResourceId>,
    pub effects: Vec<ResourceId>,
    pub volume: u8,
    pub ticks: u64,
    next: u32,
}

impl HeadlessSound {
    #[must_use]
    pub fn new() -> Self {
        Self {
            volume: 255,
            ..Self::default()
        }
    }
}

impl Sound for HeadlessSound {
    fn play_music(&mut self, id: &ResourceId) -> Option<MusicHandle> {
        self.music.push(id.clone());
        self.next += 1;
        Some(MusicHandle(self.next))
    }

    fn stop_music(&mut self) {}

    fn play_effect(&mut self, id: &ResourceId) {
        self.effects.push(id.clone());
    }

    fn set_volume(&mut self, volume: u8) {
        self.volume = volume;
    }

    fn tick(&mut self) {
        self.ticks += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_music_and_effects() {
        let mut sound = HeadlessSound::new();
        let music = ResourceId::new("MUSIC.RES", 2, None::<String>);
        let effect = ResourceId::new("EXTER.RES", 1000, None::<String>);
        assert!(sound.play_music(&music).is_some());
        sound.play_effect(&effect);
        sound.set_volume(100);
        sound.tick();
        assert_eq!(sound.music, vec![music]);
        assert_eq!(sound.effects, vec![effect]);
        assert_eq!(sound.volume, 100);
        assert_eq!(sound.ticks, 1);
    }
}
