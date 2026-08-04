//! §3.2 step 12 — hand out routes to units that need one.
//!
//! # A per-tick budget, and why it is not cheating
//! A* over a 128x128 map is cheap for one unit and ruinous for two hundred
//! ordered at the same instant — which is exactly what a box-select and a
//! right-click produce. So a bounded number of searches run per tick and the
//! rest wait their turn.
//!
//! That is a real behaviour, not a hack: a unit with no path yet simply has
//! not set off. At 30Hz a backlog of 200 units at 16 searches per tick clears
//! in 13 ticks — under half a second, and it reads as the group "getting
//! going" rather than teleporting into formation.
//!
//! The budget is served in ascending entity index (§6.9), so *which* units go
//! first is a property of the state, not of timing. Two clients with the same
//! command log serve the same units in the same order on the same ticks.
//!
//! # Repathing
//! A unit whose slot runs out re-paths from wherever it now stands. That falls
//! out of the design rather than being bolted on, and it is also what makes
//! units react to a building placed across their route after they set off.

use sim_core::{Registries, SimState, UnitState};

use crate::SimContext;
use crate::path::{find_path, simplify};

pub fn pathfinding(state: &mut SimState, reg: &Registries, ctx: &mut SimContext) {
    let budget = reg.steering.max_paths_per_tick;
    if budget == 0 {
        return;
    }
    let mut served = 0u16;

    for i in 0..state.c.capacity() {
        if served >= budget {
            break;
        }
        if !state.c.alive.get(i) {
            continue;
        }
        let n = i as usize;
        // Harvesters and returners travel too — the pathfinder serves any
        // state that is trying to be somewhere else, not just a plain Move.
        let travelling = matches!(
            state.c.state[n],
            UnitState::Moving | UnitState::Harvesting | UnitState::ReturningCargo
        );
        if !travelling || !state.c.path[n].is_empty() {
            continue;
        }
        // A cleared destination means "arrived, nothing to walk to". Without
        // this a harvester standing at its node would re-path every tick.
        if state.c.dest[n] == trilateral_fixed::FixedVec2::ZERO {
            continue;
        }
        // A unit that cannot move cannot use a path.
        let can_move = reg
            .units
            .by_archetype(state.c.archetype[n])
            .map(|u| u.move_speed > trilateral_fixed::Fixed::ZERO)
            .unwrap_or(false);
        if !can_move {
            state.c.state[n] = UnitState::Idle;
            continue;
        }

        served += 1;
        // EGRESS. A unit can legitimately be standing inside blocked tiles: it
        // spawned in a base.s footprint, or a building went up on top of it.
        // Pathing from a sealed tile finds nothing, so the unit would be
        // entombed forever. Start the search from the nearest tile it could
        // stand on instead — statics are occupancy, not collision, so it can
        // physically walk out.
        let here = state.grid.tile_at(state.c.pos[n]);
        let from = crate::path::nearest_walkable(&state.grid, here, 4).unwrap_or(here);
        // Walk as close as possible to a blocked destination rather than
        // refusing the order: harvest targets and buildings both occupy tiles.
        let Some(to) =
            crate::path::nearest_walkable(&state.grid, state.grid.tile_at(state.c.dest[n]), 4)
        else {
            state.c.state[n] = UnitState::Idle;
            state.c.dest[n] = trilateral_fixed::FixedVec2::ZERO;
            continue;
        };

        if from == to {
            // Already in the destination tile — walk straight at the exact
            // destination rather than aiming at the tile centre and arriving
            // somewhere subtly wrong.
            let single = [state.grid.index(to)];
            state.c.path[n].set(&single);
            continue;
        }

        let found = find_path(
            &state.grid,
            &mut ctx.path_scratch,
            from,
            to,
            &mut ctx.path_out,
        );
        if !found {
            // Unreachable. Stop rather than walking hopefully into a wall —
            // and clear the destination so the unit does not re-request the
            // same impossible path every tick for the rest of the match.
            state.c.state[n] = UnitState::Idle;
            state.c.dest[n] = trilateral_fixed::FixedVec2::ZERO;
            state.c.stuck_ticks[n] = 0;
            continue;
        }

        simplify(&mut ctx.path_out);
        // Drop the first tile: the unit is already standing in it, and aiming
        // at its centre would drag the unit backwards before it set off.
        let start = usize::from(ctx.path_out.len() > 1);
        ctx.path_indices.clear();
        for t in &ctx.path_out[start..] {
            ctx.path_indices.push(state.grid.index(*t));
        }
        state.c.path[n].set(&ctx.path_indices);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tick;
    use sim_core::registry::{Role, Shape, UnitCost, UnitRegistry, UnitStats};
    use sim_core::{
        ArchetypeId, Capacities, Command, EntityHandle, IssuedCommand, PlayerId, Spawn,
        SteeringParams, Tile,
    };
    use trilateral_fixed::{Fixed, FixedVec2};

    fn caps() -> Capacities {
        Capacities {
            max_entities: 64,
            max_projectiles: 8,
            max_commands_per_tick: 64,
            max_players: 4,
            cmd_queue_slots: 16,
            modifier_slots: 8,
            command_log_reserve: 256,
        }
    }

    fn registries(budget: u16) -> Registries {
        Registries {
            units: UnitRegistry::new(vec![UnitStats {
                id: "runner".into(),
                faction: "t".into(),
                role: Role::Melee,
                shape: Shape::Circle,
                cost: UnitCost {
                    ore: 1,
                    flux: 0,
                    ticks: 1,
                },
                supply_x2: 2,
                provides_supply_x2: 0,
                hp: 10,
                armor: 0,
                collider_radius: Fixed::from_ratio(1, 8),
                mass: 1,
                move_speed: Fixed::from_ratio(1, 4),
                turn_rate: Fixed::from_ratio(1, 10),
                sight_range: Fixed::from_int(5),
                selection_weight: 1,
                attack: None,
                resource: None,
                tags: vec![],
            }]),
            steering: SteeringParams {
                max_paths_per_tick: budget,
                ..SteeringParams::default()
            },
        }
    }

    fn spawn(s: &mut SimState, x: i32, y: i32) -> EntityHandle {
        s.spawn(Spawn {
            archetype: ArchetypeId(0),
            owner: PlayerId(0),
            pos: FixedVec2::new(
                Fixed::from_int(x) + Fixed::HALF,
                Fixed::from_int(y) + Fixed::HALF,
            ),
            hp: 10,
            resource: 0,
        })
        .unwrap()
    }

    fn order(s: &mut SimState, h: EntityHandle, x: i32, y: i32) {
        s.ingest(IssuedCommand {
            tick: s.clock.tick,
            player: PlayerId(0),
            subject: h,
            command: Command::Move {
                target: FixedVec2::new(
                    Fixed::from_int(x) + Fixed::HALF,
                    Fixed::from_int(y) + Fixed::HALF,
                ),
            },
        })
        .unwrap();
    }

    fn ctx(s: &SimState, reg: &Registries) -> SimContext {
        SimContext::new(64, reg, s.c.capacity(), s.grid.tile_count())
    }

    #[test]
    fn a_mover_is_given_a_route() {
        let reg = registries(16);
        let mut s = SimState::new(caps(), 1, 64);
        let h = spawn(&mut s, 2, 2);
        order(&mut s, h, 20, 2);
        let mut c = ctx(&s, &reg);
        tick(&mut s, &reg, &mut c);
        assert!(
            !s.c.path[h.index as usize].is_empty(),
            "no route was handed out"
        );
    }

    #[test]
    fn a_unit_walks_around_a_wall_instead_of_into_it() {
        // The whole point of wiring the pathfinder in.
        let reg = registries(16);
        let mut s = SimState::new(caps(), 1, 64);
        for y in 0..20u16 {
            s.grid.set_walkable(Tile::new(10, y), false);
        }
        let h = spawn(&mut s, 2, 2);
        order(&mut s, h, 20, 2);
        let mut c = ctx(&s, &reg);

        let mut ever_below = false;
        for _ in 0..2000 {
            tick(&mut s, &reg, &mut c);
            let p = s.c.pos[h.index as usize];
            // Never inside the wall column.
            let t = s.grid.tile_at(p);
            assert!(
                s.grid.is_terrain_walkable(t),
                "unit entered the wall at {t:?}"
            );
            if p.y > Fixed::from_int(19) {
                ever_below = true;
            }
            if s.c.state[h.index as usize] == UnitState::Idle && ever_below {
                break;
            }
        }
        assert!(ever_below, "unit never went round the end of the wall");
        let end = s.c.pos[h.index as usize];
        assert!(
            end.x > Fixed::from_int(19),
            "unit did not reach the far side, stopped at {end:?}"
        );
    }

    #[test]
    fn an_unreachable_destination_stops_the_unit_rather_than_looping() {
        // Sealed goal. The unit must give up AND forget the destination, or it
        // re-requests the same impossible search every tick forever.
        let reg = registries(16);
        let mut s = SimState::new(caps(), 1, 64);
        for d in -1..=1i32 {
            s.grid.set_walkable(Tile::new((30 + d) as u16, 29), false);
            s.grid.set_walkable(Tile::new((30 + d) as u16, 31), false);
        }
        s.grid.set_walkable(Tile::new(29, 30), false);
        s.grid.set_walkable(Tile::new(31, 30), false);

        let h = spawn(&mut s, 2, 2);
        order(&mut s, h, 30, 30);
        let mut c = ctx(&s, &reg);
        for _ in 0..10 {
            tick(&mut s, &reg, &mut c);
        }
        assert_eq!(s.c.state[h.index as usize], UnitState::Idle);
        assert_eq!(
            s.c.dest[h.index as usize],
            FixedVec2::ZERO,
            "destination not cleared — will re-path forever"
        );
    }

    #[test]
    fn the_budget_serves_units_in_ascending_index_order() {
        // Which units set off first must be a property of the state, not of
        // timing. Budget of 2 with 6 movers: exactly 0 and 1 go first.
        let reg = registries(2);
        let mut s = SimState::new(caps(), 1, 64);
        let hs: Vec<_> = (0..6).map(|i| spawn(&mut s, 2 + i, 2)).collect();
        for h in &hs {
            order(&mut s, *h, 40, 40);
        }
        let mut c = ctx(&s, &reg);
        tick(&mut s, &reg, &mut c);

        let has_path: Vec<bool> = hs
            .iter()
            .map(|h| !s.c.path[h.index as usize].is_empty())
            .collect();
        assert_eq!(
            has_path,
            vec![true, true, false, false, false, false],
            "budget was not served in index order"
        );
    }

    #[test]
    fn a_backlog_clears_and_everyone_eventually_moves() {
        let reg = registries(2);
        let mut s = SimState::new(caps(), 1, 64);
        let hs: Vec<_> = (0..10).map(|i| spawn(&mut s, 2 + i, 2)).collect();
        for h in &hs {
            order(&mut s, *h, 2 + (h.index as i32), 40);
        }
        let mut c = ctx(&s, &reg);
        for _ in 0..600 {
            tick(&mut s, &reg, &mut c);
        }
        for h in &hs {
            assert!(
                s.c.pos[h.index as usize].y > Fixed::from_int(30),
                "unit {} never got going",
                h.index
            );
        }
    }

    #[test]
    fn a_wall_built_across_a_route_is_noticed_on_repath() {
        // Falls out of the design: the slot runs out, the unit re-paths from
        // where it stands, and the new wall is simply part of the grid.
        let reg = registries(16);
        let mut s = SimState::new(caps(), 1, 64);
        let h = spawn(&mut s, 2, 30);
        order(&mut s, h, 60, 30);
        let mut c = ctx(&s, &reg);
        for _ in 0..100 {
            tick(&mut s, &reg, &mut c);
        }
        // Drop a wall ahead of it and clear the stale route.
        for y in 20..40u16 {
            s.grid.set_walkable(Tile::new(30, y), false);
        }
        s.c.path[h.index as usize].clear();
        for _ in 0..4000 {
            tick(&mut s, &reg, &mut c);
            let t = s.grid.tile_at(s.c.pos[h.index as usize]);
            assert!(s.grid.is_terrain_walkable(t), "walked into the new wall");
            if s.c.state[h.index as usize] == UnitState::Idle {
                break;
            }
        }
    }

    #[test]
    fn path_following_is_reproducible() {
        fn run() -> u64 {
            let reg = registries(4);
            let mut s = SimState::new(caps(), 7, 64);
            for y in 10..50u16 {
                s.grid.set_walkable(Tile::new(25, y), false);
            }
            let hs: Vec<_> = (0..12)
                .map(|i| spawn(&mut s, 2 + i % 4, 20 + i / 4))
                .collect();
            for h in &hs {
                order(&mut s, *h, 50, 30);
            }
            let mut c = ctx(&s, &reg);
            for _ in 0..800 {
                tick(&mut s, &reg, &mut c);
            }
            s.hash()
        }
        assert_eq!(run(), run());
    }
}
