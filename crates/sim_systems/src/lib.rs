//! sim_systems — the tick pipeline. CLAUDE.md §3.2.
//!
//! # The pipeline is hand-written and its order is law
//! §6.7: never reorder the pipeline, never auto-schedule. [`tick`] is a
//! literal sequence of calls in the order §3.2 lists, and that order is part
//! of the simulation's definition — moving one call changes outcomes and
//! invalidates every stored replay. There is no scheduler to consult, no
//! dependency graph to infer, and nothing that could reorder itself under
//! optimisation.
//!
//! Systems not yet written are present as comments at their exact position
//! rather than absent. The shape of the tick is therefore visible in full from
//! day one: a reader can see what is missing and precisely where it goes, and
//! adding a system later is filling in a slot rather than deciding where it
//! belongs.
//!
//! # Determinism obligations every system inherits
//! * Iterate in ascending entity index — never in "whatever order the query
//!   returned", unless that order is itself index-sorted (it is; see
//!   [`spatial::SpatialHash`]).
//! * Resolve ties by handle index (§6.9).
//! * No floats (§1.1), no wall clock, no hash containers (§1.2).

#![forbid(unsafe_code)]

pub mod economy;
pub mod movement;
pub mod occupancy;
pub mod path;
pub mod pathing;
pub mod spatial;
pub mod steering;

use sim_core::{EntityIndex, Registries, SimState};
use trilateral_fixed::{Fixed, FixedVec2};

use crate::spatial::SpatialHash;

/// Everything a tick needs that is not `SimState`.
///
/// Scratch buffers live here so they are allocated once at match start (§1.4)
/// rather than per tick. `SpatialHash` lives here rather than in `SimState`
/// because it is a *derived* index: it can be rebuilt from positions at any
/// time, so snapshotting it would store the same information twice and invite
/// the two copies to disagree.
pub struct SimContext {
    pub spatial: SpatialHash,
    /// Reused by systems that query neighbours.
    pub query_scratch: Vec<EntityIndex>,
    /// Per-entity displacement owed to neighbours this tick. Written by
    /// `steering`, consumed by `movement` — which is the single place
    /// positions are ever written.
    pub separation: Vec<FixedVec2>,
    /// Scratch for one relaxation pass, so each pass stays a pure function
    /// of the state it started from.
    pub sep_delta: Vec<FixedVec2>,
    /// Set when a unit is pressed against someone walking the same way —
    /// queuing rather than jammed. Slows the settle counter.
    pub queued: Vec<bool>,
    /// Per-tick neighbour candidate lists in CSR form, so the broad phase is
    /// paid once per tick rather than once per relaxation pass.
    pub neighbour_start: Vec<u32>,
    pub neighbour_data: Vec<u32>,
    /// Slots taken at each node this tick, rebuilt from scratch.
    pub node_used: Vec<u8>,
    /// Whether each worker currently holds a slot rather than queuing.
    pub slot_holder: Vec<bool>,
    /// A* buffers, sized to the map once (§1.4).
    pub path_scratch: crate::path::PathScratch,
    /// Reused by the pathfinder for one search.s output.
    pub path_out: Vec<sim_core::Tile>,
    pub path_indices: Vec<u32>,
}

impl SimContext {
    pub fn new(world_tiles: i32, reg: &Registries, capacity: u32, tile_count: u32) -> SimContext {
        // TECH_SPEC §4: cell = 2x the largest collider in the REGISTRY, not
        // among spawned units — a cell size that changed with the contents of
        // the map would change query results and therefore gameplay.
        let radius = reg.units.max_collider_radius();
        let cell = if radius > Fixed::ZERO {
            radius * Fixed::TWO
        } else {
            Fixed::ONE
        };
        SimContext {
            spatial: SpatialHash::new(world_tiles, cell),
            query_scratch: Vec::with_capacity(256),
            // Sized once at match start (§1.4); never grows afterwards.
            separation: vec![FixedVec2::ZERO; capacity as usize],
            sep_delta: vec![FixedVec2::ZERO; capacity as usize],
            queued: vec![false; capacity as usize],
            neighbour_start: Vec::with_capacity(capacity as usize + 1),
            neighbour_data: Vec::with_capacity(capacity as usize * 8),
            node_used: vec![0u8; capacity as usize],
            slot_holder: vec![false; capacity as usize],
            path_scratch: crate::path::PathScratch::new(tile_count),
            path_out: Vec::with_capacity(256),
            path_indices: Vec::with_capacity(64),
        }
    }
}

/// Advance the simulation by exactly one tick.
///
/// The nineteen steps of §3.2, in order. Steps whose systems do not exist yet
/// are named in place with the phase that fills them in.
pub fn tick(state: &mut SimState, reg: &Registries, ctx: &mut SimContext) {
    //  1. command_ingest      — commands enter at the boundary today
    //                           (SimState::ingest); per-entity CmdQueue later.
    //  2. command_execution   — front-of-queue Command -> UnitState.
    movement::command_execution(state, reg);
    //  3. ai_tick             — Phase 10
    //  4. target_acquisition  — Phase 5
    //  5. combat_fsm          — Phase 5
    //  6. projectile_advance  — Phase 5
    //  7. damage_application  — Phase 5
    //  8. death_cleanup       — Phase 5
    //  9. modifier_pipeline   — Phase 7
    // 10. production_tick     — Phase 6
    // 11. economy_tick — the harvest loop.
    economy::economy(state, reg, ctx);
    // 12. pathfinding — hand routes to movers that need one.
    pathing::pathfinding(state, reg, ctx);
    // 13. steering — local separation into ctx.separation.
    steering::steering(state, reg, ctx);
    // 14. movement            — integrate, clamp, reindex.
    movement::movement(state, reg, ctx);
    ctx.spatial.rebuild(state);
    // 15. fog_update          — Phase 6
    // 16. victory_check       — Phase 6
    // 17. state_hash          — recording side arrives with replays (Phase 11);
    //                           SimState::hash() is callable now.
    // 18. event_flush         — Phase 9 (SimEventBuffer)
    // 19. command_log_append  — done at ingest today.

    state.clock.advance();
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{ArchetypeId, Capacities, PlayerId, Spawn};
    use trilateral_fixed::FixedVec2;

    pub(crate) fn caps() -> Capacities {
        Capacities {
            max_entities: 64,
            max_projectiles: 8,
            max_commands_per_tick: 16,
            max_players: 4,
            cmd_queue_slots: 16,
            modifier_slots: 8,
            command_log_reserve: 64,
        }
    }

    #[test]
    fn a_tick_advances_the_clock_by_exactly_one() {
        let reg = Registries::default();
        let mut s = SimState::new(caps(), 1, 64);
        let mut ctx = SimContext::new(128, &reg, caps().max_entities, 64 * 64);
        for expected in 1..=10u64 {
            tick(&mut s, &reg, &mut ctx);
            assert_eq!(s.clock.tick.0, expected);
        }
    }

    #[test]
    fn two_identical_runs_stay_hash_equal() {
        let reg = Registries::default();
        let mut a = SimState::new(caps(), 7, 64);
        let mut b = SimState::new(caps(), 7, 64);
        let mut ca = SimContext::new(128, &reg, caps().max_entities, 64 * 64);
        let mut cb = SimContext::new(128, &reg, caps().max_entities, 64 * 64);
        for _ in 0..100 {
            tick(&mut a, &reg, &mut ca);
            tick(&mut b, &reg, &mut cb);
            assert_eq!(a.hash(), b.hash());
        }
    }

    #[test]
    fn the_spatial_index_tracks_entities_across_ticks() {
        let reg = Registries::default();
        let mut s = SimState::new(caps(), 1, 64);
        let mut ctx = SimContext::new(128, &reg, caps().max_entities, 64 * 64);
        s.spawn(Spawn {
            archetype: ArchetypeId(0),
            owner: PlayerId(0),
            pos: FixedVec2::from_ints(3, 3),
            hp: 10,
            resource: 0,
        })
        .unwrap();
        tick(&mut s, &reg, &mut ctx);
        let mut out = Vec::new();
        ctx.spatial
            .query_square(FixedVec2::from_ints(3, 3), Fixed::ONE, &mut out);
        assert_eq!(out, vec![0]);
    }
}
