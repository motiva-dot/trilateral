//! The determinism arena. TECH_SPEC §9 job 5, CLAUDE.md §1.5.
//!
//! Builds a `SimState` from the real `engine.yaml`, populates it
//! deterministically, advances it, and writes a state hash at every checkpoint
//! tick. CI runs this on x86-64 Linux, ARM64 Linux and x86-64 Windows and
//! requires the three files to be byte-identical.
//!
//! # What this proves, and how it has grown
//! Through Phase 1 this printed a hardcoded `0xC0FFEE` — enough to prove the
//! CI plumbing (build, run, upload, compare) and nothing else. Phase 2 gave it
//! a real `SimState` to fold: `Capacities`, `SimClock`, `SimRng`, the
//! `EntityAllocator` free list, every component array, and the `CommandLog`.
//!
//! **Phase 3 is the one that matters.** The arena now runs the real
//! `sim_systems::tick()` pipeline for 10,000 ticks with units executing move
//! orders, so the hashes depend on *system behaviour* — command execution
//! ordering, arrival arithmetic, vector normalisation, spatial reindexing —
//! and not merely on stored values. This is the first time CI can catch a
//! system that behaves differently on ARM than on x86.
//!
//! Per IMPLEMENTATION_PLAN's standing rule the arena grows with the engine and
//! is never allowed to shrink.

use std::fmt::Write as _;

use sim_content::{capacities_from_yaml, steering_from_yaml, units_from_yaml};
use sim_core::{Command, IssuedCommand, PlayerId, Registries, SimState, Spawn, Tick};
use sim_systems::{SimContext, tick};
use trilateral_fixed::{Fixed, FixedAngle, FixedVec2};

/// Content is read from the committed files rather than hardcoded, so the
/// arena and the game agree by construction.
const ENGINE_YAML: &str = include_str!("../../../../assets/data/engine.yaml");
const UNITS_YAML: &str = include_str!("../../../../assets/data/units.yaml");
const STEERING_YAML: &str = include_str!("../../../../assets/data/steering.yaml");
/// Map extent for the spatial hash. PRD §4 puts prototype maps at 128x128.
const WORLD_TILES: i32 = 128;

/// TECH_SPEC §9: 10,000 ticks, checkpoints every 1,000.
const ARENA_TICKS: u64 = 10_000;
const CHECKPOINT_EVERY: u64 = 1_000;
/// 600 scripted units, per the spec. Well inside `max_entities`.
const ARENA_UNITS: i32 = 600;
const ARENA_SEED: u64 = 42;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = arg_value(&args, "--out").unwrap_or_else(|| "arena_hashes.txt".into());
    let seed = arg_value(&args, "--seed")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(ARENA_SEED);

    let caps = capacities_from_yaml(ENGINE_YAML).expect("assets/data/engine.yaml must be valid");
    let reg = Registries {
        units: units_from_yaml(UNITS_YAML).expect("assets/data/units.yaml must be valid"),
        steering: steering_from_yaml(STEERING_YAML)
            .expect("assets/data/steering.yaml must be valid"),
    };
    let mut state = SimState::new(caps, seed);
    let mut ctx = SimContext::new(WORLD_TILES, &reg, state.capacities.max_entities);
    populate(&mut state, &reg);

    let mut report = String::new();
    for t in 0..=ARENA_TICKS {
        if t.is_multiple_of(CHECKPOINT_EVERY) {
            writeln!(report, "{t}:{:016x}", state.hash()).expect("write to String");
        }
        if t < ARENA_TICKS {
            tick(&mut state, &reg, &mut ctx);
        }
    }

    // `\n` only. `std::fs::write` performs no newline translation on Windows,
    // which is what lets determinism-compare byte-compare the three artifacts.
    std::fs::write(&out, report).expect("write hash file");
    println!("headless_sim: {ARENA_UNITS} units, {ARENA_TICKS} ticks, seed {seed} -> {out}");
}

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
}

/// Deterministic population. Every value derives from the loop index, so the
/// arena is a pure function of (engine.yaml, seed) with no ambient input.
///
/// Uses fixed-point positions, angles, and a spread of owners and archetypes,
/// so the hash exercises every component array rather than folding a field of
/// zeroes and calling it agreement.
fn populate(state: &mut SimState, reg: &Registries) {
    // Real archetypes from units.yaml, so the arena exercises the actual
    // move_speed values the game will use rather than invented ones.
    let roster: Vec<u16> = ["mur_drone", "mur_mite", "mur_lasher", "mur_supply_sac"]
        .iter()
        .map(|id| {
            reg.units
                .index_of(id)
                .unwrap_or_else(|| panic!("units.yaml is missing {id}")) as u16
        })
        .collect();

    let mut handles = Vec::with_capacity(ARENA_UNITS as usize);
    for i in 0..ARENA_UNITS {
        let archetype = sim_core::ArchetypeId(roster[(i as usize) % roster.len()]);
        let h = state
            .spawn(Spawn {
                archetype,
                owner: PlayerId((i % 3) as u8),
                pos: FixedVec2::new(
                    Fixed::from_ratio((i % 60) as i64, 1) + Fixed::from_ratio(i as i64, 7),
                    Fixed::from_ratio((i / 60) as i64, 1) + Fixed::from_ratio(i as i64, 11),
                ),
                hp: 40 + (i % 13) * 5,
            })
            .expect("engine.yaml max_entities must exceed the arena size");
        handles.push(h);

        let idx = h.index as usize;
        state.c.facing[idx] = FixedAngle::from_bits((i as u32).wrapping_mul(7_919) << 12);
        state.c.shields[idx] = (i % 4) * 25;
    }

    // Kill a deterministic subset, so freed slots, bumped generations and the
    // free list all participate rather than staying pristine.
    for i in (0..handles.len()).step_by(37) {
        state.despawn(handles[i]);
    }

    // Move orders spread across the first 30 ticks, at destinations far enough
    // away that units are still travelling at the last checkpoint. Some
    // subjects are already dead, so the reject path and the rejection counter
    // get exercised too — both are hashed state.
    for (n, h) in handles.iter().enumerate() {
        let cmd = match n % 4 {
            0 => Command::Move {
                target: FixedVec2::from_ints(100 - (n % 40) as i32, 100 - (n % 27) as i32),
            },
            1 => Command::Move {
                target: FixedVec2::from_ints((n % 23) as i32, 110 - (n % 31) as i32),
            },
            2 => Command::Stop,
            _ => Command::Move {
                target: FixedVec2::from_ints(115 - (n % 19) as i32, (n % 17) as i32),
            },
        };
        let _ = state.ingest(IssuedCommand {
            tick: Tick((n % 30) as u64),
            player: PlayerId((n % 3) as u8),
            subject: *h,
            command: cmd,
        });
    }
}
