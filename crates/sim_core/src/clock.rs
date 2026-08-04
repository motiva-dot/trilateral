//! Simulation time. CLAUDE.md §1.1: "Time in the simulation is `Tick(u64)`.
//! There is no delta time, no wall clock."
//!
//! Every duration in this project is an integer tick count loaded from YAML.
//! If you ever find yourself wanting a `dt` here, what you actually want is
//! either a tick count or a presentation-side interpolation factor — and the
//! second of those lives in `presentation`, never in sim state.

use serde::{Deserialize, Serialize};

/// Simulation tick number. Monotonic, never reset within a match.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize,
)]
#[repr(transparent)]
pub struct Tick(pub u64);

impl Tick {
    pub const ZERO: Tick = Tick(0);

    #[inline]
    pub const fn next(self) -> Tick {
        Tick(self.0 + 1)
    }

    /// Ticks elapsed since `earlier`, saturating at zero.
    #[inline]
    pub const fn since(self, earlier: Tick) -> u64 {
        self.0.saturating_sub(earlier.0)
    }

    /// Whether this tick is on a `period`-tick boundary. The staggering
    /// primitive for §5.3 target acquisition and §3.2 step 17 hashing.
    ///
    /// # Panics
    /// If `period` is zero.
    #[inline]
    pub const fn is_multiple_of(self, period: u64) -> bool {
        assert!(period != 0, "Tick::is_multiple_of(0)");
        // std's `is_multiple_of` treats a zero divisor as "only zero divides
        // it"; the assert above is what makes a zero period a caller bug
        // instead of a silently surprising answer.
        self.0.is_multiple_of(period)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct SimClock {
    pub tick: Tick,
}

impl SimClock {
    /// Ticks per second. The 30Hz baseline from audit item 7.
    ///
    /// This is the one timing constant that is *not* YAML-driven, because it
    /// is not a gameplay number — it is the unit in which every gameplay
    /// number is denominated. Changing it is an engine change (and a
    /// determinism break), not a balance change.
    pub const TICKS_PER_SECOND: u64 = 30;

    #[inline]
    pub const fn new() -> SimClock {
        SimClock { tick: Tick::ZERO }
    }

    #[inline]
    pub const fn advance(&mut self) {
        self.tick = self.tick.next();
    }

    /// Whole seconds elapsed. Debug and UI only — never branch simulation
    /// logic on this, branch on tick counts.
    #[inline]
    pub const fn seconds(&self) -> u64 {
        self.tick.0 / Self::TICKS_PER_SECOND
    }

    #[inline]
    pub fn hash_into(&self, h: &mut crate::hash::SimHasher) {
        h.write_u64(self.tick.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_clock_starts_at_zero() {
        assert_eq!(SimClock::new().tick, Tick::ZERO);
    }

    #[test]
    fn advance_increments_by_exactly_one() {
        let mut c = SimClock::new();
        for expected in 1..=100u64 {
            c.advance();
            assert_eq!(c.tick, Tick(expected));
        }
    }

    #[test]
    fn seconds_matches_the_thirty_hertz_baseline() {
        let mut c = SimClock::new();
        for _ in 0..29 {
            c.advance();
        }
        assert_eq!(c.seconds(), 0, "29 ticks is still second zero");
        c.advance();
        assert_eq!(c.seconds(), 1, "tick 30 is one second");
    }

    #[test]
    fn since_saturates_rather_than_underflowing() {
        assert_eq!(Tick(10).since(Tick(4)), 6);
        assert_eq!(Tick(4).since(Tick(10)), 0, "must saturate, not wrap");
    }

    #[test]
    fn is_multiple_of_drives_staggering() {
        assert!(Tick(0).is_multiple_of(8));
        assert!(!Tick(7).is_multiple_of(8));
        assert!(Tick(16).is_multiple_of(8));
        // The §3.2 step 17 case: hash every 10 ticks.
        assert!(Tick(1000).is_multiple_of(10));
    }

    #[test]
    #[should_panic(expected = "is_multiple_of(0)")]
    fn is_multiple_of_zero_panics() {
        let _ = Tick(1).is_multiple_of(0);
    }
}
