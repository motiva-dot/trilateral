# IMPLEMENTATION_PLAN.md — Phased Build Plan v3
# Project: TRILATERAL

> One phase = 1–3 Claude Code sessions. No phase starts until the previous
> phase's exit gate is green. Every phase ends with the determinism test
> passing — determinism is not a phase, it is a permanent condition.

---

## REVISION v2 → v3 (2026-08-04) — Build Order, Not Phase Content

Recorded audit-style per Standing Rules: verdict, rationale, then the spec.
**Phase numbers are unchanged and phase content is almost unchanged.** What
changes is the *order they are built in*, plus one new phase. Every existing
cross-reference (CLAUDE.md, TECH_SPEC §9, Ledger ADR-001) stays valid.

| # | Item | Verdict | Rationale |
|---|------|---------|-----------|
| A | All visual feedback deferred to Phase 9 | **REVISE** | Critical for this project's stated goal |
| B | Combat (P5) before Economy (P6) | **REVISE → swap** | The target slice *is* the economy |
| C | Murmur Brood Pool sitting in Phase 8 | **REVISE → pull into P6** | Without it there is no distinctive macro to judge |
| D | Phases 7, 8 (rest), 10–13 before a playable slice | **DEFER** | Not required by the slice |

### A — Visual feedback at Phase 9 (the important one)

The architect's contribution to this project is *feel*; the engine work is
delegated. v2's ordering means nothing is visible until roughly session 12–16
— so the one person qualified to judge the one thing that differentiates this
game is asked to judge it only after every decision determining it has been
made. That is a description of how Stormgate arrived where it arrived: not bad
engineers, but a feedback loop on feel that closed too late to act on. It is
the specific failure this project exists to avoid, and v2 reproduced it in the
schedule.

**v3:** a throwaway debug view lands right after Phase 3 (new **Phase 3.5**),
so every phase from movement onward ends in something watchable and playable.

This costs nothing architecturally. CLAUDE.md §1.6 already requires
presentation to be read-only over `SimEventBuffer`; building it at Phase 3
means that law is enforced from the start rather than retrofitted at Phase 9,
which is the usual way engines lose their decoupling.

### B — Economy before combat

"The first couple of minutes" is worker split, harvest rhythm, first building,
first units. Fighting is minute four or five. PRD design pillar #1 is "Macro
is the game" — so the earliest thing that becomes judgeable should be the
thing the design says matters most.

### C — Brood Pool pulled forward from Phase 8 into Phase 6

Evaluating macro feel against generic parallel production means judging a
generic RTS, not this one. The Brood Pool *is* Murmur's macro skill expression
(never float points; choose what the swarm becomes at the moment of spending).
Bloom, add-ons, lattice and the rest stay in Phase 8.

---

## BUILD ORDER (v3)

```
  0 ✅  Scaffold & truth infrastructure          DONE 2026-08-04
  1     trilateral_fixed
  2     SoA SimState + registries
  3     Loop, movement, spatial
  3.5   Debug view          ← NEW. first time anything is visible
  4     Pathfinding & steering                   ← FEEL CHECKPOINT 1
  6     Economy & production + Murmur Brood Pool ← FEEL CHECKPOINT 2
  5     Combat                                   ← FEEL CHECKPOINT 3
  9L    Presentation pass (slice-grade)          ← VERTICAL SLICE COMPLETE
  ─────────────────────────────────────────────────────────────────
  then, from strength: 7 (tech trees), 8 (remaining race mechanics),
  9 (full presentation), 10 (bots), 11 (replays), 12 (lockstep), 13 (polish)
```

### THE VERTICAL SLICE — exit test for the whole reordering

> Load a map, split workers, harvest Ore and Flux, build a building, train
> units from the Brood Pool, move them across the map, and fight — and have a
> Brood War player call the movement and response **sharp**.

That is the milestone the build order above exists to reach. It supersedes
nothing in PRD §5 (P1 remains the fuller definition); it is the earlier point
at which feel becomes judgeable at all.

**Standing rule added in v3:** every phase from 3.5 onward ends with a play
session, not just a green gate. A phase whose gate is green but that has not
been *watched* is not finished. Feel debt compounds like tech debt — and
unlike tech debt, CI cannot see it.

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

## Phase 3.5 — Debug View (1–2 sessions)  ← NEW IN v3
The throwaway renderer that makes every later phase judgeable. It is allowed
to be ugly; it is not allowed to be wrong about decoupling.
- `presentation`: wgpu + winit window, one instanced pipeline, flat-coloured
  circles/quads. No SDF, no art, no HUD chrome — those stay in Phase 9.
- Camera: pan (keys + edge + MMB-drag), zoom steps. Hardware cursor.
- Interpolation from day one: hold T-1/T position copies, lerp with f64 alpha
  (TECH_SPEC §8). Never draw raw fixed positions — retrofitting this is how
  motion ends up looking stepped.
- Click-select + drag-box select, selection ring, and **Frame-0 feedback**
  (marker appears on click, before the sim executes anything).
- `SimEventBuffer` drained once per tick even though almost nothing consumes
  it yet — the contract is established now, not later.
- F-key debug overlays: spatial-hash grid, collider outlines, path nodes.
**Gate:** 500 units visible moving at monitor rate; presentation contains
zero writes to `SimState` (reviewed, not assumed); `cargo test` and the
determinism arena unaffected — the sim must not know the window exists.
**Play session:** watch 500 units move. This is the first feel data point.

## Phase 4 — Pathfinding & Steering (2–3 sessions)
- Tile A* → HPA* chunks/portals; integration flow fields + LRU cache +
  chunk-dirty invalidation; policy switch at flow_field_threshold.
- Velocity-obstacle steering in fixed point; mass yielding; settle rule;
  group anchor offsets; circle collision (Murmur first).
**Gate:** 50-scenario maze suite; 150-unit convergence without jitter;
building placement re-routes; bench: steering ≤2.5ms, pathing ≤1.5ms @1,200;
determinism green.
**Play session (FEEL CHECKPOINT 1) — the StarCraft-standard bar.** Green
tests are necessary and nowhere near sufficient here; movement is where an
RTS lives or dies. Judged by eye, not by assertion: units shove rather than
jostle and heavier units win (integer `mass` priority); a group ordered
across the map arrives as a group without the tail rubber-banding; no
oscillation when two groups cross; units that arrive settle instead of
vibrating against each other (the settle rule); a unit ordered one tile away
turns and goes rather than pathing around itself. Clumping is *tunable*, not
eliminated — PRD §7 is explicit that some friction creates positional skill.
Any of these failing means the phase is not done regardless of CI.

## Phase 5 — Combat (2 sessions)
- Staggered target acquisition + priority heuristic + deterministic ties;
  incoming-damage overkill prevention; 3-phase FSM with cancels; facing
  gates + turn rates; projectiles; deaths; **no RNG anywhere**.
- High-ground: uphill vision denial + damage-taken modifier (YAML).
**Gate:** stutter-step test (move-cancel in backswing preserves DPS window);
overkill retarget test; 200v200 for 2,000 ticks deterministic on 3 platforms.
**Play session (FEEL CHECKPOINT 3) — responsiveness under fire.** Does a
stutter-step reward the hands that can do it? Does a right-click during
backswing feel instant (Frame-0) while remaining honest to `input_delay`? Do
units acquire targets without dithering between two equidistant enemies (ties
by handle index — must look decisive, not twitchy)? Does a surround feel like
a surround? Combat with no RNG should read as *earned*; if an exchange feels
arbitrary, something is wrong even when the maths is right.

## Phase 6 — Two-Resource Economy & Production (2–3 sessions)
> **v3: built BEFORE Phase 5, and absorbs the Murmur Brood Pool from Phase 8.**
> Added scope: Brood Point generation (1 per 90 ticks per base, stockpile cap
> 3, all from YAML), spend-to-spawn at any base, and the supply_x2 accounting
> around it. Without it there is no distinctive macro loop to have an opinion
> about at Feel Checkpoint 2.

- Ore nodes + slot reservation; Flux fissures + extractor requirement;
  worker phasing; cargo/deposit; player stockpiles; supply_x2.
- Building placement validation (incl. race predicates: Bloom / power /
  anywhere) → MacroGrid occupancy → path invalidation; construction rules
  per race (Bastion worker-occupied builds; Murmur/Concord morph-style);
  Train/Research/Cancel through queues; rally points.
**Gate:** full macro loop headless: 12 workers saturate a base, expansion
completes, army trains; Bastion build-denial test (kill builder mid-build);
Brood Points accrue, cap, and spend deterministically; determinism green.
**Play session (FEEL CHECKPOINT 2) — the macro rhythm.** Does the first
ninety seconds have a *pulse*? Worker split → harvest loop → the first
supply-block moment → the first real "units now or tech later" decision.
Specifically: is the harvest round trip long enough to notice and short
enough to stay busy; does floating Brood Points feel like a mistake you can
see yourself making; is the saturation curve legible without a HUD readout.
These are YAML numbers — expect to tune them here, in play, not in code.

## Phase 7 — Tech Trees & Modifier Pipeline (1–2 sessions)
- ModifierStack math (`clamp(base + Σadd) × Πmult`), all combat/movement
  reads switch to effective stats; TechRegistry effects attach modifiers on
  research completion; weapon/armor 3-level tracks; choice-pair prereqs.
**Gate:** +1 attack research changes damage dealt exactly; aura
attach/detach on radius cross; upgrade determinism test.

## Phase 8 — Race Mechanics (2–3 sessions)
> **v3: Brood Pool points/spend moved out of this phase into Phase 6.**
> What remains here for Murmur is Bloom only.
- Murmur: Bloom spread/denial/placement/speed. (Brood Pool: see Phase 6.)
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

### Phase 9L — Slice-grade presentation pass (1–2 sessions)  ← NEW IN v3
The subset of Phase 9 required to declare the Vertical Slice complete, run
immediately after Phase 5 rather than waiting for full Phase 9. Promotes the
Phase 3.5 debug view to something playable without apology: SDF instancing
for the three shape classes, selection UI and control groups, health bars,
minimap, fog rendering with building ghosts, terrain layer, resource/supply
readout. Everything else in Phase 9 (audio, full debug overlay suite, min-spec
optimisation pass) stays in Phase 9 proper.
**Gate:** THE VERTICAL SLICE exit test at the top of this document.

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
- **A phase whose gate is green but which has not been played is not done**
  (from Phase 3.5 onward). CI cannot see feel debt, and feel debt compounds
  like tech debt. Record each play session's punch list in the Session Log.
- **The StarCraft-standard bar applies to movement, response and combat
  feel** — it is a delegated engineering responsibility, not a matter of
  taste to be deferred to the architect. Where a feel question is genuinely a
  taste call (pathing friction, turn rates, harvest trip length, input delay),
  bring it to him with the YAML knob already exposed and a recommendation.
- Any phase may add YAML knobs; no phase may add hardcoded numbers.
- The determinism CI arena grows with content (P5: combat arena → P8:
  3-race arena → P10: bot round-robin) and is never allowed to shrink.
- If a phase reveals a spec error, update 00_AUDIT.md-style: verdict,
  rationale, then the spec — never silently diverge code from docs.
