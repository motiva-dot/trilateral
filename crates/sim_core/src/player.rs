//! `PlayerState` — stockpiles and supply. GAME_DESIGN §1, CLAUDE.md §4.
//!
//! # Two resources, and why the split is the whole game
//! **Ore** is abundant and worker-scaling: it measures your macro mechanics.
//! **Flux** is scarce and gates tier-2/3 tech: it forces expansion timing and
//! commitment. GAME_DESIGN §1.2 calls the tension "the eternal question every
//! thirty seconds" — more Ore units now, or Flux tech later.
//!
//! # Supply is stored doubled
//! Audit item 29: `supply: 0.5` cannot exist in an integer simulation, so a
//! Mite costs `supply_x2 = 1` and displays as 0.5. Every number here is `_x2`
//! and every display divides by two. The one place this must never be
//! forgotten is a comparison against the cap.

use serde::{Deserialize, Serialize};

use crate::hash::SimHasher;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct PlayerState {
    pub ore: u32,
    pub flux: u32,
    /// Supply consumed by living units, doubled.
    pub supply_used_x2: u16,
    /// Supply provided by bases and supply units, doubled.
    pub supply_cap_x2: u16,
    /// Murmur's Brood Points (GAME_DESIGN §2.1). Pulled forward from Phase 8
    /// by the v3 build order, because judging macro feel against generic
    /// parallel production means judging a generic RTS.
    pub brood_points: u16,
    /// Ticks accumulated toward the next Brood Point.
    pub brood_progress: u32,
    /// Whether this slot is in the match at all.
    pub active: bool,
}

impl PlayerState {
    /// Displayed supply, halved. UI only — never compare this against a cost.
    #[inline]
    pub fn supply_used(&self) -> u16 {
        self.supply_used_x2 / 2
    }

    #[inline]
    pub fn supply_cap(&self) -> u16 {
        self.supply_cap_x2 / 2
    }

    /// Can this player afford to field a unit of the given supply cost?
    ///
    /// Compared in doubled units, deliberately. Halving first would let a
    /// 0.5-supply unit slip past a full cap through integer truncation, which
    /// is precisely the bug audit item 29 exists to prevent.
    #[inline]
    pub fn has_supply_for(&self, cost_x2: u16) -> bool {
        self.supply_used_x2 + cost_x2 <= self.supply_cap_x2
    }

    #[inline]
    pub fn can_afford(&self, ore: u32, flux: u32) -> bool {
        self.ore >= ore && self.flux >= flux
    }

    /// Deduct a cost. Returns false and changes nothing if unaffordable —
    /// a partial spend would be worse than a refused one.
    pub fn try_spend(&mut self, ore: u32, flux: u32) -> bool {
        if !self.can_afford(ore, flux) {
            return false;
        }
        self.ore -= ore;
        self.flux -= flux;
        true
    }

    pub fn hash_into(&self, h: &mut SimHasher) {
        h.write_u32(self.ore);
        h.write_u32(self.flux);
        h.write_u32(self.supply_used_x2 as u32);
        h.write_u32(self.supply_cap_x2 as u32);
        h.write_u32(self.brood_points as u32);
        h.write_u32(self.brood_progress);
        h.write_bool(self.active);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spending_more_than_you_have_changes_nothing() {
        let mut p = PlayerState {
            ore: 50,
            flux: 10,
            ..Default::default()
        };
        assert!(!p.try_spend(100, 0));
        assert_eq!((p.ore, p.flux), (50, 10), "a refused spend must be atomic");
        assert!(!p.try_spend(50, 25), "flux shortfall must also refuse");
        assert_eq!((p.ore, p.flux), (50, 10));
        assert!(p.try_spend(50, 10));
        assert_eq!((p.ore, p.flux), (0, 0));
    }

    #[test]
    fn half_supply_units_cannot_slip_past_a_full_cap() {
        // Audit item 29, stated as the bug it prevents. Cap 10 (5 displayed),
        // 10 used. A 0.5-supply Mite costs 1 and must NOT fit.
        let p = PlayerState {
            supply_used_x2: 10,
            supply_cap_x2: 10,
            ..Default::default()
        };
        assert!(
            !p.has_supply_for(1),
            "a half-supply unit slipped past a full cap"
        );
        assert!(!p.has_supply_for(2));
    }

    #[test]
    fn a_half_slot_of_room_fits_exactly_one_half_supply_unit() {
        let p = PlayerState {
            supply_used_x2: 9,
            supply_cap_x2: 10,
            ..Default::default()
        };
        assert!(p.has_supply_for(1), "0.5 free should fit a 0.5 unit");
        assert!(!p.has_supply_for(2), "0.5 free must not fit a 1.0 unit");
    }

    #[test]
    fn displayed_supply_halves_and_truncates() {
        let p = PlayerState {
            supply_used_x2: 9,
            supply_cap_x2: 20,
            ..Default::default()
        };
        assert_eq!(p.supply_used(), 4, "4.5 displays as 4");
        assert_eq!(p.supply_cap(), 10);
    }

    #[test]
    fn the_hash_notices_every_field() {
        let base = PlayerState::default();
        let hash = |p: &PlayerState| {
            let mut h = SimHasher::new();
            p.hash_into(&mut h);
            h.finish()
        };
        let h0 = hash(&base);
        for mutate in [
            |p: &mut PlayerState| p.ore = 1,
            |p: &mut PlayerState| p.flux = 1,
            |p: &mut PlayerState| p.supply_used_x2 = 1,
            |p: &mut PlayerState| p.supply_cap_x2 = 1,
            |p: &mut PlayerState| p.brood_points = 1,
            |p: &mut PlayerState| p.brood_progress = 1,
            |p: &mut PlayerState| p.active = true,
        ] {
            let mut p = base;
            mutate(&mut p);
            assert_ne!(hash(&p), h0, "a field is missing from the hash");
        }
    }
}
