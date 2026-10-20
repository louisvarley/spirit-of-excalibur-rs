//! Timer boundary mirroring the DOS INT 1Ah tick counter (18.2 Hz).

#![forbid(unsafe_code)]

/// One BIOS tick is 1/18.2 s, approximately 54.945 ms.
pub const TICK_MS: f64 = 1000.0 / 18.2;

/// The platform timer boundary.
pub trait Timer {
    /// BIOS-style tick count, wrapping at 24 hours as the original does.
    fn ticks(&self) -> u32;
    /// Milliseconds since an arbitrary epoch.
    fn elapsed_ms(&self) -> u64;
}

/// Deterministic tick source for tests and scripted runs.
#[derive(Debug, Default, Clone)]
pub struct HeadlessTimer {
    ticks: u32,
}

impl HeadlessTimer {
    #[must_use]
    pub fn new(ticks: u32) -> Self {
        Self { ticks }
    }

    /// Advance by `n` BIOS ticks.
    pub fn advance_ticks(&mut self, n: u32) {
        self.ticks = self.ticks.wrapping_add(n) % 1_573_040; // 24h * 18.2Hz
    }
}

impl Timer for HeadlessTimer {
    fn ticks(&self) -> u32 {
        self.ticks
    }

    fn elapsed_ms(&self) -> u64 {
        (f64::from(self.ticks) * TICK_MS) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_ticks_to_ms() {
        let mut t = HeadlessTimer::new(18);
        assert_eq!(t.ticks(), 18);
        assert!((t.elapsed_ms() as i64 - 989).abs() <= 2);
        t.advance_ticks(2);
        assert_eq!(t.ticks(), 20);
    }

    #[test]
    fn wraps_at_24_hours() {
        let mut t = HeadlessTimer::new(1_573_039);
        t.advance_ticks(2);
        assert_eq!(t.ticks(), 1);
    }
}
