//! Race macro mechanics. GAME_DESIGN §2.
//!
//! §2's design law is that races differ in *how you produce and expand* before
//! they differ in unit stats. These are the parameters that make that true.

use trilateral_fixed::Fixed;

/// Murmur's Brood Pool (GAME_DESIGN §2.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BroodParams {
    /// Ticks per Brood Point, per base.
    pub period_ticks: u32,
    /// Stockpile cap contributed by each base.
    pub stockpile_cap_per_base: u16,
}

impl Default for BroodParams {
    /// Tests and tools only. Real values come from `races.yaml` (§1.8).
    fn default() -> Self {
        BroodParams {
            period_ticks: 90,
            stockpile_cap_per_base: 3,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RaceParams {
    pub brood: BroodParams,
}

/// Cost of putting a unit into production, resolved from the registry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TrainCost {
    pub ore: u32,
    pub flux: u32,
    pub ticks: u32,
    pub supply_x2: u16,
    /// Murmur pays a Brood Point on top of resources. Zero for races that do
    /// not use the pool.
    pub brood: u16,
}

/// Unused today, kept so `Fixed` stays imported for the modifier pipeline that
/// will scale these at Phase 7.
#[allow(dead_code)]
const _SCALE_PLACEHOLDER: Fixed = Fixed::ONE;
