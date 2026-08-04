//! The Phase 4 exit gate, as executable tests.
//!
//! IMPLEMENTATION_PLAN Phase 4: *"50-scenario maze suite; 150-unit convergence
//! without jitter; building placement re-routes; determinism green."*
//!
//! These live in `tests/` rather than beside the code because they exercise the
//! whole pipeline through its public surface — which is also the only way to be
//! sure the gate is testing the engine rather than an internal shortcut.

use sim_core::registry::{Role, Shape, UnitCost, UnitRegistry, UnitStats};
use sim_core::{
    ArchetypeId, Capacities, Command, EntityHandle, IssuedCommand, OptionalHandle, PlayerId,
    Registries, SimState, Spawn, SteeringParams, Tile, UnitState,
};
use sim_systems::{SimContext, tick};
use trilateral_fixed::{Fixed, FixedVec2};

const MAP: u16 = 64;

fn caps(n: u32) -> Capacities {
    Capacities {
        max_entities: n,
        max_projectiles: 8,
        max_commands_per_tick: 256,
        max_players: 4,
        cmd_queue_slots: 16,
        modifier_slots: 8,
        command_log_reserve: 1024,
    }
}

fn registries() -> Registries {
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
            collider_radius: Fixed::from_ratio(1, 5),
            mass: 1,
            move_speed: Fixed::from_ratio(1, 5),
            turn_rate: Fixed::from_ratio(1, 10),
            sight_range: Fixed::from_int(5),
            selection_weight: 1,
            attack: None,
            resource: None,
            tags: vec![],
        }]),
        steering: SteeringParams {
            max_paths_per_tick: 32,
            ..SteeringParams::default()
        },
    }
}

fn centre_of(x: i32, y: i32) -> FixedVec2 {
    FixedVec2::new(
        Fixed::from_int(x) + Fixed::HALF,
        Fixed::from_int(y) + Fixed::HALF,
    )
}

fn spawn(s: &mut SimState, x: i32, y: i32) -> EntityHandle {
    s.spawn(Spawn {
        archetype: ArchetypeId(0),
        owner: PlayerId(0),
        pos: centre_of(x, y),
        hp: 10,
        resource: 0,
    })
    .unwrap()
}

fn order(s: &mut SimState, h: EntityHandle, at: FixedVec2) {
    s.ingest(IssuedCommand {
        tick: s.clock.tick,
        player: PlayerId(0),
        subject: h,
        command: Command::Move { target: at },
    })
    .unwrap();
}

fn context(s: &SimState, reg: &Registries) -> SimContext {
    SimContext::new(MAP as i32, reg, s.c.capacity(), s.grid.tile_count())
}

/// A deterministic pseudo-random maze, seeded by scenario number.
///
/// Not a perfect maze — a scatter of wall segments, which is closer to what an
/// RTS map actually contains than a labyrinth is, and much more likely to
/// produce the dead ends and narrow gaps that break naive pathing.
fn build_maze(s: &mut SimState, seed: u64) {
    let mut rng = sim_core::SimRng::from_seed(seed);
    for _ in 0..40 {
        let x = rng.below(MAP as u64 - 8) as u16 + 2;
        let y = rng.below(MAP as u64 - 8) as u16 + 2;
        let len = rng.below(10) as u16 + 3;
        let horizontal = rng.below(2) == 0;
        for k in 0..len {
            let t = if horizontal {
                Tile::new((x + k).min(MAP - 1), y)
            } else {
                Tile::new(x, (y + k).min(MAP - 1))
            };
            // Never seal the corners the test spawns and aims at.
            if (t.x < 4 && t.y < 4) || (t.x > MAP - 5 && t.y > MAP - 5) {
                continue;
            }
            s.grid.set_walkable(t, false);
        }
    }
}

// ---------------------------------------------------------------------------
// GATE 1 — 50-scenario maze suite
// ---------------------------------------------------------------------------

#[test]
fn gate_fifty_maze_scenarios() {
    let reg = registries();
    let mut solved = 0;
    let mut unreachable = 0;

    for seed in 1..=50u64 {
        let mut s = SimState::new(caps(16), seed, MAP);
        build_maze(&mut s, seed);
        let h = spawn(&mut s, 1, 1);
        let goal = centre_of(MAP as i32 - 2, MAP as i32 - 2);
        order(&mut s, h, goal);
        let mut ctx = context(&s, &reg);

        let mut arrived = false;
        for _ in 0..6000 {
            tick(&mut s, &reg, &mut ctx);
            let n = h.index as usize;

            // INVARIANT, every tick of every scenario: never inside a wall.
            let t = s.grid.tile_at(s.c.pos[n]);
            assert!(
                s.grid.is_terrain_walkable(t),
                "seed {seed}: unit entered a wall at {t:?}"
            );

            if s.c.state[n] == UnitState::Idle {
                arrived = s.c.pos[n].distance(goal) < Fixed::ONE;
                break;
            }
        }
        if arrived {
            solved += 1;
        } else {
            // A scenario can legitimately seal the goal off. What must never
            // happen is walking into a wall, which is asserted above.
            unreachable += 1;
        }
    }

    // The suite is only meaningful if most scenarios are actually solvable —
    // 50 sealed maps would pass the wall invariant trivially.
    assert!(
        solved >= 40,
        "only {solved}/50 solved ({unreachable} unreachable) — suite too easy or pathing broken"
    );
}

// ---------------------------------------------------------------------------
// GATE 2 — 150-unit convergence without jitter
// ---------------------------------------------------------------------------

#[test]
fn gate_one_hundred_fifty_units_converge_without_jitter() {
    let reg = registries();
    let mut s = SimState::new(caps(256), 42, MAP);
    let hs: Vec<_> = (0..150i32)
        .map(|i| spawn(&mut s, 4 + (i % 15), 4 + (i / 15)))
        .collect();

    // Everyone ordered to the SAME point. Without separation this is a stack;
    // without the settle rule it is a permanent scrum.
    let goal = centre_of(45, 45);
    for h in &hs {
        order(&mut s, *h, goal);
    }
    let mut ctx = context(&s, &reg);

    // Run until the crowd stops MOVING AT ALL, rather than guessing a horizon.
    //
    // "No jitter" is not "settles within N ticks" — it is "reaches a state it
    // then never leaves". So the test finds that state and then proves it is
    // stable, and reports how long it took if it never arrives. 150 units
    // ordered onto one point is the worst case the separation rule ever sees:
    // they arrive as a dense pile and have to spread from there.
    let snapshot = |st: &SimState| -> Vec<FixedVec2> {
        hs.iter().map(|h| st.c.pos[h.index as usize]).collect()
    };
    let mut still_for = 0;
    let mut prev = snapshot(&s);
    let mut settled_at = None;
    for t in 1..=30_000u32 {
        tick(&mut s, &reg, &mut ctx);
        let now = snapshot(&s);
        if now == prev {
            still_for += 1;
            if still_for >= 120 {
                settled_at = Some(t);
                break;
            }
        } else {
            still_for = 0;
        }
        prev = now;
    }
    let settled_at = settled_at.expect("150 units never stopped moving — jitter");

    // Nobody is still trying to go anywhere.
    let moving = hs
        .iter()
        .filter(|h| s.c.state[h.index as usize] == UnitState::Moving)
        .count();
    assert_eq!(moving, 0, "{moving} units still Moving after settling");

    // And the stillness is permanent, not a pause between shoves.
    let before = snapshot(&s);
    for _ in 0..600 {
        tick(&mut s, &reg, &mut ctx);
    }
    assert_eq!(snapshot(&s), before, "the crowd started moving again");

    // Sanity on the horizon: if this ever creeps toward the cap, the crowd is
    // converging far more slowly than it should and is worth a look.
    assert!(
        settled_at < 20_000,
        "took {settled_at} ticks to settle — convergence has regressed"
    );

    // And they are actually gathered, not scattered where they started.
    let near = hs
        .iter()
        .filter(|h| s.c.pos[h.index as usize].distance(goal) < Fixed::from_int(12))
        .count();
    assert!(near >= 120, "only {near}/150 reached the destination area");
}

// ---------------------------------------------------------------------------
// GATE 3 — building placement re-routes
// ---------------------------------------------------------------------------

#[test]
fn gate_building_placement_reroutes_traffic() {
    let reg = registries();
    let mut s = SimState::new(caps(16), 3, MAP);
    let h = spawn(&mut s, 2, 32);
    let goal = centre_of(60, 32);
    order(&mut s, h, goal);
    let mut ctx = context(&s, &reg);

    for _ in 0..150 {
        tick(&mut s, &reg, &mut ctx);
    }
    assert_eq!(s.c.state[h.index as usize], UnitState::Moving);

    // Drop a building wall across the route, exactly as Phase 6 will.
    let owner = OptionalHandle::some(EntityHandle::new(9, 1));
    for y in 20..45u16 {
        s.grid.set_occupancy(Tile::new(30, y), owner);
    }
    // Invalidate the stale route, as the building-placement system will.
    s.c.path[h.index as usize].clear();

    for _ in 0..8000 {
        tick(&mut s, &reg, &mut ctx);
        let t = s.grid.tile_at(s.c.pos[h.index as usize]);
        assert!(
            s.grid.occupancy_at(t).is_none(),
            "unit walked through a building at {t:?}"
        );
        if s.c.state[h.index as usize] == UnitState::Idle {
            break;
        }
    }
    assert!(
        s.c.pos[h.index as usize].x > Fixed::from_int(55),
        "unit never got round the building"
    );
}

// ---------------------------------------------------------------------------
// GATE 4 — determinism, with everything running at once
// ---------------------------------------------------------------------------

#[test]
fn gate_determinism_with_pathing_and_crowding() {
    fn run() -> u64 {
        let reg = registries();
        let mut s = SimState::new(caps(256), 99, MAP);
        build_maze(&mut s, 7);
        let hs: Vec<_> = (0..120i32)
            .map(|i| spawn(&mut s, 1 + (i % 3), 1 + (i / 3)))
            .collect();
        for (k, h) in hs.iter().enumerate() {
            order(
                &mut s,
                *h,
                centre_of(58 + (k % 4) as i32, 58 - (k % 5) as i32),
            );
        }
        let mut ctx = context(&s, &reg);
        for _ in 0..2000 {
            tick(&mut s, &reg, &mut ctx);
        }
        s.hash()
    }
    assert_eq!(run(), run(), "two identical runs diverged");
}
