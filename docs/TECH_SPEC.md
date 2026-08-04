# TECH_SPEC.md — Technical Specification v2
# Project: TRILATERAL

> Engineering authority. Incorporates every 00_AUDIT.md verdict.
> Where CLAUDE.md states a law, this doc states the mechanism.

---

## 1. Stack

- **Rust**, current stable, edition 2024. `Cargo.lock` committed = version
  truth; docs never pin versions (audit item 39). At Phase 0, Claude Code
  checks crates.io for current releases of: `wgpu`, `winit`, `bytemuck`,
  `serde`, `serde_yaml` (or `serde_yml` successor), `smallvec`, `kira`,
  `quinn`, `criterion`, `glyphon`.
- **Crate graph** (each arrow = allowed dependency, nothing else):
```
trilateral_fixed  →  (serde only)
sim_core          →  trilateral_fixed, serde, smallvec
sim_systems       →  sim_core
sim_content       →  sim_core (YAML loaders → registries; float_bridge here)
bot               →  sim_core, sim_systems (InputSource impl)
netcode           →  sim_core (lockstep session, quinn)
replay            →  sim_core (log format, seek index)
presentation      →  sim_core READ-ONLY, wgpu, winit, kira, glyphon
trilateral_app    →  everything (wiring only)
tools             →  bins: headless_sim, desync_trace, map_check, bench_match
```
`cargo deny` enforces the sim-crate allowlist in CI.

---

## 2. Fixed-Point Math (`trilateral_fixed`) — Q32.32 on i64

Replaces v1's Q16.16/i32 (overflow bug — audit item 3).

- `Fixed(i64)`: 32 integer bits (±2.1e9), 32 fractional (~2.3e-10 step).
- `mul`: `((a as i128 * b as i128) >> 32) as i64`. `div`: `((a as i128) << 32) / b`.
  Debug builds assert no i128→i64 truncation; release wraps (and the
  determinism arena would catch any divergence anyway).
- `sqrt`: bit-by-bit integer method (exact, no iteration-count ambiguity).
- `FixedAngle(u32)`: full turn = 2³². Natural wrapping. `sin/cos` via a
  4096-entry quarter-wave table + symmetry + linear interpolation between
  entries (all integer). `atan2` via 32-iteration integer CORDIC.
- `FixedVec2 { x, y }`: dot, cross_z, length_sq (i128-safe), manhattan,
  normalize (returns zero on zero), rotate (via tables), clamp_length.
- `from_f64/to_f64` behind `feature = "float_bridge"` only.
- Serde as raw i64 (never as float text).
- Test bar: 250+ cases — algebraic identities, boundary values, table-edge
  angles, quadrant sweeps, i128 widening proofs, serde round-trip, and a
  10k-op seeded fuzz sequence with a golden output hash committed to the repo.

---

## 3. SimState — Custom SoA Store (audit items 5–6)

As specified in CLAUDE.md §3. Mechanism notes:

- `EntityHandle { index: u32, generation: u32 }`; `EntityAllocator` with an
  intrusive free list; generation bump on free; all stale-handle derefs return
  None (never panic in release).
- Capacities from `assets/data/engine.yaml`: `max_entities` (prototype 2048),
  `max_projectiles` (4096), `max_commands_per_tick` (256/player), queue and
  stack caps. Loaded once at match start; arrays boxed and pre-touched.
- `snapshot()`: `Box<SimStateSnapshot>` copy, target < 1 ms at prototype
  scale; used by replay keyframes (every 600 ticks), reconnect, and save.
- `hash()`: fold arrays in the declared field order with a fixed-seed 64-bit
  hasher (vendored, no HashDoS randomness). Only `alive` slots are folded;
  freed-slot garbage is excluded by construction (slots zeroed on free).
- Systems are `fn(&mut SimState, &Registries)`; the pipeline is a hand-written
  ordered call sequence in `sim_systems::tick()`. No scheduler.

---

## 4. Spatial & Pathfinding

- **SpatialHash**: uniform grid, cell = 2× max collider radius; rebuilt
  incrementally in `movement`; queries return handles sorted by index
  (determinism).
- **MacroGrid**: per tile — walkable, buildable, elevation(0-2), bloom bit,
  power bit(s), occupancy handle.
- **HPA\***: 16×16 chunks, portal graph, rebuilt per-chunk on grid change.
- **Flow fields**: integration Dijkstra from destination; cache keyed by
  destination tile, LRU 32 fields, invalidated per dirty chunk.
- **Policy** (audit item 11): groups ≤ `flow_field_threshold` (YAML, 12) use
  one shared HPA* path + anchor-offset following; larger groups use flow
  fields. Steering beneath both: velocity-obstacle local avoidance in fixed
  point, mass-priority yielding, harvest phasing, settle rule.
- **Collision**: circle–circle (r² sums), AABB–AABB (min-penetration axis,
  no slide), triangle SAT (via fixed trig), plus the three cross pairs.
  Buildings resolve as static grid occupancy, not colliders.

---

## 5. Simulation Loop & Time

```
accumulator += real_frame_time            // presentation-side f64, never enters sim
while accumulator >= TICK_DT { sim_tick(); accumulator -= TICK_DT }
alpha = accumulator / TICK_DT             // render interpolation factor
```
- `TICK_DT` = 1/30 s. All gameplay durations are integer ticks in YAML.
- Headless mode (tools/headless_sim): the same `sim_tick` in a tight loop —
  benchmarking, bot training, replay seeking, reconnect catch-up.
- Victory: elimination (all base structures dead) or surrender command.

---

## 6. Netcode — Deterministic Lockstep (audit item 4)

- Session over QUIC (quinn); direct P2P with a dumb relay fallback.
- Handshake: engine build hash, content hash (YAML tree), map hash, seed,
  player slots. Any hash mismatch aborts the lobby (this is also how balance
  mods are kept off ladder without banning them elsewhere).
- Each tick T, each client sends its command batch stamped for execution at
  T + input_delay (adaptive 2–4 based on RTT jitter percentile). Sim advances
  only when all batches for T are present; otherwise pause + "waiting" UI.
- Empty batches are still sent (heartbeat + progress).
- Hash exchange every 60 ticks; mismatch ⇒ match void, both logs auto-saved,
  desync_trace tool ingests both.
- Reconnect: rejoin lobby → receive command log tail → headless catch-up at
  max speed → resume. (Snapshots make mid-log start possible: nearest
  keyframe + delta log.)
- Rollback: explicitly out of prototype scope; snapshot infrastructure keeps
  the option open.

---

## 7. Replay & Ghost Trainer

- File: header (build/content/map hashes, seed, players, tick rate) + body
  (tick, player, command)*, bincode, zstd-compressed. Target: full 20-min
  game < 200 KB.
- Keyframes: every 600 ticks a snapshot offset index appended at file end ⇒
  O(1) seek to any point, backward seeking included — a headline feature.
- Playback speeds ×¼ … ×8 and MAX (headless).
- **Ghost trainer**: load a replay, take player X's command stream as a bot
  `InputSource`, play against it live. Deterministic engine ⇒ the ghost
  executes its historical game exactly (until interaction diverges outcomes —
  which is the training signal). v0: economy-ghost (mirror your own macro
  side-by-side, HUD deltas for worker count / supply / spend).

---

## 8. Presentation

- wgpu; one instanced pipeline per shape class (circle/AABB/triangle/ring/
  bar); instance = `{pos, facing, scale, shape, color, flags}` packed via
  bytemuck; terrain as textured/tiled quad layer; text via glyph atlas
  (glyphon). SDF fragment shaders give crisp edges + state effects
  (selection ring, shield shimmer, damage flash) at zero CPU cost.
- Interpolation: hold T-1/T component copies (pos, facing, hp for bars),
  lerp with f64 alpha; angles lerp shortest-arc.
- Fog: per-player bitmask → texture → soft-edge blur shader; explored-but-
  not-visible shows terrain + last-seen building ghosts (stored in
  presentation RenderState, not sim).
- Camera: pan (keys/edge/MMB-drag), zoom steps, minimap click/drag; F-key
  debug overlays (grid, flow vectors, colliders, aggro lines, path nodes).
- Input: event-driven winit, timestamped, order-preserving, drained fully per
  tick (audit item 12); selection semantics (click/drag/double/ctrl-groups/
  tab-subselect by selection_weight); Frame-0 feedback; hardware cursor.
- Audio: kira; SimEventBuffer consumer; spatial throttle ≤3 same-class per
  area per 100 ms window, presentation-RNG pitch variance.

---

## 9. CI & Quality Infrastructure (audit items 10, 36, 37)

`.github/workflows/ci.yml` jobs, all required to merge:
1. **build+test** on ubuntu-x86_64, ubuntu-arm64, windows-x86_64.
2. **clippy** `-D warnings`; **fmt** check.
3. **ban_floats** grep linter (fast first line).
4. **cargo-deny**: sim-crate dependency allowlist + advisories.
5. **determinism arena**: headless 600-unit mixed-race scripted match,
   10,000 ticks, hashes at 1k-tick checkpoints — compared ACROSS the three
   platforms' artifacts. The real float/entropy guard.
6. **allocation gate**: benchmark match ticks 1,000–2,000 with counting
   allocator ⇒ zero allocations.
7. **bench regression**: criterion per-system timings vs committed baselines;
   >15% regression fails.
8. **map_check** on all map files (symmetry, resource parity, connectivity).

Per-tick budgets @1,200 entities, 30Hz (asserted by job 7):
steering 2.5ms · pathfinding 1.5ms · target-acq 1.0ms · combat+damage 1.0ms ·
movement+spatial 0.8ms · economy+production 0.4ms · modifiers 0.4ms ·
fog 0.6ms · everything else 0.8ms ⇒ ~9ms ceiling, ≤8ms target, 33ms wall.

---

## 10. Determinism Debugging

`tools/desync_trace`: given two command logs (or one log run twice), bisects
to the first divergent hash checkpoint, re-runs to that tick, dumps full
per-entity component JSON from both, diffs, and reports
`(tick, entity, component, values, last-writing system)` — the last-writer is
identified by re-running the tick with per-system hash sampling. No guessing.
