//! Per-system timing against the budgets in TECH_SPEC §9.
//!
//! # Why the bench composes the pipeline itself
//! `sim_systems` cannot time itself: §1.2 bans `Instant` from simulation
//! crates, and a feature-gated exception would put a wall clock one careless
//! `cfg` away from the tick. So the bench calls each system in the §3.2 order
//! and times them from outside.
//!
//! That buys a real risk — the bench's copy of the pipeline drifting from the
//! real `tick()`. `--verify` exists for exactly that: it runs both for the same
//! number of ticks from the same seed and compares state hashes. CI runs it.
//!
//! # On CI numbers
//! Shared runners are noisy; a 15% run-to-run threshold would flap constantly.
//! So `--check-budgets` enforces the TECH_SPEC CEILINGS, which have real
//! headroom, and the run-to-run comparison is a local tool. A ceiling
//! breach means something got genuinely slower, not that a neighbour VM was
//! busy.

use std::time::Instant;

use sim_content::{capacities_from_yaml, races_from_yaml, steering_from_yaml, units_from_yaml};
use sim_core::{
    ArchetypeId, Command, IssuedCommand, PlayerId, Registries, SimState, Spawn, Tick, Tile,
};
use sim_systems::{SimContext, economy, movement, pathing, production, steering, tick};
use trilateral_fixed::{Fixed, FixedVec2};

const ENGINE_YAML: &str = include_str!("../../../../assets/data/engine.yaml");
const UNITS_YAML: &str = include_str!("../../../../assets/data/units.yaml");
const STEERING_YAML: &str = include_str!("../../../../assets/data/steering.yaml");
const RACES_YAML: &str = include_str!("../../../../assets/data/races.yaml");

/// PRD §4: the prototype target is 1,200 entities at 30Hz.
const ENTITIES: i32 = 1_200;
const WORLD: u16 = 128;
const WARMUP: u32 = 200;
const MEASURE: u32 = 1_000;

/// TECH_SPEC §9 per-tick budgets at 1,200 entities, in milliseconds.
struct Budget {
    name: &'static str,
    ms: f64,
}

const BUDGETS: &[Budget] = &[
    Budget {
        name: "steering",
        ms: 2.5,
    },
    Budget {
        name: "pathfinding",
        ms: 1.5,
    },
    Budget {
        name: "movement+spatial",
        ms: 0.8,
    },
    Budget {
        name: "commands+economy+production",
        ms: 0.4,
    },
];

#[derive(Default)]
struct Timings {
    command_execution: f64,
    pathfinding: f64,
    steering: f64,
    movement_spatial: f64,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let verify = args.iter().any(|a| a == "--verify");
    let check = args.iter().any(|a| a == "--check-budgets");

    if verify {
        std::process::exit(i32::from(!verify_pipeline_matches()));
    }

    let (mut state, reg, mut ctx) = build();
    // Warm up: the first ticks pay for page faults and the initial pathing
    // backlog, neither of which is representative of steady state.
    for _ in 0..WARMUP {
        tick(&mut state, &reg, &mut ctx);
    }

    let mut t = Timings::default();
    for _ in 0..MEASURE {
        let a = Instant::now();
        movement::command_execution(&mut state, &reg);
        production::production(&mut state, &reg);
        economy::economy(&mut state, &reg, &mut ctx);
        let b = Instant::now();
        pathing::pathfinding(&mut state, &reg, &mut ctx);
        let c = Instant::now();
        steering::steering(&state, &reg, &mut ctx);
        let d = Instant::now();
        movement::movement(&mut state, &reg, &ctx);
        ctx.spatial.rebuild(&state);
        let e = Instant::now();
        state.clock.advance();

        t.command_execution += (b - a).as_secs_f64();
        t.pathfinding += (c - b).as_secs_f64();
        t.steering += (d - c).as_secs_f64();
        t.movement_spatial += (e - d).as_secs_f64();
    }

    let n = MEASURE as f64;
    let measured = [
        ("steering", t.steering / n * 1000.0),
        ("pathfinding", t.pathfinding / n * 1000.0),
        ("movement+spatial", t.movement_spatial / n * 1000.0),
        (
            "commands+economy+production",
            t.command_execution / n * 1000.0,
        ),
    ];
    let total: f64 = measured.iter().map(|(_, ms)| ms).sum();

    println!("bench_match: {ENTITIES} entities, {MEASURE} ticks, {WORLD}x{WORLD} map");
    println!(
        "{:<20} {:>9}  {:>9}  {:>7}",
        "system", "ms/tick", "budget", "used"
    );
    let mut over = Vec::new();
    for (name, ms) in measured {
        let budget = BUDGETS.iter().find(|b| b.name == name).map(|b| b.ms);
        match budget {
            Some(b) => {
                let pct = ms / b * 100.0;
                println!("{name:<20} {ms:>9.3}  {b:>9.3}  {pct:>6.1}%");
                if ms > b {
                    over.push((name, ms, b));
                }
            }
            None => println!("{name:<20} {ms:>9.3}  {:>9}  {:>7}", "-", "-"),
        }
    }
    // PRD §4: <= 8ms per tick at 1,200 entities, 33ms wall.
    println!("{:<20} {total:>9.3}  {:>9.3}", "TOTAL (measured)", 8.0);

    if check {
        if over.is_empty() {
            println!("\nAll measured systems inside their TECH_SPEC §9 budgets.");
        } else {
            for (name, ms, b) in &over {
                eprintln!("::error::{name} took {ms:.3}ms/tick, budget is {b:.3}ms");
            }
            std::process::exit(1);
        }
        if total > 8.0 {
            eprintln!("::error::total {total:.3}ms/tick exceeds the 8ms PRD §4 target");
            std::process::exit(1);
        }
    }
}

fn build() -> (SimState, Registries, SimContext) {
    let caps = capacities_from_yaml(ENGINE_YAML).expect("engine.yaml");
    let reg = Registries {
        units: units_from_yaml(UNITS_YAML).expect("units.yaml"),
        steering: steering_from_yaml(STEERING_YAML).expect("steering.yaml"),
        race: races_from_yaml(RACES_YAML).expect("races.yaml"),
    };
    let mut state = SimState::new(caps, 42, WORLD);

    // Obstacles, so pathfinding does real work rather than emitting straight
    // lines. A bench on an empty map measures the wrong thing.
    for y in 10..110u16 {
        if !(58..64).contains(&y) {
            state.grid.set_walkable(Tile::new(64, y), false);
        }
    }
    for gx in 0..10u16 {
        for gy in 0..10u16 {
            state
                .grid
                .set_walkable(Tile::new(8 + gx * 5, 8 + gy * 5), false);
        }
    }

    let roster: Vec<u16> = ["mur_mite", "mur_drone", "mur_lasher", "mur_supply_sac"]
        .iter()
        .map(|id| reg.units.index_of(id).expect("unit missing") as u16)
        .collect();

    let mut handles = Vec::with_capacity(ENTITIES as usize);
    for i in 0..ENTITIES {
        let col = i % 40;
        let row = i / 40;
        let h = state
            .spawn(Spawn {
                archetype: ArchetypeId(roster[(i as usize) % roster.len()]),
                owner: PlayerId((i % 2) as u8),
                pos: FixedVec2::new(
                    Fixed::from_int(4 + col) + Fixed::HALF,
                    Fixed::from_int(4 + row) + Fixed::HALF,
                ),
                hp: 40,
                resource: 0,
            })
            .expect("engine.yaml max_entities must exceed the bench size");
        handles.push(h);
    }

    // Everyone crosses the map through the gap: sustained pathing, sustained
    // crowding, sustained spatial churn. This is the worst realistic case.
    for (k, h) in handles.iter().enumerate() {
        let _ = state.ingest(IssuedCommand {
            tick: Tick((k % 60) as u64),
            player: state.c.owner[h.index as usize],
            subject: *h,
            command: Command::Move {
                target: FixedVec2::new(
                    Fixed::from_int(100 + (k % 20) as i32),
                    Fixed::from_int(20 + (k % 80) as i32),
                ),
            },
        });
    }

    let ctx = SimContext::new(
        WORLD as i32,
        &reg,
        state.capacities.max_entities,
        state.grid.tile_count(),
    );
    (state, reg, ctx)
}

/// Prove the bench's hand-composed pipeline still matches the real `tick()`.
///
/// Without this the bench slowly becomes a measurement of something that is
/// not the game.
fn verify_pipeline_matches() -> bool {
    let (mut a, reg_a, mut ctx_a) = build();
    let (mut b, reg_b, mut ctx_b) = build();

    for _ in 0..300 {
        tick(&mut a, &reg_a, &mut ctx_a);

        movement::command_execution(&mut b, &reg_b);
        production::production(&mut b, &reg_b);
        economy::economy(&mut b, &reg_b, &mut ctx_b);
        pathing::pathfinding(&mut b, &reg_b, &mut ctx_b);
        steering::steering(&b, &reg_b, &mut ctx_b);
        movement::movement(&mut b, &reg_b, &ctx_b);
        ctx_b.spatial.rebuild(&b);
        b.clock.advance();
    }

    let (ha, hb) = (a.hash(), b.hash());
    if ha == hb {
        println!("bench pipeline matches sim_systems::tick() — {ha:#018x}");
        true
    } else {
        eprintln!("::error::bench pipeline has DRIFTED from sim_systems::tick()");
        eprintln!("  tick():      {ha:#018x}");
        eprintln!("  bench order: {hb:#018x}");
        false
    }
}
