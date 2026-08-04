//! §3.2 step 11 — the harvest loop. GAME_DESIGN §1, TECH_SPEC §5.4.
//!
//! # The macro clock
//! This is the file the whole game's rhythm comes out of. A worker's round
//! trip — walk to the node, wait for a slot, mine, walk back, deposit — is the
//! pulse a player builds everything else around, and its length is the single
//! most consequential number in the content directory.
//!
//! # Saturation emerges; it is not scripted
//! GAME_DESIGN §1.1 is explicit: no diminishing-returns table. Each node has a
//! fixed number of harvest slots, workers reserve them, and the third worker
//! on a two-slot node spends part of every cycle waiting. Saturation is what
//! *happens* when slots run out, not a curve someone tuned. That means the
//! shape of the economy falls out of two numbers — `harvest_slots` and
//! `trip_ticks` — rather than out of a formula, which is exactly what makes it
//! tunable by feel.
//!
//! # Slot reservation is recomputed, never bookkept
//! The obvious design gives each node a counter that workers increment and
//! decrement. That leaks: a worker that dies mid-trip, is retargeted, or has
//! its node mined out never decrements, and the node is permanently short a
//! slot in a way nothing detects until an economy mysteriously underperforms.
//!
//! Instead occupancy is rebuilt from scratch each tick by counting the workers
//! actually assigned to each node. It cannot leak, it cannot double-count, and
//! it needs no cleanup path in `death_cleanup`. Ascending entity index means
//! that when a node is oversubscribed, *which* workers hold the slots is a
//! property of the state rather than of arrival order (§6.9).

use sim_core::components::UnitState;
use sim_core::registry::{ResourceKind, Role};
use sim_core::{EntityHandle, OptionalHandle, Registries, SimState};
use trilateral_fixed::Fixed;

use crate::SimContext;

/// How close a worker must be to mine or deposit, beyond the two radii.
///
/// Generous on purpose, and for two reasons. A worker jostled out of position
/// by its neighbours should not silently stop working; and because statics
/// block whole tiles, the closest a worker can actually STAND is the centre
/// of an adjacent tile, which is up to a tile and a half from the target
/// surface. Too tight a value here means workers that path correctly, arrive
/// correctly, and then mine nothing.
const INTERACT_SLACK: Fixed = Fixed::from_ratio(3, 2);

pub fn economy(state: &mut SimState, reg: &Registries, ctx: &mut SimContext) {
    rebuild_slot_occupancy(state, reg, ctx);

    for i in 0..state.c.capacity() {
        if !state.c.alive.get(i) {
            continue;
        }
        let n = i as usize;
        match state.c.state[n] {
            UnitState::Harvesting => step_harvesting(state, reg, ctx, i),
            UnitState::ReturningCargo => step_returning(state, reg, i),
            _ => {}
        }
    }
}

/// Count the workers assigned to each node, and decide which of them hold
/// slots. Rebuilt every tick — see the module note on why this is not a
/// counter.
fn rebuild_slot_occupancy(state: &SimState, reg: &Registries, ctx: &mut SimContext) {
    let cap = state.c.capacity() as usize;
    ctx.slot_holder.clear();
    ctx.slot_holder.resize(cap, false);
    ctx.node_used.clear();
    ctx.node_used.resize(cap, 0);

    for i in 0..state.c.capacity() {
        if !state.c.alive.get(i) || state.c.state[i as usize] != UnitState::Harvesting {
            continue;
        }
        let Some(node) = state.c.harvest_target[i as usize].get() else {
            continue;
        };
        if !state.entities.is_alive(node) {
            continue;
        }
        let slots = reg
            .units
            .by_archetype(state.c.archetype[node.index as usize])
            .and_then(|u| u.resource)
            .map(|r| r.harvest_slots)
            .unwrap_or(0);
        let used = &mut ctx.node_used[node.index as usize];
        if *used < slots {
            *used += 1;
            ctx.slot_holder[i as usize] = true;
        }
        // Otherwise the worker is queued at the node. It keeps its assignment
        // and simply waits, which is what makes over-saturation cost time
        // rather than production.
    }
}

fn step_harvesting(state: &mut SimState, reg: &Registries, ctx: &SimContext, i: u32) {
    let n = i as usize;
    let Some(node) = state.c.harvest_target[n].get() else {
        state.c.state[n] = UnitState::Idle;
        return;
    };
    if !state.entities.is_alive(node) || state.c.resource_left[node.index as usize] == 0 {
        // Node gone or mined out. Drop the assignment and stand down rather
        // than mining a corpse — reassignment is the player's job (and, later,
        // the bot's).
        state.c.harvest_target[n] = OptionalHandle::NONE;
        state.c.harvest_ticks[n] = 0;
        state.c.state[n] = UnitState::Idle;
        return;
    }

    if !within_reach(state, reg, i, node) {
        // Still walking. Re-assert the destination each tick: movement clears
        // it on arrival, and the node may have been reassigned.
        state.c.dest[n] = state.c.pos[node.index as usize];
        return;
    }
    if !ctx.slot_holder[n] {
        // At the node but queued behind a full slot list. This waiting is the
        // entire mechanism behind saturation.
        return;
    }

    let Some(res) = reg
        .units
        .by_archetype(state.c.archetype[node.index as usize])
        .and_then(|u| u.resource)
    else {
        state.c.harvest_target[n] = OptionalHandle::NONE;
        state.c.state[n] = UnitState::Idle;
        return;
    };

    state.c.harvest_ticks[n] += 1;
    if state.c.harvest_ticks[n] < res.trip_ticks {
        return;
    }

    // Trip complete. Take what is there, which may be less than a full load
    // from the last scraps of a node.
    let taken = res.per_trip.min(state.c.resource_left[node.index as usize]);
    state.c.resource_left[node.index as usize] -= taken;
    state.c.cargo[n] = taken;
    state.c.harvest_ticks[n] = 0;
    state.c.state[n] = UnitState::ReturningCargo;
    state.c.path[n].clear();
    if let Some(base) = nearest_base(state, reg, i) {
        state.c.dest[n] = state.c.pos[base.index as usize];
    }
}

fn step_returning(state: &mut SimState, reg: &Registries, i: u32) {
    let n = i as usize;
    let Some(base) = nearest_base(state, reg, i) else {
        // Nowhere to deposit. Hold the cargo rather than dropping it; if a
        // base is rebuilt the worker resumes, and a player who lost every
        // base has larger problems.
        return;
    };
    if !within_reach(state, reg, i, base) {
        state.c.dest[n] = state.c.pos[base.index as usize];
        return;
    }

    // Deposit. The resource kind comes from the NODE, not the worker, so one
    // worker can be switched between Ore and Flux without carrying the wrong
    // currency home.
    let kind = state.c.harvest_target[n]
        .get()
        .filter(|h| state.entities.is_alive(*h))
        .and_then(|h| reg.units.by_archetype(state.c.archetype[h.index as usize]))
        .and_then(|u| u.resource)
        .map(|r| r.kind);

    let owner = state.c.owner[n].0 as usize;
    let amount = state.c.cargo[n];
    if let Some(p) = state.players.get_mut(owner) {
        match kind {
            Some(ResourceKind::Flux) => p.flux += amount,
            // Ore is the default: a worker whose node vanished mid-trip still
            // banks what it mined rather than losing it.
            _ => p.ore += amount,
        }
    }
    state.c.cargo[n] = 0;

    // Straight back out. This is the loop.
    match state.c.harvest_target[n].get() {
        Some(node)
            if state.entities.is_alive(node) && state.c.resource_left[node.index as usize] > 0 =>
        {
            state.c.state[n] = UnitState::Harvesting;
            state.c.dest[n] = state.c.pos[node.index as usize];
            state.c.path[n].clear();
        }
        _ => {
            state.c.harvest_target[n] = OptionalHandle::NONE;
            state.c.state[n] = UnitState::Idle;
        }
    }
}

fn within_reach(state: &SimState, reg: &Registries, a: u32, b: EntityHandle) -> bool {
    let ra = reg
        .units
        .by_archetype(state.c.archetype[a as usize])
        .map(|u| u.collider_radius)
        .unwrap_or(Fixed::ZERO);
    let rb = reg
        .units
        .by_archetype(state.c.archetype[b.index as usize])
        .map(|u| u.collider_radius)
        .unwrap_or(Fixed::ZERO);
    let reach = ra + rb + INTERACT_SLACK;
    let d2 = state.c.pos[a as usize].distance_sq_wide(state.c.pos[b.index as usize]);
    let r2 = ((reach.to_bits() as i128) * (reach.to_bits() as i128)) >> 32;
    d2 <= r2
}

/// Nearest friendly base, ties by lowest handle index (§6.9).
///
/// Recomputed rather than cached: bases are few, and a cached "my base" goes
/// stale the moment one dies — silently, and in a way that only shows up as an
/// economy that stopped working.
fn nearest_base(state: &SimState, reg: &Registries, i: u32) -> Option<EntityHandle> {
    let owner = state.c.owner[i as usize];
    let from = state.c.pos[i as usize];
    let mut best: Option<(i128, EntityHandle)> = None;
    for h in state.entities.iter_live() {
        let n = h.index as usize;
        if state.c.owner[n] != owner {
            continue;
        }
        let is_base = reg
            .units
            .by_archetype(state.c.archetype[n])
            .map(|u| u.role == Role::Base)
            .unwrap_or(false);
        if !is_base {
            continue;
        }
        let d = state.c.pos[n].distance_sq_wide(from);
        match best {
            Some((bd, _)) if bd <= d => {}
            _ => best = Some((d, h)),
        }
    }
    best.map(|(_, h)| h)
}
