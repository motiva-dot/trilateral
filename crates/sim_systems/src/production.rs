//! §3.2 step 10 — supply, the Brood Pool, and production queues.
//!
//! # Supply and Brood are RECOMPUTED, not bookkept
//! Same reasoning as harvest slots, and it matters more here. A supply counter
//! that units increment on spawn and decrement on death is wrong the first
//! time anything dies outside `death_cleanup` — a despawn during a rollback,
//! a unit removed by a future ability, a slot recycled by the allocator. The
//! symptom is a player permanently supply-blocked at 190/200 with no visible
//! cause, which is the kind of bug that survives to ship.
//!
//! Recomputing from live entities each tick cannot drift. It costs one pass
//! over the component arrays, which the bench says is free next to steering.
//!
//! # The Brood Pool is Murmur's macro mechanic (GAME_DESIGN §2.1)
//! Every base feeds one shared pool; any unit is bought from it at any base.
//! One production currency instead of parallel buildings, so Murmur macro
//! skill is *never floating points* and *choosing what the swarm becomes at
//! the moment of spending*. The cap scales per base, which is why losing a
//! base costs production capacity itself rather than just buildings — §2.1's
//! "Murmur death-spirals hard when behind".

use sim_core::registry::Role;
use sim_core::{EntityHandle, PlayerId, Registries, SimState, Spawn, TrainCost, UnitState};
use trilateral_fixed::{Fixed, FixedVec2};

/// Resolve what putting `archetype` into production costs.
///
/// Returns `None` for archetypes that cannot be trained at all — bases,
/// resource nodes, and anything the registry does not know.
pub fn train_cost(reg: &Registries, archetype: sim_core::ArchetypeId) -> Option<TrainCost> {
    let u = reg.units.by_archetype(archetype)?;
    if matches!(u.role, Role::Base | Role::Resource) {
        return None;
    }
    Some(TrainCost {
        ore: u.cost.ore,
        flux: u.cost.flux,
        ticks: u.cost.ticks,
        supply_x2: u.supply_x2,
        // Murmur pays a Brood Point for every unit. Other races will not.
        brood: if u.faction == "murmur" { 1 } else { 0 },
    })
}

pub fn production(state: &mut SimState, reg: &Registries) {
    recompute_supply_and_brood_cap(state, reg);
    accrue_brood(state, reg);
    advance_queues(state, reg);
}

/// Rebuild supply used, supply cap, and the Brood cap from live entities.
fn recompute_supply_and_brood_cap(state: &mut SimState, reg: &Registries) {
    let players = state.players.len();
    let mut used = vec![0u32; players];
    let mut cap = vec![0u32; players];
    let mut bases = vec![0u32; players];

    for i in 0..state.c.capacity() {
        if !state.c.alive.get(i) {
            continue;
        }
        let n = i as usize;
        let owner = state.c.owner[n];
        if owner == PlayerId::NEUTRAL {
            continue;
        }
        let p = owner.0 as usize;
        if p >= players {
            continue;
        }
        let Some(u) = reg.units.by_archetype(state.c.archetype[n]) else {
            continue;
        };
        used[p] += u.supply_x2 as u32;
        cap[p] += u.provides_supply_x2 as u32;
        if u.role == Role::Base {
            bases[p] += 1;
        }
    }

    let brood_cap_per_base = reg.race.brood.stockpile_cap_per_base as u32;
    for (p, player) in state.players.iter_mut().enumerate() {
        // GAME_DESIGN §1.3: cap 200 displayed, so 400 doubled.
        player.supply_used_x2 = used[p].min(u16::MAX as u32) as u16;
        player.supply_cap_x2 = cap[p].min(400) as u16;
        let brood_cap = (bases[p] * brood_cap_per_base).min(u16::MAX as u32) as u16;
        if player.brood_points > brood_cap {
            // Losing a base can put you over the cap. Trim rather than keep
            // the surplus: the cap is meant to punish floating, and a player
            // who kept banked points through a base loss would be rewarded
            // for exactly the thing §2.1 wants to punish.
            player.brood_points = brood_cap;
        }
    }
}

/// One Brood Point per `period_ticks` per base, into the shared pool.
fn accrue_brood(state: &mut SimState, reg: &Registries) {
    let period = reg.race.brood.period_ticks.max(1);
    let per_base = reg.race.brood.stockpile_cap_per_base as u32;

    let mut bases = vec![0u32; state.players.len()];
    for i in 0..state.c.capacity() {
        if !state.c.alive.get(i) {
            continue;
        }
        let n = i as usize;
        let owner = state.c.owner[n];
        if owner == PlayerId::NEUTRAL || owner.0 as usize >= bases.len() {
            continue;
        }
        if reg
            .units
            .by_archetype(state.c.archetype[n])
            .map(|u| u.role == Role::Base)
            .unwrap_or(false)
        {
            bases[owner.0 as usize] += 1;
        }
    }

    for (p, player) in state.players.iter_mut().enumerate() {
        if bases[p] == 0 {
            // No bases, no generation — and the progress resets, so rebuilding
            // does not hand back a point that was nearly earned before the
            // base died.
            player.brood_progress = 0;
            continue;
        }
        let cap = (bases[p] * per_base).min(u16::MAX as u32) as u16;
        // Each base contributes one tick of progress per tick.
        player.brood_progress += bases[p];
        while player.brood_progress >= period {
            player.brood_progress -= period;
            if player.brood_points < cap {
                player.brood_points += 1;
            }
            // At the cap the progress is simply lost. That IS the pressure:
            // a floating pool is production you never bought.
        }
    }
}

fn advance_queues(state: &mut SimState, reg: &Registries) {
    // Collect completions first: spawning mutates the allocator, and doing it
    // inside the scan would let a newly spawned unit be visited this tick.
    let mut completed: Vec<(EntityHandle, sim_core::ArchetypeId)> = Vec::new();
    for h in state.entities.iter_live().collect::<Vec<_>>() {
        let n = h.index as usize;
        if state.c.production[n].is_empty() {
            continue;
        }
        if let Some(done) = state.c.production[n].tick_front() {
            completed.push((h, done));
        }
    }

    for (building, archetype) in completed {
        let n = building.index as usize;
        let owner = state.c.owner[n];
        let hp = reg.units.by_archetype(archetype).map(|u| u.hp).unwrap_or(1);
        let spawn_at = spawn_position(state, reg, building);

        let Some(unit) = state.spawn(Spawn {
            archetype,
            owner,
            pos: spawn_at,
            hp,
            resource: 0,
        }) else {
            // Out of entity slots. The unit is lost rather than the match
            // crashing; §1.4 sizes the arrays so this should not happen, and
            // if it does `engine.yaml` is wrong.
            continue;
        };

        // Rally, if the building has one. A produced unit that walks to the
        // rally is the difference between a queue and a staging area.
        let rally = state.c.rally[n];
        if rally != FixedVec2::ZERO {
            state.c.dest[unit.index as usize] = rally;
            state.c.state[unit.index as usize] = UnitState::Moving;
        }
    }
}

/// Where a completed unit appears: just outside the building, on a ring that
/// advances with the tick so successive units do not stack on one point.
fn spawn_position(state: &SimState, reg: &Registries, building: EntityHandle) -> FixedVec2 {
    let n = building.index as usize;
    let centre = state.c.pos[n];
    let r = reg
        .units
        .by_archetype(state.c.archetype[n])
        .map(|u| u.collider_radius)
        .unwrap_or(Fixed::ONE);
    // Offset by a full building radius plus a margin, rotating by tick so a
    // stream of units fans out rather than piling on the same tile. Derived
    // from the clock, so it is identical on every client.
    let step = (state.clock.tick.0 % 8) as i64;
    let angle = trilateral_fixed::FixedAngle::from_turn_ratio(step, 8);
    let out = r + Fixed::ONE;
    centre + angle.to_unit_vec().scale(out)
}
