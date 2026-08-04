//! `SimState` — everything the simulation is. CLAUDE.md §3.1.
//!
//! # The two operations this whole architecture exists to make cheap
//! * `snapshot()` — a flat copy, used by replay keyframes (every 600 ticks),
//!   reconnect catch-up, and the desync trace tool.
//! * `hash()` — a fold over contiguous buffers in a fixed order, recorded
//!   every 10 ticks (§3.2 step 17) and exchanged between peers every 60 ticks.
//!
//! Audit items 5 and 6 chose a hand-rolled SoA store over an ECS for exactly
//! these two. In an archetype ECS both depend on component insertion history;
//! here neither depends on anything but the values.
//!
//! Fields listed in §3.1 but absent — `players`, `grid`, `fog`, `spatial`,
//! `flow_cache`, `events`, `cmd_log` — arrive with the phases that give them
//! meaning. Each will need a line in `hash_into`, and adding one changes the
//! state hash, which is a replay-invalidating change.

use serde::{Deserialize, Serialize};
use trilateral_fixed::FixedVec2;

use crate::capacities::Capacities;
use crate::clock::SimClock;
use crate::command::{CommandLog, IssuedCommand, Reject};
use crate::components::Components;
use crate::entity::EntityAllocator;
use crate::hash::SimHasher;
use crate::ids::{ArchetypeId, EntityHandle, PlayerId};
use crate::rng::SimRng;

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct SimState {
    pub capacities: Capacities,
    pub clock: SimClock,
    pub rng: SimRng,
    pub entities: EntityAllocator,
    pub c: Components,
    pub cmd_log: CommandLog,
}

/// What a spawn needs that the caller must supply. Stats such as max HP come
/// from the registries (§1.8), not from here.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Spawn {
    pub archetype: ArchetypeId,
    pub owner: PlayerId,
    pub pos: FixedVec2,
    pub hp: i32,
}

impl SimState {
    /// Allocate a whole simulation. The only allocation point — everything
    /// after this is reads and writes into buffers that already exist (§1.4).
    ///
    /// # Panics
    /// If `capacities` is invalid. Callers load them from `engine.yaml` and
    /// should call `Capacities::validate()` first to report the problem with
    /// a message naming the field.
    pub fn new(capacities: Capacities, seed: u64) -> SimState {
        capacities
            .validate()
            .expect("SimState::new given invalid capacities; validate() first");
        SimState {
            capacities,
            clock: SimClock::new(),
            rng: SimRng::from_seed(seed),
            entities: EntityAllocator::with_capacity(capacities.max_entities),
            c: Components::new(capacities.max_entities),
            cmd_log: CommandLog::with_reserve(capacities.command_log_reserve),
        }
    }

    /// Validate and record a command. §4: invalid commands are dropped and
    /// counted, never obeyed and never a panic.
    ///
    /// Returns the rejection reason if it was dropped, so callers can surface
    /// it (a UI bark locally, telemetry for a networked peer).
    pub fn ingest(&mut self, cmd: IssuedCommand) -> Result<(), Reject> {
        match crate::command::validate(self, &cmd) {
            Ok(()) => {
                self.cmd_log.push(cmd);
                Ok(())
            }
            Err(r) => {
                self.cmd_log.record_rejection();
                Err(r)
            }
        }
    }

    /// Take a slot and populate it.
    ///
    /// `None` when full — see `EntityAllocator::alloc`. A dropped spawn is a
    /// capacity problem to surface, not a crash mid-match.
    pub fn spawn(&mut self, s: Spawn) -> Option<EntityHandle> {
        let h = self.entities.alloc()?;
        let i = h.index as usize;
        self.c.alive.set(h.index, true);
        self.c.archetype[i] = s.archetype;
        self.c.owner[i] = s.owner;
        self.c.pos[i] = s.pos;
        self.c.hp[i] = s.hp;
        Some(h)
    }

    /// Free a slot and zero it. Returns false for a stale handle.
    pub fn despawn(&mut self, h: EntityHandle) -> bool {
        if !self.entities.free(h) {
            return false;
        }
        self.c.clear_slot(h.index);
        true
    }

    #[inline]
    pub fn is_alive(&self, h: EntityHandle) -> bool {
        self.entities.is_alive(h)
    }

    #[inline]
    pub fn live_count(&self) -> u32 {
        self.entities.live_count()
    }

    /// A complete, independent copy.
    ///
    /// Boxed because `SimState` owns several megabytes of arrays at prototype
    /// scale and callers keep these around (replay keyframes every 600 ticks).
    pub fn snapshot(&self) -> Box<SimState> {
        Box::new(self.clone())
    }

    /// Overwrite with a snapshot. Byte-for-byte restoration, not a merge.
    pub fn restore(&mut self, snap: &SimState) {
        self.clone_from(snap);
    }

    /// The number CLAUDE.md §1.5 is about.
    ///
    /// Field order here IS the hash definition. Reordering these calls changes
    /// every hash in the project and invalidates every stored replay.
    pub fn hash(&self) -> u64 {
        let mut h = SimHasher::new();
        self.capacities.hash_into(&mut h);
        self.clock.hash_into(&mut h);
        for w in self.rng.state() {
            h.write_u64(w);
        }
        self.entities.hash_into(&mut h);
        self.c.hash_into(&mut h);
        self.cmd_log.hash_into(&mut h);
        h.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{UnitState, slot_is_clear};

    fn caps(max_entities: u32) -> Capacities {
        Capacities {
            max_entities,
            max_projectiles: 64,
            max_commands_per_tick: 256,
            max_players: 8,
            cmd_queue_slots: 16,
            modifier_slots: 8,
            command_log_reserve: 1024,
        }
    }

    fn spawn_at(x: i32, y: i32, arch: u16, owner: u8) -> Spawn {
        Spawn {
            archetype: ArchetypeId(arch),
            owner: PlayerId(owner),
            pos: FixedVec2::from_ints(x, y),
            hp: 100,
        }
    }

    #[test]
    fn a_new_state_is_empty_and_at_tick_zero() {
        let s = SimState::new(caps(64), 42);
        assert_eq!(s.live_count(), 0);
        assert_eq!(s.clock.tick.0, 0);
    }

    #[test]
    fn spawn_populates_the_slot_and_despawn_clears_it() {
        let mut s = SimState::new(caps(16), 1);
        let h = s.spawn(spawn_at(3, 4, 7, 1)).unwrap();
        assert!(s.is_alive(h));
        assert_eq!(s.c.pos[h.index as usize], FixedVec2::from_ints(3, 4));
        assert_eq!(s.c.hp[h.index as usize], 100);
        assert_eq!(s.c.archetype[h.index as usize], ArchetypeId(7));
        assert_eq!(s.c.state[h.index as usize], UnitState::Idle);

        assert!(s.despawn(h));
        assert!(!s.is_alive(h));
        assert!(slot_is_clear(&s.c, h.index));
        assert!(!s.despawn(h), "double despawn should report failure");
    }

    // ---- THE PHASE 2 GATE -------------------------------------------------

    /// 500 mixed archetypes, per IMPLEMENTATION_PLAN Phase 2.
    fn populated_500(seed: u64) -> SimState {
        let mut s = SimState::new(caps(2048), seed);
        for i in 0..500i32 {
            let sp = Spawn {
                archetype: ArchetypeId((i % 17) as u16),
                owner: PlayerId((i % 3) as u8),
                pos: FixedVec2::from_ints(i % 64, i / 64),
                hp: 50 + (i % 7) * 10,
            };
            s.spawn(sp).expect("capacity is 2048");
        }
        s
    }

    #[test]
    fn gate_spawn_five_hundred_mixed_archetypes() {
        let s = populated_500(42);
        assert_eq!(s.live_count(), 500);
        assert_eq!(s.c.alive.count_ones(), 500);
    }

    #[test]
    fn gate_snapshot_restore_is_byte_equal() {
        let mut s = populated_500(42);
        let snap = s.snapshot();
        let hash_before = s.hash();

        // Churn: move things, kill some, spawn more.
        for i in 0..200u32 {
            s.c.pos[i as usize] = FixedVec2::from_ints(-1, -1);
        }
        let victims: Vec<_> = s.entities.iter_live().take(50).collect();
        for v in victims {
            s.despawn(v);
        }
        for i in 0..30 {
            s.spawn(spawn_at(i, i, 1, 0));
        }
        s.clock.advance();
        assert_ne!(s.hash(), hash_before, "churn should have moved the hash");

        s.restore(&snap);
        assert_eq!(s, *snap, "restore was not byte-equal");
        assert_eq!(s.hash(), hash_before, "restore did not recover the hash");
    }

    #[test]
    fn gate_hash_is_stable_across_identical_runs() {
        // Same seed, same operations, from two independent constructions.
        assert_eq!(populated_500(42).hash(), populated_500(42).hash());
    }

    #[test]
    fn a_different_seed_gives_a_different_hash() {
        assert_ne!(populated_500(42).hash(), populated_500(43).hash());
    }

    #[test]
    fn the_hash_moves_when_anything_moves() {
        let mut s = populated_500(42);
        let before = s.hash();

        s.c.hp[123] += 1;
        let after_hp = s.hash();
        assert_ne!(after_hp, before, "one hp point");

        s.c.hp[123] -= 1;
        assert_eq!(s.hash(), before, "undo should restore the hash exactly");

        s.clock.advance();
        assert_ne!(s.hash(), before, "the tick number is part of the state");
    }

    #[test]
    fn death_order_is_part_of_the_state_and_the_hash_says_so() {
        // Subtle, and worth stating precisely because the obvious intuition is
        // wrong.
        //
        // Zero-on-free means the COMPONENT ARRAYS do not leak death order —
        // that is proven in `components::tests`. But the ALLOCATOR does, and
        // deliberately: the free list is LIFO, so killing A-then-B versus
        // B-then-A leaves different slots at the head, and the very next spawn
        // lands somewhere different. Since §6.9 makes handle index a
        // tie-breaker, "which slot" is gameplay, not bookkeeping.
        //
        // So two clients that somehow processed the same deaths in a different
        // order have genuinely diverged, and the hash must say so loudly at the
        // next checkpoint rather than wait for the divergence to become
        // visible in combat. In a correct lockstep run this cannot happen —
        // every client executes one command log in one order.
        let mut a = populated_500(42);
        let mut b = populated_500(42);
        let targets: Vec<_> = a.entities.iter_live().skip(10).take(5).collect();

        for t in targets.iter() {
            a.despawn(*t);
        }
        for t in targets.iter().rev() {
            b.despawn(*t);
        }
        assert_ne!(
            a.hash(),
            b.hash(),
            "differing free-list state must be visible to the hash"
        );

        // The same deaths in the same order agree exactly, which is the
        // property lockstep actually relies on.
        let mut c = populated_500(42);
        for t in targets.iter() {
            c.despawn(*t);
        }
        assert_eq!(a.hash(), c.hash());
    }

    #[test]
    fn component_arrays_alone_are_death_order_independent() {
        // The narrower property zero-on-free does buy, isolated from the
        // allocator so the distinction above is unmistakable.
        let mut a = populated_500(42);
        let mut b = populated_500(42);
        let targets: Vec<_> = a.entities.iter_live().skip(10).take(5).collect();
        for t in targets.iter() {
            a.despawn(*t);
        }
        for t in targets.iter().rev() {
            b.despawn(*t);
        }
        let mut ha = SimHasher::new();
        let mut hb = SimHasher::new();
        a.c.hash_into(&mut ha);
        b.c.hash_into(&mut hb);
        assert_eq!(ha.finish(), hb.finish(), "components leaked death order");
    }

    #[test]
    fn running_out_of_capacity_returns_none_rather_than_panicking() {
        let mut s = SimState::new(caps(4), 1);
        for _ in 0..4 {
            assert!(s.spawn(spawn_at(0, 0, 0, 0)).is_some());
        }
        assert!(s.spawn(spawn_at(0, 0, 0, 0)).is_none());
        assert_eq!(s.live_count(), 4);
    }

    #[test]
    #[should_panic(expected = "invalid capacities")]
    fn invalid_capacities_are_refused_loudly() {
        SimState::new(caps(0), 1);
    }

    #[test]
    fn snapshots_are_independent_of_the_state_they_came_from() {
        let mut s = SimState::new(caps(16), 1);
        let h = s.spawn(spawn_at(1, 1, 0, 0)).unwrap();
        let snap = s.snapshot();
        s.c.hp[h.index as usize] = 999;
        assert_eq!(
            snap.c.hp[h.index as usize], 100,
            "snapshot aliased the live state"
        );
    }
}
