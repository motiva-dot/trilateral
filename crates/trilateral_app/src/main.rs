//! trilateral — wiring only. TECH_SPEC §1: this crate depends on everything
//! and contains no logic of its own.
//!
//! Phase 3.5: loads the real content, builds a simulation, and opens the debug
//! view. There is no menu, no map file and no win condition yet — the point is
//! that movement can be *watched*, because feel cannot be judged headlessly.
//!
//! Controls:
//!   WASD / arrows  pan          scroll  zoom
//!   left click     select       drag    box select
//!   right click    move order   space   pause      esc  quit

use presentation::app::DebugApp;
use sim_content::{capacities_from_yaml, steering_from_yaml, units_from_yaml};
use sim_core::{ArchetypeId, PlayerId, Registries, SimState, Spawn};
use trilateral_fixed::{Fixed, FixedVec2};
use winit::event_loop::{ControlFlow, EventLoop};

const ENGINE_YAML: &str = include_str!("../../../assets/data/engine.yaml");
const UNITS_YAML: &str = include_str!("../../../assets/data/units.yaml");
const STEERING_YAML: &str = include_str!("../../../assets/data/steering.yaml");

/// PRD §4: prototype maps are 128x128 tiles.
const WORLD_TILES: i32 = 128;
/// Phase 3.5 gate: 500 units visible and moving.
const DEMO_UNITS: i32 = 500;

fn main() {
    let caps = capacities_from_yaml(ENGINE_YAML).expect("engine.yaml");
    let reg = Registries {
        units: units_from_yaml(UNITS_YAML).expect("units.yaml"),
        steering: steering_from_yaml(STEERING_YAML).expect("steering.yaml"),
    };
    let mut state = SimState::new(caps, 42, WORLD_TILES as u16);
    build_obstacles(&mut state);
    populate(&mut state, &reg);

    println!(
        "trilateral debug view — {} units, {} archetypes loaded",
        state.live_count(),
        reg.units.len()
    );
    println!("WASD/arrows pan · scroll zoom · click select · drag box-select");
    println!("right-click move · space pause · esc quit");

    let event_loop = EventLoop::new().expect("create event loop");
    // Poll rather than Wait: the simulation must advance on its own schedule,
    // not only when an input event arrives.
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = DebugApp::new(state, reg, WORLD_TILES);
    event_loop.run_app(&mut app).expect("event loop");
}

/// Walls, so there is something to path *around*.
///
/// Until now the demo map was empty, which meant the pathfinder was provably
/// correct and completely invisible — a straight line across open ground looks
/// identical whether A\* produced it or a beeline did. These obstacles exist
/// purely so the behaviour can be judged by eye: a long wall with one gap, a
/// pillar field that forces constant small course corrections, and a corridor
/// narrow enough that a group has to file through it.
///
/// Not a real map. Maps are RON files with symmetry validation (GAME_DESIGN
/// §6) and arrive at Phase 6; this is scaffolding for looking at movement.
fn build_obstacles(state: &mut SimState) {
    use sim_core::Tile;

    // A long wall with a single gap, between the two upper clusters.
    for y in 8..46u16 {
        if !(26..30).contains(&y) {
            state.grid.set_walkable(Tile::new(44, y), false);
        }
    }

    // A pillar field. Each is small enough to walk past, dense enough that a
    // group crossing it has to keep adjusting — the case where jitter and
    // oscillation show up if the steering is wrong.
    for gx in 0..6u16 {
        for gy in 0..4u16 {
            let x = 20 + gx * 5;
            let y = 40 + gy * 5;
            state.grid.set_walkable(Tile::new(x, y), false);
            state.grid.set_walkable(Tile::new(x + 1, y), false);
            state.grid.set_walkable(Tile::new(x, y + 1), false);
            state.grid.set_walkable(Tile::new(x + 1, y + 1), false);
        }
    }

    // A corridor two tiles wide. A group ordered through it must file, which
    // is where the settle rule and the pathing budget are most visible.
    for x in 60..100u16 {
        state.grid.set_walkable(Tile::new(x, 60), false);
        state.grid.set_walkable(Tile::new(x, 63), false);
    }
}

/// A readable starting arrangement: three blocks of units, one per player,
/// spread far enough apart that group movement is visible.
fn populate(state: &mut SimState, reg: &Registries) {
    let roster: Vec<u16> = ["mur_mite", "mur_drone", "mur_lasher"]
        .iter()
        .map(|id| reg.units.index_of(id).expect("unit missing") as u16)
        .collect();

    for i in 0..DEMO_UNITS {
        let player = (i % 3) as u8;
        let col = i % 20;
        let row = i / 20;
        // Three clusters, one per player, laid out across the map.
        let origin = match player {
            0 => (16.0, 20.0),
            1 => (72.0, 20.0),
            _ => (44.0, 80.0),
        };
        let pos = FixedVec2::new(
            Fixed::from_f64(origin.0 + (col as f64) * 0.9),
            Fixed::from_f64(origin.1 + (row as f64) * 0.9),
        );
        state
            .spawn(Spawn {
                archetype: ArchetypeId(roster[(i as usize) % roster.len()]),
                owner: PlayerId(player),
                pos,
                hp: 40,
            })
            .expect("engine.yaml max_entities must exceed the demo size");
    }
}
