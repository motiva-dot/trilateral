//! presentation — the Phase 3.5 debug view.
//!
//! # The one law this crate must not break
//! §1.6: presentation **reads** simulation state and never writes it. Every
//! function here takes `&SimState`, never `&mut`. That is not a style
//! preference — a render path that could nudge simulation state would be a
//! desync generator that only fires on the machine with the window open, and
//! it would never show up in the headless arena.
//!
//! # This is deliberately a throwaway
//! IMPLEMENTATION_PLAN Phase 3.5: flat circles, no SDF, no art, no HUD chrome.
//! It exists so every phase from movement onward can be *watched*, because
//! feel cannot be judged headlessly and the architect's contribution to this
//! project is feel. Phase 9L promotes it to something playable without
//! apology. Ugly is allowed here; wrong about decoupling is not.
//!
//! # Interpolation is here from the first frame, on purpose
//! TECH_SPEC §8: hold state at T-1 and T, lerp with an `f64` alpha, never draw
//! raw fixed positions. Retrofitting this later is how engines end up with
//! motion that looks stepped at high refresh rates, and by then every camera
//! and effect assumes the un-interpolated positions.

pub mod app;
pub mod camera;
pub mod render_state;
pub mod renderer;

pub use camera::Camera;
pub use render_state::{Instance, RenderState};
pub use renderer::Renderer;

/// Simulation ticks per second. Must match `SimClock::TICKS_PER_SECOND`;
/// asserted in tests rather than assumed.
pub const TICK_HZ: f64 = 30.0;
/// Seconds per simulation tick. The fixed timestep of TECH_SPEC §5.
pub const TICK_DT: f64 = 1.0 / TICK_HZ;

/// The fixed-step accumulator from TECH_SPEC §5.
///
/// ```text
/// accumulator += real_frame_time
/// while accumulator >= TICK_DT { sim_tick(); accumulator -= TICK_DT }
/// alpha = accumulator / TICK_DT
/// ```
///
/// `real_frame_time` is the only wall-clock reading in the entire project, and
/// it lives here rather than in the simulation because §1.1 says simulation
/// time is `Tick(u64)` and nothing else. What crosses the boundary is a *tick
/// count*, never a duration.
#[derive(Clone, Copy, Debug, Default)]
pub struct StepAccumulator {
    seconds: f64,
}

impl StepAccumulator {
    pub const fn new() -> StepAccumulator {
        StepAccumulator { seconds: 0.0 }
    }

    /// Feed elapsed real time; returns how many sim ticks are owed.
    ///
    /// Clamps the frame time to avoid the "spiral of death": after a long
    /// stall (a breakpoint, a moved window, a laptop lid) an unclamped
    /// accumulator would demand hundreds of ticks at once, take longer than
    /// real time to run them, and fall further behind every frame.
    pub fn advance(&mut self, real_seconds: f64) -> u32 {
        const MAX_FRAME: f64 = 0.25;
        // Clamped at both ends: the upper bound stops the spiral, the lower
        // bound absorbs the zero or backwards delta some platforms report
        // across a resume.
        self.seconds += real_seconds.clamp(0.0, MAX_FRAME);
        let mut owed = 0;
        while self.seconds >= TICK_DT {
            self.seconds -= TICK_DT;
            owed += 1;
        }
        owed
    }

    /// Fraction of the way from the last tick to the next, in `[0, 1)`.
    #[inline]
    pub fn alpha(&self) -> f64 {
        self.seconds / TICK_DT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_rate_matches_the_simulation() {
        // If these ever disagree, rendering interpolates against the wrong
        // timestep and everything looks subtly wrong with no error anywhere.
        assert_eq!(TICK_HZ as u64, sim_core::SimClock::TICKS_PER_SECOND);
    }

    #[test]
    fn exactly_one_tick_per_dt() {
        let mut a = StepAccumulator::new();
        assert_eq!(a.advance(TICK_DT), 1);
        assert!(a.alpha() < 1e-9, "alpha should be ~0 right after a tick");
    }

    #[test]
    fn partial_frames_accumulate_rather_than_being_lost() {
        let mut a = StepAccumulator::new();
        assert_eq!(a.advance(TICK_DT / 2.0), 0);
        assert!((a.alpha() - 0.5).abs() < 1e-9);
        assert_eq!(a.advance(TICK_DT / 2.0), 1);
    }

    #[test]
    fn a_slow_frame_owes_several_ticks() {
        let mut a = StepAccumulator::new();
        assert_eq!(a.advance(TICK_DT * 3.0), 3);
    }

    #[test]
    fn a_long_stall_is_clamped_rather_than_spiralling() {
        // Ten seconds of stall would be 300 ticks; clamped to 0.25s = 7.
        let mut a = StepAccumulator::new();
        let owed = a.advance(10.0);
        assert!(owed <= 8, "owed {owed} ticks after a stall — spiral risk");
    }

    #[test]
    fn negative_or_zero_frame_times_are_harmless() {
        // Some platforms report a zero or backwards delta across a resume.
        let mut a = StepAccumulator::new();
        assert_eq!(a.advance(-1.0), 0);
        assert_eq!(a.advance(0.0), 0);
        assert!(a.alpha() >= 0.0);
    }

    #[test]
    fn alpha_stays_in_the_unit_interval() {
        let mut a = StepAccumulator::new();
        for i in 0..1000 {
            a.advance(0.001 * (i % 7) as f64);
            let al = a.alpha();
            assert!((0.0..1.0).contains(&al), "alpha escaped: {al}");
        }
    }
}
