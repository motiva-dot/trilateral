# IMPLEMENTATION_PLAN.md — Phased Build Plan v2
# Project: TRILATERAL

> One phase = 1–3 Claude Code sessions. No phase starts until the previous
> phase's exit gate is green. Every phase ends with the determinism test
> passing — determinism is not a phase, it is a permanent condition.

---

## Phase 0 — Scaffold & Truth Infrastructure (1 session)
- Cargo workspace, all crate stubs per TECH_SPEC §1 graph.
- **Check crates.io for current versions of all dependencies** (do not trust
  any version number found in documents); commit Cargo.lock.
- `scripts/ban_floats.sh`, `.github/workflows/ci.yml`, `cargo-deny` config.
- `docs/` populated (this suite), Ledger + Session Log initialized.
- `tools/headless_sim` stub that ticks an empty SimState and prints a hash.
**Gate:** workspace builds on CI (3 platforms); empty-state hash identical
across platforms; ban_floats green.

## Phase 1 — trilateral_fixed (1–2 sessions)
- Q32.32 `Fixed` with i128 widening; `FixedVec2`; `FixedAngle` + 4096-entry
  tables + CORDIC atan2; bit-exact sqrt; serde as i64.
- 250+ tests incl. the seeded 10k-op fuzz with committed golden hash.
**Gate:** all tests green on 3 platforms; golden fuzz hash identical
cross-platform (first real determinism proof).

## Phase 2 — SoA SimState + Registries (2 sessions)
- EntityAllocator (generational), Components arrays, SimClock, SimRng,
  MacroGrid, snapshot(), hash(), zero-on-free.
- `sim_content`: YAML → UnitRegistry / BuildingRegistry / TechRegistry /
  engine.yaml capacities; float_bridge quarantined here; hot reload (dev).
- Command enum + validation-at-ingest skeleton + CommandLog.
**Gate:** spawn 500 mixed archetypes from YAML; snapshot/restore byte-equal;
hash stable across runs & platforms; YAML round-trip tests green.

## Phase 3 — Loop, Movement, Spatial (1–2 sessions)
- `sim_systems::tick()` full ordered pipeline (stubs where needed).
- SpatialHash + queries (index-sorted results); movement integration;
  command_ingest/execution for Move/Stop; fixed-step accumulator (app side).
**Gate:** 60-tick straight-line motion asserts exact fixed values; two
identical runs hash-equal; 1,200 movers under budget in bench harness.

## Phase 4 — Pathfinding & Steering (2–3 sessions)
- Tile A* → HPA* chunks/portals; integration flow fields + LRU cache +
  chunk-dirty invalidation; policy switch at flow_field_threshold.
- Velocity-obstacle steering in fixed point; mass yielding; settle rule;
  group anchor offsets; circle collision (Murmur first).
**Gate:** 50-scenario maze suite; 150-unit convergence without jitter;
building placement re-routes; bench: steering ≤2.5ms, pathing ≤1.5ms @1,200;
determinism green.

## Phase 5 — Combat (2 sessions)
- Staggered target acquisition + priority heuristic + deterministic ties;
  incoming-damage overkill prevention; 3-phase FSM with cancels; facing
  gates + turn rates; projectiles; deaths; **no RNG anywhere**.
- High-ground: uphill vision denial + damage-taken modifier (YAML).
**Gate:** stutter-step test (move-cancel in backswing preserves DPS window);
overkill retarget test; 200v200 for 2,000 ticks deterministic on 3 platforms.

## Phase 6 — Two-Resource Economy & Production (2 sessions)
- Ore nodes + slot reservation; Flux fissures + extractor requirement;
  worker phasing; cargo/deposit; player stockpiles; supply_x2.
- Building placement validation (incl. race predicates: Bloom / power /
  anywhere) → MacroGrid occupancy → path invalidation; construction rules
  per race (Bastion worker-occupied builds; Murmur/Concord morph-style);
  Train/Research/Cancel through queues; rally points.
**Gate:** full macro loop headless: 12 workers saturate a base, expansion
completes, army trains; Bastion build-denial test (kill builder mid-build);
determinism green.

## Phase 7 — Tech Trees & Modifier Pipeline (1–2 sessions)
- ModifierStack math (`clamp(base + Σadd) × Πmult`), all combat/movement
  reads switch to effective stats; TechRegistry effects attach modifiers on
  research completion; weapon/armor 3-level tracks; choice-pair prereqs.
**Gate:** +1 attack research changes damage dealt exactly; aura
attach/detach on radius cross; upgrade determinism test.

## Phase 8 — Race Mechanics (2–3 sessions)
- Murmur: Brood Pool points/spend, Bloom spread/denial/placement/speed.
- Bastion: add-ons, depot lower/raise, repair, salvage.
- Concord: lattice power fields, shield regen, facing arcs (SAT + front-arc
  damage/armor), warp resonance with cost premium.
- AABB and triangle collision + cross-pairs land here.
**Gate:** per-mechanic headless tests (Bloom denial shrinks build space;
unpowered Bastile stops producing; salvage refunds and unblocks path;
front-arc bonus exact); full 3-race determinism arena becomes THE CI arena.

## Phase 9 — Presentation v1 (2–3 sessions)
- wgpu instanced SDF pipelines; interpolation; camera; selection UI;
  health/shield bars; minimap; fog rendering with ghosts; terrain layer;
  text; hardware cursor; Frame-0 feedback; debug overlays; audio v0 with
  throttling.
**Gate:** play Murmur-mirror by hand; 1,200 entities at monitor rate on
min-spec target; visual review checklist with the human architect.

## Phase 10 — Bots from Decision Trees (2 sessions)
- `InputSource` trait finalized (human/net/bot/replay all conform).
- AI YAML: opening book executor, rule evaluator, fog-legal observation
  layer, APM cap, micro levels 0–2; one standard book per race.
**Gate:** bot completes a full macro game each race; bot-vs-bot 3-race
round-robin runs headless and deterministic (this becomes a CI job);
map-hack review: grep + targeted tests prove fog-legality.

## Phase 11 — Replays & Ghost Trainer v0 (1–2 sessions)
- Replay writer/reader + zstd; keyframe index; seek both directions; speed
  controls; watch-as-player views; economy-ghost HUD mode.
**Gate:** record→replay hash-equal; seek-to-tick equals play-to-tick; ghost
mode runs a recorded stream against live input.

## Phase 12 — Lockstep Online (2–3 sessions)
- quinn session, handshake hashes, adaptive input delay, pause/stall UI,
  hash exchange, desync auto-capture, relay fallback, reconnect catch-up.
**Gate:** P3 exit test from PRD §5 — 20 clean cross-platform games, blind
latency test passes, forced-desync drill produces a usable trace.

## Phase 13 — Prototype Polish (1–2 sessions)
- Victory/defeat flow, main menu (Play Bot / Online / Replays / Settings),
  hotkey remap, settings persistence, second map, perf pass, README.
**Gate:** the PRD P1/P2/P3 exit tests all pass; a fresh clone builds and
plays from README alone.

---

## Standing Rules
- Any phase may add YAML knobs; no phase may add hardcoded numbers.
- The determinism CI arena grows with content (P5: combat arena → P8:
  3-race arena → P10: bot round-robin) and is never allowed to shrink.
- If a phase reveals a spec error, update 00_AUDIT.md-style: verdict,
  rationale, then the spec — never silently diverge code from docs.
