# CLAUDE.md — Governance Protocol v2
# Project: TRILATERAL — A Deterministic Competitive RTS (working title)
# Supersedes all v1 documents. Grounded in 00_AUDIT.md verdicts.

> **This file is the supreme authority for all code in this repository.**
> Re-read it at the start of every session. When any other document conflicts
> with this file, this file wins. When this file conflicts with 00_AUDIT.md,
> flag the conflict to the human architect and stop.

---

## 0. SESSION PROTOCOL

### 0.1 Session Start
1. Read this file in full.
2. Read `docs/ARCHITECTURE_LEDGER.md` (current codebase state).
3. Read the latest entry in `docs/SESSION_LOG.md`.
4. Run `cargo test --workspace`. If anything fails, fixing it is task #1.
5. State which phase and task (from `docs/IMPLEMENTATION_PLAN.md`) you are on.

### 0.2 Session End
1. `cargo test --workspace` — green.
2. `./scripts/ban_floats.sh` — zero violations.
3. `cargo clippy --workspace -- -D warnings` — clean.
4. Update `docs/ARCHITECTURE_LEDGER.md` (what exists now, key type names, ADRs).
5. Append to `docs/SESSION_LOG.md`: date, phase, completed, tests added/passing,
   known issues, concrete next steps.
6. Commit with a descriptive message.

### 0.3 If You Notice Context Drift
If you find yourself inventing type names, importing crates not in Cargo.toml,
or unsure why a rule exists: stop, re-read this file and the Ledger, and state
what you re-learned before continuing.

---

## 1. SUPREME ENGINE LAWS

### 1.1 The Float Ban (Simulation)
- No `f32`/`f64`, no `std` float functions, no float literals anywhere in
  `crates/sim_core/` or `crates/sim_systems/`.
- All simulation math uses `trilateral_fixed`: **`Fixed` = i64-backed Q32.32**.
  Multiplication and division widen through `i128`. (Q16.16-on-i32 was
  rejected in the audit for map-scale overflow — do not regress to it.)
- Trig via `fixed_trig` lookup tables (4096-entry sin table, CORDIC atan2).
- `Fixed::from_f64`/`to_f64` exist only behind `feature = "float_bridge"`,
  enabled only by the YAML loader (parse-time) and the presentation crate.
- Time in the simulation is `Tick(u64)`. There is no delta time, no wall clock.

### 1.2 The Entropy Ban
- No `HashMap`/`HashSet`, no `thread_rng`, no `SystemTime`/`Instant`, no OS
  entropy in simulation crates. `BTreeMap` or index-based arrays only.
- All simulation randomness flows through `SimRng` (seeded xoshiro256**),
  and per the design spec, **combat resolution uses no randomness at all** —
  `SimRng` is for map generation and data-driven optional modes only.

### 1.3 The Dependency Ban
- `sim_core` and `sim_systems` may depend ONLY on: `trilateral_fixed`,
  `serde`, `smallvec`. Nothing else, ever, without a written ADR in the Ledger
  approved by the human architect. Enforced by `cargo deny` in CI.

### 1.4 The Steady-State Allocation Rule
- All simulation buffers are pre-sized at match start from YAML-declared
  maxima (`max_entities`, `max_projectiles`, `max_commands_per_tick`, ...).
- The benchmark harness wraps the global allocator and asserts **zero heap
  allocations between tick 1,000 and tick 2,000** of the standard benchmark
  match. This test is the enforcement mechanism; write code accordingly.

### 1.5 The Determinism Invariant (The Only Test That Really Matters)
- Same seed + same command log ⇒ bit-identical state hash at every checkpoint
  tick, across x86-64 Linux, ARM64 Linux, and x86-64 Windows.
- CI runs this on every push (see `.github/workflows/ci.yml`). A red
  determinism job blocks all merges, no exceptions.

### 1.6 The Render Decoupling Law
- `crates/presentation/` READS simulation state; it never writes it.
- All visual/audio triggers arrive via the `SimEventBuffer`, drained once per
  tick. Visual-only state lives in `RenderState`, owned by presentation.

### 1.7 The Replay Mandate
- Every `Command` variant is serde-serializable. Any action that cannot be
  reproduced from `(seed, command log)` alone is rejected in review.
- The `CommandLogger` records every command with its tick. This is not
  optional instrumentation; replays, reconnection, the desync arena, and the
  ghost-trainer feature all depend on it.

### 1.8 The Asset Rule
- No gameplay number may be hardcoded in Rust: no HP, damage, costs, ranges,
  timings, supply, tech prerequisites, AI thresholds. Everything loads from
  `assets/data/*.yaml` into registries at match start, hot-reloadable in dev.

---

## 2. STANDARD OPERATING PROCEDURE (Every Task)

1. **Plan** — 3 bullets: components/arrays touched, invariants preserved,
   test scenario.
2. **Test first** — headless deterministic test in the relevant `tests/` dir:
   build minimal state → inject commands → tick N → assert exact fixed-point
   values (never "approximately").
3. **Implement** — inside the assigned crate only.
4. **Self-lint** — `./scripts/ban_floats.sh` && `cargo clippy -- -D warnings`.
5. **Verify determinism** — `cargo test -p sim_systems --test determinism`.
6. **Commit** — only when 1–5 pass. On a determinism failure: do NOT guess;
   run `./scripts/run_desync_trace.sh`, diff the dumped states, find the exact
   component/entity/tick, fix the root cause.

---

## 3. STATE ARCHITECTURE — The SoA SimState (Not an ECS Framework)

Per the audit, we use a **custom Structure-of-Arrays state store**, not Bevy
and not hecs. Rationale: snapshot = memcpy, hash = contiguous buffers in fixed
order, no framework behavior to hallucinate against.

### 3.1 Core Shape
```rust
pub struct SimState {
    pub clock: SimClock,                  // tick: u64
    pub rng: SimRng,
    pub entities: EntityAllocator,        // generational handles + free list
    pub c: Components,                    // all component arrays (SoA)
    pub players: [PlayerState; MAX_PLAYERS],
    pub grid: MacroGrid,
    pub fog: FogMasks,                    // one bitmask per player
    pub spatial: SpatialHash,
    pub flow_cache: FlowFieldCache,
    pub events: SimEventBuffer,           // transient, cleared per tick
    pub cmd_log: CommandLog,
}

pub struct Components {
    // Parallel arrays indexed by EntityIndex. Every array has EXACTLY
    // `capacities.max_entities` elements, allocated once at match start from
    // assets/data/engine.yaml and never resized. See ADR-005.
    pub alive:        BitSet,                  // len = max_entities bits
    pub archetype:    Box<[ArchetypeId]>,      // index into UnitRegistry
    pub owner:        Box<[PlayerId]>,
    pub pos:          Box<[FixedVec2]>,
    pub facing:       Box<[FixedAngle]>,
    pub vel:          Box<[FixedVec2]>,
    pub hp:           Box<[i32]>,
    pub shields:      Box<[i32]>,              // Concord race mechanic
    pub incoming_dmg: Box<[i32]>,
    pub state:        Box<[UnitState]>,
    pub attack:       Box<[AttackState]>,
    pub target:       Box<[OptionalHandle]>,
    pub cmd_queue:    Box<[CmdQueue]>,         // fixed-cap ring, 16 slots
    pub modifiers:    Box<[ModifierStack]>,    // fixed-cap, 8 slots
    pub cargo:        Box<[Cargo]>,
    pub production:   Box<[ProductionQueue]>,
    pub cooldowns:    Box<[AbilityCooldowns]>,
}
```
- **Capacity is runtime, not compile-time (ADR-005, 2026-08-04).** Earlier
  drafts of this file wrote `[T; MAX_ENTITIES]`, which contradicted
  TECH_SPEC §3 and violated §1.8 (no gameplay number hardcoded in Rust).
  `Box<[T]>` is allocated once at match start and never grows, so §1.4 (no
  steady-state allocation) is satisfied exactly as before: the allocation
  happens at setup, not per tick, and the allocation gate measures ticks
  1,000–2,000.
- Every element type is `Copy` and fixed-size. `SimState::snapshot()` copies
  the boxed slices; `SimState::hash()` folds the arrays in declared order with
  a stable, fixed-seed, vendored 64-bit hasher (`SimHasher` — see ADR-006 for
  why not xxh3).
- Systems are plain functions: `fn movement_system(s: &mut SimState)`.

### 3.2 System Execution Order (Immutable, Explicitly Coded)
```
 1. command_ingest        — drain CommandBuffer into per-entity CmdQueues
 2. command_execution     — front-of-queue Command → UnitState transitions
 3. ai_tick               — bot InputSources emit commands (they enter next tick)
 4. target_acquisition    — staggered: (tick + handle_index) % STAGGER == 0
 5. combat_fsm            — frontswing/damage-point/backswing; turn-to-face
 6. projectile_advance    — move projectiles, detect impacts
 7. damage_application    — apply queued damage; update incoming_dmg
 8. death_cleanup         — free handles, emit UnitDied, clear reservations
 9. modifier_pipeline     — recompute effective stats from base + stack
10. production_tick       — build/research queues, spawn completions
11. economy_tick          — harvest slots, cargo, deposits (Ore & Flux)
12. pathfinding           — HPA*/flow-field assignment per policy (see §5.2)
13. steering              — local avoidance + mass priority + settle
14. movement              — integrate velocity; clamp; update spatial hash
15. fog_update            — per-player vision stamps
16. victory_check         — elimination / surrender conditions
17. state_hash            — every 10 ticks, fold and record
18. event_flush           — swap SimEventBuffer for presentation
19. command_log_append
```

---

## 4. UBIQUITOUS LANGUAGE (Do Not Invent Synonyms)

Types: `Fixed`, `FixedVec2`, `FixedAngle`, `Tick`, `EntityHandle`,
`ArchetypeId`, `PlayerId`, `SimState`, `SimRng`, `SimClock`, `MacroGrid`,
`FogMasks`, `SpatialHash`, `FlowFieldCache`, `SimEventBuffer`, `CommandLog`,
`CommandBuffer`, `UnitRegistry`, `TechRegistry`, `BuildingRegistry`,
`ModifierStack`, `AttackState`, `UnitState`, `CmdQueue`.

Resources (economy): **`Ore`** and **`Flux`**. Supply is `supply_x2: u16`
(stored doubled; displayed halved).

Races: **`Murmur`** (circles / swarm), **`Bastion`** (squares / fortification),
**`Concord`** (triangles / precision). See GAME_DESIGN.md — race mechanics are
data-driven wherever possible; race-specific systems get their own files
(`murmur_bloom.rs`, `bastion_construction.rs`, `concord_lattice.rs`).

```rust
pub enum Command {
    Move { target: FixedVec2 },
    AttackMove { target: FixedVec2 },
    AttackTarget { target: EntityHandle },
    Stop, HoldPosition,
    Patrol { waypoint: FixedVec2 },
    Harvest { node: EntityHandle },
    ReturnCargo,
    Build { building: ArchetypeId, pos: FixedVec2 },
    Train { unit: ArchetypeId },                    // issued to a building
    Research { tech: TechId },                      // issued to a building
    Cancel { queue_slot: u8 },
    UseAbility { ability: AbilityId, target: AbilityTarget },
    SetRally { pos: FixedVec2 },
    Surrender,
}
```
Every command is validated at ingest (ownership, cost, prerequisites, fog
legality for targeted commands) — an invalid command is dropped and logged,
never a panic. This is also the anti-cheat boundary for networked play.

---

## 5. SYSTEM DIRECTIVES

### 5.1 Netcode: Lockstep-First (Audit item 4)
- Deterministic lockstep, adaptive input delay 2–4 ticks, 30Hz simulation.
- Commands are scheduled for execution at `issue_tick + input_delay` on ALL
  clients simultaneously.
- Frame-0 local feedback (marker, bark, visual facing) masks the delay.
- Snapshots exist from day one (replay seeking / reconnect), but rollback
  re-simulation is explicitly OUT of the prototype scope.
- Stall handling: if a peer's commands for tick T are absent at T-ε, the sim
  pauses with a "waiting" indicator; reconnect uses command-log catch-up at
  max sim speed with presentation disabled.
- Desync detection: peers exchange state hashes every 60 ticks; mismatch ends
  the match with both replay logs saved for the desync trace tool.

### 5.2 Pathfinding Policy (Audit item 11)
- HPA* over 16×16-tile chunks for macro routes.
- Group ≤ `flow_field_threshold` (YAML, default 12): one shared HPA* path,
  members follow with formation offsets from the group anchor.
- Group > threshold, or destination already cached: integration flow field.
- Underneath both: local steering with velocity-obstacle avoidance, integer
  `mass` priority (heavier shoves lighter), harvest-state phasing, and the
  settle rule (near destination + blocked by idle friendly ⇒ Idle).
- Building placement/destruction dirties only affected chunks and cached
  fields.

### 5.3 Combat
- Three-phase attack FSM; backswing cancel on Move/Stop (stutter-step);
  frontswing cancel forfeits the attack. All phase durations from YAML.
- Facing gates: unit must be within `attack_arc` of target bearing to enter
  frontswing; turn rate from YAML via fixed trig tables.
- Target acquisition staggered by `(tick + handle_index) % 8`; priority:
  attacker-of-me > combat unit > worker > building, ties broken by lowest
  handle index (deterministic).
- Overkill prevention via `incoming_dmg` before committing a frontswing.
- **No randomness.** High ground = uphill vision denial + `high_ground_damage_taken_mult`
  (YAML). Optional classic miss-chance mode exists only as a YAML toggle for
  custom games.

### 5.4 Economy (Two Resources)
- **Ore**: patches of 6–9 nodes per base; each node `harvest_slots: 2`; workers
  reserve slots deterministically; diminishing returns past saturation emerge
  naturally from slot contention (no scripted soft cap).
- **Flux**: 1–2 extractor sites per base; requires race extractor building;
  fixed 3 harvest slots; gates tier-2/3 tech and advanced units.
- Worker phasing while `Harvesting`/`ReturnCargo` (collision mask off vs
  friendlies). Cargo amounts, trip times, and node capacities all in YAML.

### 5.5 Tech & Modifiers
- `TechRegistry` from `tech_tree.yaml`: id, race, tier, cost (Ore/Flux/time),
  prerequisites (buildings and techs), effects.
- ALL tech effects, auras, and buffs flow through the modifier pipeline:
  `effective = clamp(base_from_registry + Σ additive) × Π multiplicative`,
  recomputed each tick from attached `Modifier` entries. No system reads a
  base stat directly in combat/movement math — only effective stats.

### 5.6 AI (Data-Driven Decision Trees)
- The bot is an `InputSource` like a human or the network — the sim cannot
  tell the difference.
- Behavior defined in `assets/data/ai/<race>_standard.yaml`:
  an **opening book** (build-order steps with supply timings) followed by a
  **decision tree** (condition → action rules over fog-legal observations).
- The bot reads ONLY: own state + the same fog-filtered enemy info a player
  sees. Map-hack reads are forbidden and reviewed for.
- Bot acts at a YAML-set command cadence (default: one decision batch per 8
  ticks) with an APM cap per difficulty.

### 5.7 Presentation
- 30Hz sim → interpolated rendering at monitor rate; presentation holds
  state T-1 and T, lerps with f64 alpha. Never draw raw fixed positions.
- SDF instanced rendering for units/buildings/projectiles/shape-UI; textured
  quads permitted for terrain layers and text (glyph atlas). One instanced
  draw call per shape class.
- Hardware cursor, always. Frame-0 feedback (§5.1). Input: event-driven,
  timestamped, order-preserving, fully drained each tick; never drop an event.
- Audio consumes SimEventBuffer with spatial throttling (≤3 concurrent
  same-class sounds per area, pitch-varied by a presentation-only RNG).

---

## 6. THINGS YOU MUST NEVER DO
1. Import a physics engine, ECS framework, or game engine crate into sim code.
2. Use floats, wall-clock time, HashMap, or OS entropy in sim crates.
3. Regress `Fixed` to 32-bit storage or skip i128 widening in mul/div.
4. Add a dependency to sim crates without a written, approved ADR.
5. Hardcode any gameplay number in Rust.
6. Let presentation write simulation state, or the bot read through fog.
7. Reorder the system pipeline, or auto-schedule systems.
8. Implement any command that bypasses the CommandLog.
9. Resolve ties or iteration order by anything other than handle index.
10. Trust version numbers in docs — check crates.io at Phase 0; the committed
    Cargo.lock is the source of truth thereafter.
