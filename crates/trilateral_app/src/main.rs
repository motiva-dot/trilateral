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
