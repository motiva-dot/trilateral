//! `Components` — the Structure-of-Arrays store. CLAUDE.md §3.1, ADR-005.
//!
//! Parallel arrays indexed by `EntityIndex`, every one exactly
//! `capacities.max_entities` long, allocated once and never resized.
//!
//! # Two invariants everything else depends on
//! * **Declared order is hash order.** `hash_into` folds the arrays in the
//!   order they are declared in the struct. Adding a field changes the state
//!   hash — which is correct and expected as content grows, but it invalidates
//!   stored replays, so it belongs in a phase, not in a drive-by commit.
//! * **Freed slots are zeroed.** TECH_SPEC §3 excludes freed-slot garbage from
//!   the hash "by construction". This is that construction: `clear_slot`
//!   writes a known value into every array. Without it, two clients whose
//!   entities died in a different *order* would hold different garbage in dead
//!   slots and hash differently while being gameplay-identical.
//!
//! Fields listed in §3.1 but absent here — `attack`, `cmd_queue`, `modifiers`,
//! `cargo`, `production`, `cooldowns` — arrive with the phases that give them
//! meaning (5, 3, 7, 6, 6, 5 respectively). Adding them now would mean
//! inventing their shape before the systems that read them exist.

use serde::{Deserialize, Serialize};
use trilateral_fixed::{FixedAngle, FixedVec2};

use crate::bitset::BitSet;
use crate::hash::SimHasher;
use crate::ids::{ArchetypeId, EntityIndex, OptionalHandle, PlayerId};

/// Coarse unit state. The command execution system (§3.2 step 2) drives
/// transitions; systems read it to decide whether they apply.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[repr(u8)]
pub enum UnitState {
    #[default]
    Idle = 0,
    Moving = 1,
    Attacking = 2,
    Harvesting = 3,
    ReturningCargo = 4,
    Constructing = 5,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Components {
    pub alive: BitSet,
    pub archetype: Box<[ArchetypeId]>,
    pub owner: Box<[PlayerId]>,
    pub pos: Box<[FixedVec2]>,
    pub facing: Box<[FixedAngle]>,
    pub vel: Box<[FixedVec2]>,
    pub hp: Box<[i32]>,
    pub shields: Box<[i32]>,
    pub incoming_dmg: Box<[i32]>,
    pub state: Box<[UnitState]>,
    pub target: Box<[OptionalHandle]>,
}

impl Components {
    pub fn new(capacity: u32) -> Components {
        let n = capacity as usize;
        // `vec![v; n]` writes every element, which pre-touches the pages —
        // required by ADR-005 so no first-touch fault lands mid-benchmark.
        Components {
            alive: BitSet::new(capacity),
            archetype: vec![ArchetypeId::NONE; n].into_boxed_slice(),
            owner: vec![PlayerId::NEUTRAL; n].into_boxed_slice(),
            pos: vec![FixedVec2::ZERO; n].into_boxed_slice(),
            facing: vec![FixedAngle::ZERO; n].into_boxed_slice(),
            vel: vec![FixedVec2::ZERO; n].into_boxed_slice(),
            hp: vec![0i32; n].into_boxed_slice(),
            shields: vec![0i32; n].into_boxed_slice(),
            incoming_dmg: vec![0i32; n].into_boxed_slice(),
            state: vec![UnitState::Idle; n].into_boxed_slice(),
            target: vec![OptionalHandle::NONE; n].into_boxed_slice(),
        }
    }

    #[inline]
    pub fn capacity(&self) -> u32 {
        self.alive.len()
    }

    /// Reset a slot to the same values a freshly-constructed store holds.
    ///
    /// Called on despawn. See the module note: this is what makes freed-slot
    /// contents a function of *nothing*, rather than of match history.
    pub fn clear_slot(&mut self, i: EntityIndex) {
        let n = i as usize;
        self.alive.set(i, false);
        self.archetype[n] = ArchetypeId::NONE;
        self.owner[n] = PlayerId::NEUTRAL;
        self.pos[n] = FixedVec2::ZERO;
        self.facing[n] = FixedAngle::ZERO;
        self.vel[n] = FixedVec2::ZERO;
        self.hp[n] = 0;
        self.shields[n] = 0;
        self.incoming_dmg[n] = 0;
        self.state[n] = UnitState::Idle;
        self.target[n] = OptionalHandle::NONE;
    }

    /// Fold every array, in declared order.
    pub fn hash_into(&self, h: &mut SimHasher) {
        self.alive.hash_into(h);
        for v in &self.archetype {
            h.write_u32(v.0 as u32);
        }
        for v in &self.owner {
            h.write_u8(v.0);
        }
        hash_vec2s(h, &self.pos);
        for v in &self.facing {
            h.write_u32(v.to_bits());
        }
        hash_vec2s(h, &self.vel);
        h.write_i32_slice(&self.hp);
        h.write_i32_slice(&self.shields);
        h.write_i32_slice(&self.incoming_dmg);
        for v in &self.state {
            h.write_u8(*v as u8);
        }
        for v in &self.target {
            let (i, g) = v.raw();
            h.write_u32(i);
            h.write_u32(g);
        }
    }
}

#[inline]
fn hash_vec2s(h: &mut SimHasher, vs: &[FixedVec2]) {
    for v in vs {
        h.write_i64(v.x.to_bits());
        h.write_i64(v.y.to_bits());
    }
}

/// Debug helper: is this slot at its cleared value in every array?
///
/// Used by the zero-on-free tests, and by `desync_trace` later.
pub fn slot_is_clear(c: &Components, i: EntityIndex) -> bool {
    let n = i as usize;
    !c.alive.get(i)
        && c.archetype[n] == ArchetypeId::NONE
        && c.owner[n] == PlayerId::NEUTRAL
        && c.pos[n] == FixedVec2::ZERO
        && c.facing[n] == FixedAngle::ZERO
        && c.vel[n] == FixedVec2::ZERO
        && c.hp[n] == 0
        && c.shields[n] == 0
        && c.incoming_dmg[n] == 0
        && c.state[n] == UnitState::Idle
        && c.target[n] == OptionalHandle::NONE
}

#[cfg(test)]
mod tests {
    use super::*;
    use trilateral_fixed::Fixed;

    fn hash(c: &Components) -> u64 {
        let mut h = SimHasher::new();
        c.hash_into(&mut h);
        h.finish()
    }

    #[test]
    fn a_new_store_is_entirely_clear() {
        let c = Components::new(64);
        assert_eq!(c.capacity(), 64);
        for i in 0..64 {
            assert!(slot_is_clear(&c, i), "slot {i} was not clear");
        }
    }

    #[test]
    fn clear_slot_restores_every_array() {
        let mut c = Components::new(8);
        c.alive.set(3, true);
        c.archetype[3] = ArchetypeId(7);
        c.owner[3] = PlayerId(1);
        c.pos[3] = FixedVec2::from_ints(5, -9);
        c.facing[3] = FixedAngle::QUARTER_TURN;
        c.vel[3] = FixedVec2::from_ints(1, 1);
        c.hp[3] = 42;
        c.shields[3] = 7;
        c.incoming_dmg[3] = 3;
        c.state[3] = UnitState::Attacking;
        assert!(!slot_is_clear(&c, 3));
        c.clear_slot(3);
        assert!(slot_is_clear(&c, 3), "clear_slot missed an array");
    }

    #[test]
    fn zero_on_free_makes_the_hash_independent_of_death_order() {
        // THE reason clear_slot exists. Two stores that reach the same live
        // configuration by different histories must hash identically.
        let mut a = Components::new(16);
        let mut b = Components::new(16);

        for (i, c) in [(0u32, &mut a), (0, &mut b)] {
            let _ = i;
            for slot in 0..4u32 {
                c.alive.set(slot, true);
                c.hp[slot as usize] = 100 + slot as i32;
                c.pos[slot as usize] = FixedVec2::from_ints(slot as i32, 0);
            }
        }
        // Kill 1 then 2 in a; kill 2 then 1 in b. Same survivors either way.
        a.clear_slot(1);
        a.clear_slot(2);
        b.clear_slot(2);
        b.clear_slot(1);
        assert_eq!(hash(&a), hash(&b), "death order leaked into the hash");
    }

    #[test]
    fn the_hash_notices_a_single_component_change() {
        let mut c = Components::new(32);
        let before = hash(&c);
        c.hp[17] = 1;
        assert_ne!(hash(&c), before, "one hp point vanished from the hash");

        let mut d = Components::new(32);
        d.pos[17] = FixedVec2::new(Fixed::from_bits(1), Fixed::ZERO);
        assert_ne!(hash(&d), before, "one position ULP vanished from the hash");
    }

    #[test]
    fn the_hash_notices_which_slot_changed() {
        // Same values, different slots. If these collided, an entity-ordering
        // desync would be invisible at the checkpoint.
        let mut a = Components::new(32);
        let mut b = Components::new(32);
        a.hp[3] = 50;
        b.hp[4] = 50;
        assert_ne!(hash(&a), hash(&b));
    }

    #[test]
    fn hashing_is_reproducible() {
        let mut c = Components::new(32);
        c.alive.set(5, true);
        c.hp[5] = 33;
        assert_eq!(hash(&c), hash(&c));
    }

    #[test]
    fn capacity_participates_in_the_hash() {
        assert_ne!(hash(&Components::new(32)), hash(&Components::new(64)));
    }

    #[test]
    fn unit_state_discriminants_are_pinned() {
        // These land in the state hash as raw bytes, so reordering the enum is
        // a determinism break. Pinned so it cannot happen by accident.
        assert_eq!(UnitState::Idle as u8, 0);
        assert_eq!(UnitState::Moving as u8, 1);
        assert_eq!(UnitState::Attacking as u8, 2);
        assert_eq!(UnitState::Harvesting as u8, 3);
        assert_eq!(UnitState::ReturningCargo as u8, 4);
        assert_eq!(UnitState::Constructing as u8, 5);
    }
}
