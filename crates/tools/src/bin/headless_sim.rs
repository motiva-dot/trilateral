//! The determinism arena. TECH_SPEC §9 job 5, CLAUDE.md §1.5.
//!
//! Builds a `SimState` from the real `engine.yaml`, populates it
//! deterministically, advances it, and writes a state hash at every checkpoint
//! tick. CI runs this on x86-64 Linux, ARM64 Linux and x86-64 Windows and
//! requires the three files to be byte-identical.
//!
//! # What this proves today, and what it does not
//! Through Phase 1 this printed a hardcoded `0xC0FFEE` — enough to prove the
//! CI plumbing (build, run, upload, compare) but nothing about the engine. It
//! now folds `Capacities`, `SimClock`, `SimRng`, the `EntityAllocator` free
//! list, every component array across all slots, and the `CommandLog`.
//!
//! What it does NOT yet prove is that *systems* are deterministic, because
//! there are no systems: `sim_systems::tick()` arrives in Phase 3. Until then
//! ticking advances the clock and nothing else, so this is a cross-platform
//! test of the state store, its hasher, and the fixed-point maths underneath —
//! not of simulation behaviour. The arena grows with the engine, and
//! IMPLEMENTATION_PLAN's standing rule is that it is never allowed to shrink.

use std::fmt::Write as _;

use sim_content::capacities_from_yaml;
use sim_core::{ArchetypeId, Command, IssuedCommand, PlayerId, SimState, Spawn, Tick};
use trilateral_fixed::{Fixed, FixedAngle, FixedVec2};

/// The capacities the arena runs at. Read from the committed file rather than
/// hardcoded, so the arena and the game agree by construction.
const ENGINE_YAML: &str = include_str!("../../../../assets/data/engine.yaml");

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
    let mut state = SimState::new(caps, seed);
    populate(&mut state);

    let mut report = String::new();
    for tick in 0..=ARENA_TICKS {
        if tick.is_multiple_of(CHECKPOINT_EVERY) {
            writeln!(report, "{tick}:{:016x}", state.hash()).expect("write to String");
        }
        if tick < ARENA_TICKS {
            state.clock.advance();
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
fn populate(state: &mut SimState) {
    let mut handles = Vec::with_capacity(ARENA_UNITS as usize);
    for i in 0..ARENA_UNITS {
        let h = state
            .spawn(Spawn {
                archetype: ArchetypeId((i % 23) as u16),
                owner: PlayerId((i % 3) as u8),
                pos: FixedVec2::new(
                    Fixed::from_ratio(i as i64, 7),
                    Fixed::from_ratio((i * 3) as i64, 11),
                ),
                hp: 40 + (i % 13) * 5,
            })
            .expect("engine.yaml max_entities must exceed the arena size");
        handles.push(h);

        let idx = h.index as usize;
        state.c.facing[idx] = FixedAngle::from_bits((i as u32).wrapping_mul(7_919) << 12);
        state.c.vel[idx] = FixedVec2::new(
            Fixed::from_ratio((i % 5) as i64 - 2, 30),
            Fixed::from_ratio((i % 7) as i64 - 3, 30),
        );
        state.c.shields[idx] = (i % 4) * 25;
    }

    // Kill a deterministic subset, so freed slots, bumped generations and the
    // free list all participate rather than staying pristine.
    for i in (0..handles.len()).step_by(37) {
        state.despawn(handles[i]);
    }

    // Ingest a mix of commands. Some subjects are now dead, so the reject path
    // and the rejection counter get exercised too — both are hashed state.
    for (n, h) in handles.iter().enumerate().take(120) {
        let cmd = match n % 4 {
            0 => Command::Move {
                target: FixedVec2::from_ints((n % 50) as i32, (n % 31) as i32),
            },
            1 => Command::Stop,
            2 => Command::HoldPosition,
            _ => Command::AttackMove {
                target: FixedVec2::from_ints((n % 17) as i32, (n % 19) as i32),
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
