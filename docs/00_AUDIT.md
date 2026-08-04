# 00_AUDIT.md — Point-by-Point Audit of Prior Documentation
# Project: (working title) TRILATERAL — A Deterministic Competitive RTS
# Audit Date: 2026-08-03

> Scope: audits BOTH the original LLM-produced architecture document AND the
> first-generation doc suite (CLAUDE.md v1, PRD v1, TECH_SPEC v1, etc.).
> Every point receives a verdict: **KEEP**, **REVISE**, **REPLACE**, or **DROP**,
> with rationale. The v2 suite is regenerated from these verdicts.

---

## Verdict Summary

| # | Item | Verdict | Severity if Unfixed |
|---|------|---------|---------------------|
| 1 | Deterministic lockstep + input-only networking | KEEP | — |
| 2 | Fixed-point math mandate (no floats in sim) | KEEP | — |
| 3 | **Q16.16 backed by i32** | **REPLACE** | **Critical — overflow bug** |
| 4 | Rollback (GGPO-style) netcode as primary | REVISE → lockstep-first | High — schedule killer |
| 5 | Bevy ECS recommendation (original doc) | REPLACE (already done in v1) | High |
| 6 | hecs ECS recommendation (v1 suite) | REVISE → custom SoA store | Medium |
| 7 | 60Hz simulation tick | REVISE → 30Hz baseline | Medium |
| 8 | 47% high-ground miss chance | DROP (make data-toggle) | Medium — design |
| 9 | Zero-allocation hot loop (absolute ban) | REVISE → steady-state ban | Low |
| 10 | grep-based `ban_floats.sh` as the determinism guard | REVISE → CI cross-arch hash is the real guard | High |
| 11 | Flow fields mandatory / A* "strictly forbidden" for groups | REVISE → hybrid policy | Medium |
| 12 | "1000Hz raw input polling" | REVISE → event-driven + timestamps | Low |
| 13 | SDF-only rendering | REVISE → SDF-primary hybrid | Low |
| 14 | Sim/render decoupling + interpolation | KEEP | — |
| 15 | Client-side "Frame 0" prediction feedback | KEEP | — |
| 16 | Hardware cursor mandate | KEEP | — |
| 17 | Tick-staggered target acquisition | KEEP (fix stagger key) | Low |
| 18 | Overkill prevention via IncomingDamage | KEEP | — |
| 19 | Attack FSM (frontswing/damage point/backswing) | KEEP | — |
| 20 | Worker phasing + resource slot mutex | KEEP | — |
| 21 | Group anchor / magic-box movement | KEEP | — |
| 22 | SimEventBuffer + audio spatial throttling | KEEP | — |
| 23 | Bitmask fog of war (no raycasting) | KEEP | — |
| 24 | Data-driven YAML stats + hot reload | KEEP | — |
| 25 | Modifier stack pipeline (auras/buffs/tech) | KEEP (promote to MVP) | — |
| 26 | CLAUDE.md governance / SOP / Ledger / Session Log | KEEP | — |
| 27 | Ubiquitous Language component dictionary | KEEP (extend) | — |
| 28 | Phased implementation plan with exit criteria | KEEP (rescope) | — |
| 29 | `supply: 0.5` in units.yaml (v1) | **REPLACE** | **Bug — fractional supply** |
| 30 | Single resource economy (v1 MVP) | REPLACE → 2 resources | High — new requirement |
| 31 | No tech trees / upgrade paths (v1) | ADD | High — new requirement |
| 32 | No per-race building trees (v1) | ADD | High — new requirement |
| 33 | No AI decision trees (v1 bot was ad-hoc FSM) | ADD | High — new requirement |
| 34 | Geometric factions ("Flatland" circles/squares/triangles) | REVISE → keep visual language, deepen into full races | Medium |
| 35 | No map format spec | ADD | Medium |
| 36 | No CI pipeline definition (mentioned, never specified) | ADD | High |
| 37 | No benchmark harness | ADD | Medium |
| 38 | No market positioning / differentiation doc | ADD | High — new requirement |
| 39 | Dependency version pins (wgpu 24, etc.) | REVISE → refresh + lockfile policy | Low |
| 40 | MVP concept (one faction vs scripted AI first) | KEEP (rescope to 3-race prototype path) | — |

---

## Detailed Findings

### 1–2. Determinism foundation — KEEP
Input-only networking over a bit-identical simulation remains the only sane
architecture for an RTS at this entity scale, and fixed-point is still the most
robust way to get cross-platform bit-identity. (Deterministic IEEE-754 float is
*possible* in 2026 with strict compiler flags and no libm, but it is fragile
under LTO/auto-vectorization and impossible to lint for. Fixed-point is
lintable, testable, and boring. Boring wins.)

### 3. Q16.16 on i32 — REPLACE (critical bug)
The v1 spec specifies Q16.16 stored in `i32` (integer range ±32,768). This
**overflows immediately** in routine RTS math:
- `distance_squared` between points 300 tiles apart = 90,000 → overflow.
- Any dot product, length-squared, or area computation at map scale overflows.
- The v1 spec's own `FixedVec2::length_squared()` is unusable on its own map.

**v2 decision:** `Fixed` is **i64-backed Q32.32**. Integer range ±2.1 billion,
fractional precision ~2.3e-10. Multiplication widens through i128 (Rust has
native i128; this is cheap on x86-64/ARM64). Memory cost is 8 bytes per scalar
— irrelevant at our entity counts. All squared-distance math now fits with
enormous headroom. A `Fixed32` (Q16.16) type MAY exist for compact storage
(e.g., per-tile costs) but never for arithmetic chains.

### 4. Rollback netcode — REVISE to lockstep-first
The original doc claims the sim is "so lightweight it could run 1,000 ticks/sec"
and therefore rollback is affordable. This contradicts its own 4,000-entity,
~11ms-per-tick budget elsewhere in the same document. Rolling back 8 frames
means re-simulating 8 full ticks inside one frame budget — ~88ms of sim work in
a 16ms window at the stated scale. Fighting games roll back 2 characters;
we have 4,000 entities.

**v2 decision:** **Deterministic lockstep with adaptive input delay (2–4 ticks)**
is the primary netcode, exactly as Brood War/SC2/AoE shipped. At 30Hz, 3 ticks
of input delay = 100ms command latency, fully masked by Frame-0 client-side
feedback (the acknowledgment bark, waypoint marker, and visual turn happen
instantly — this is what players actually perceive). We still build state
snapshotting on day one (needed for replay seeking, reconnect catch-up, and
save/load), which leaves the door open to bounded rollback later as an
optimization — but it is not on the critical path.

### 5–6. ECS choice — REPLACE Bevy (v0), REVISE hecs (v1) → custom SoA store
Bevy ECS: correctly rejected in v1 (non-deterministic parallel scheduler,
plugin runtime coupling). hecs: better, but archetype ECS makes the two things
we care about most — **whole-state snapshot** and **whole-state hashing** —
awkward, because entity memory layout depends on component insertion history.

**v2 decision:** a **custom Structure-of-Arrays SimState**: fixed-capacity typed
arrays per component, generational entity handles, explicit free lists. This is
~2 sessions of Claude Code work and buys us:
- `snapshot()` = a flat memcpy. `hash()` = hashing contiguous buffers in a
  fixed order. `serialize()` = trivial and versionable.
- Zero hidden iteration-order hazards.
- It is the single best architecture for agentic development: every "system"
  is a plain function over plain arrays with no framework magic to hallucinate
  against.
We lose ECS ergonomics (dynamic component add/remove); an RTS doesn't need
them — unit archetypes are closed and known at load time from YAML.

### 7. Simulation tick rate — REVISE to 30Hz baseline
Brood War runs ~24Hz, SC2 ~22.4Hz; both feel razor-sharp because *feel* comes
from input feedback and render interpolation, not sim frequency. 60Hz doubles
every CPU budget and halves our headroom for the 3-race prototype.
**v2:** 30Hz locked baseline, tick-rate-agnostic code (all durations in ticks,
loaded from YAML), 60Hz as a post-prototype experiment behind a config value.

### 8. 47% high-ground miss chance — DROP as default
This is Brood War nostalgia imported uncritically. Modern competitive design
(and SC2's explicit removal of it) favors **deterministic** advantages:
high ground grants vision denial (can't see/target uphill without a spotter)
plus a flat range or damage-taken modifier. Combat RNG creates outcome variance
that streams badly and frustrates ladder play.
**v2:** no RNG in combat resolution at all. `SimRng` exists for map-gen and
cosmetic-only choices. High-ground rules are data-driven, so a "BW-classic"
mode with miss chance remains a YAML toggle for custom games.

### 9. Zero-allocation hot loop — REVISE
An absolute ban on allocation inside `sim_tick()` is unenforceable dogma that
Claude Code will violate invisibly (any `SmallVec` spill allocates).
**v2:** the rule becomes **no steady-state allocation**: pre-size everything
from YAML-declared maxima; a test harness wraps the global allocator with a
counter and asserts zero allocations across ticks 1,000–2,000 of the benchmark
match. Enforced by test, not by vibes.

### 10. ban_floats.sh as the guard — REVISE
The grep linter stays as a fast first line, but grep cannot catch a `HashMap`
smuggled in via a dependency, or float behavior inside a crate we import.
**v2 real guards:**
1. `sim_core`/`sim_systems` compile with a **minimal, audited dependency set**
   (serde + smallvec only) — enforced by `cargo deny` in CI.
2. **Cross-architecture determinism CI**: the headless arena runs on
   x86-64 Linux, ARM64 (GitHub's ARM runners), and Windows; state hashes must
   match across all three. This catches float leakage *behaviorally* — the only
   test that actually matters.

### 11. Pathfinding mandates — REVISE
"A* strictly forbidden for groups" is overreach. A flow field costs a full-map
Dijkstra per distinct destination; twenty 3-unit skirmish groups with twenty
destinations is worse than twenty hierarchical A* calls with shared-path
following. **v2 policy:** HPA* single path + formation-offset following for
groups ≤ 12; integration flow fields for groups > 12 or repeated destinations
(cached); RVO-style local steering with mass priority underneath both. The
threshold lives in YAML.

### 12. "1000Hz raw input polling" — REVISE
Modern OS input is event-driven; winit delivers every event with no polling
loop needed. The actual requirements: never drop an event, preserve order,
timestamp on arrival, drain fully each sim tick. Restated that way in v2.

### 13. SDF-only rendering — REVISE to SDF-primary
SDF instanced shapes remain the core aesthetic (crisp at any zoom, tiny asset
footprint, cheap state-driven effects). But mandating "everything is SDF"
complicates text, terrain texture variety, and decals. **v2:** SDF instancing
for units/buildings/projectiles/UI-shapes; standard textured quads permitted
for terrain layers and glyph atlases (glyphon/cosmic-text for text).

### 14–28. The load-bearing good ideas — KEEP
Sim/render decoupling with interpolation; Frame-0 prediction feedback; hardware
cursor; staggered target acquisition (fix: stagger key must be the stable
entity **handle index**, not a respawn-reusable id); IncomingDamage overkill
prevention; the three-phase attack FSM enabling stutter-step; worker phasing
and harvest slot reservation; anchor-based group movement; the deaf simulation
with SimEventBuffer and audio throttling; bitmask fog; YAML-everything with hot
reload; modifier stacks (now REQUIRED for MVP since tech trees ship in the
prototype); the entire governance apparatus (CLAUDE.md, SOP, Ledger, Session
Log, phase gates). These are the parts of v1 that were genuinely right.

### 29. Fractional supply — REPLACE
`supply: 0.5` cannot exist in an integer simulation. **v2:** supply is stored
×2 internally (a zergling-alike costs `supply_x2: 1`, a worker `supply_x2: 2`),
displayed ÷2. Same trick Brood War uses.

### 30–33. The new scope requirements — ADD
- **Two resources** (v2: **Ore** — abundant, saturating, worker-scaling; and
  **Flux** — scarce, geyser-style, gates tech). The proven mineral/gas dynamic:
  Ore measures macro mechanics, Flux forces expansion timing and tech
  commitment decisions.
- **Per-race tech trees** — templated in `assets/data/tech_tree.yaml`, three
  tiers, prerequisite graph, all data-driven through the modifier pipeline.
- **Per-race building trees** — production structures, tech structures,
  defensive structures, race-mechanic structures; templated in YAML.
- **Per-race AI decision trees** — a data-driven behavior tree format in
  `assets/data/ai/*.yaml`: build orders as opening books, then condition→action
  rules over scouted state. The bot is an `InputSource` reading only fog-legal
  information.

### 34. "Flatland" geometric factions — REVISE, don't discard
The geometry-as-readability visual language is a genuine asset (instant
silhouette parsing, trivial art pipeline, distinctive look). But shape alone is
not a race. **v2** keeps circles/squares/triangles as the visual identity of
three fully asymmetric races with distinct **economy, production, and defense
mechanics** — the Brood War lesson is that races differ in *how they macro*,
not just in unit stats. See GAME_DESIGN.md.

### 35–37. Missing infrastructure — ADD
Map format spec (RON tile grid + entity placements + symmetry validation tool);
GitHub Actions CI (build, test, clippy, cargo-deny, ban_floats, cross-arch
desync arena, benchmark regression); criterion benchmark harness with
per-system budgets asserted in CI.

### 38. Market positioning — ADD
The 2026 landscape is unforgiving: Stormgate's launch collapse and ZeroSpace's
rough Early Access show that "SC2-but-new" with conventional tech is not a
product. Our structural differentiators must come from what the deterministic
headless architecture makes uniquely cheap. See MARKET_POSITION.md — the short
version: perfect seekable replays, a first-class bot/training API, ghost-replay
practice tools, community-forkable balance, and a sub-100MB game that runs on a
laptop from 2015.

### 39. Dependency pins — REVISE
Version numbers in docs rot. **v2 policy:** `Cargo.lock` is committed and is
the source of truth; docs name crates, not versions; a quarterly
`cargo update` + full CI pass is a scheduled maintenance task. Claude Code is
instructed to check crates.io for current versions at Phase 0 rather than
trusting numbers in any document.

### 40. MVP strategy — KEEP, rescoped
v1's "one faction vs scripted AI first" instinct was right. v2's prototype
ladder: **P1** = one race, full economy (2 resources), tech tier 1, vs bot →
**P2** = all three races, full trees, mirror + non-mirror matchups vs bot →
**P3** = LAN/online lockstep 1v1. "Competitive in the current market" is a
direction, not a v0 exit criterion; the prototype's exit criterion is *"a
Brood War player can play a real macro game and it feels sharp."*
