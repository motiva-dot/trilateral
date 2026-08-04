# ARCHITECTURE_LEDGER.md — Claude Code's Long-Term Memory

## Current State
- Phases **0, 1, 2, 3, 3.5 COMPLETE**; **Phase 4 gate MET** with two items
  deliberately outstanding (see ADR-012). Build order is v3.
- Next: the 1,200-mover bench — which decides whether HPA* is needed at all —
  then Feel Checkpoint 1, then Phase 6 (economy) per the v3 order.
- Last session: 2026-08-04 | Tests: **304**, green in debug and release
- Determinism arena runs the REAL `sim_systems::tick()` pipeline for 10,000
  ticks with 600 units executing move orders, and agrees byte-for-byte across
  x86-64 Linux, ARM64 Linux and x86-64 Windows.
- CI: **8/8 green on 3 architectures.** Repo is public at
  github.com/motiva-dot/trilateral so the `ubuntu-24.04-arm` legs run free.
- `main` is branch-protected: 5 required checks (`lint`, all three
  `build-test` legs, `determinism-compare`), `enforce_admins: true`,
  linear history required, force-push and deletion disabled. **Direct pushes
  to `main` are impossible for everyone, including the architect — all work
  from Phase 1 on goes through a PR.** The three `determinism` legs are not
  listed individually because `determinism-compare` `needs:` them, so a
  failed leg leaves it never-reported and the merge blocked anyway.
- Host dev machine: Windows 11, MSVC toolchain, Git Bash present (the `.sh`
  guardrail scripts run natively — no `xtask` port needed).

## Crate Status
- **trilateral_fixed: COMPLETE (Phase 1).** `Fixed` (Q32.32/i64), `FixedVec2`,
  `FixedAngle` (u32, full turn = 2^32), `tables.rs` (generated, committed),
  optional `float_bridge` feature. 80 tests, green in debug and release, on
  all three architectures. **Golden hash `0x3373a9ec5352e0b7`** over 10k mixed
  ops, seed 42 — `crates/trilateral_fixed/tests/golden.rs`. Key names:
  `Fixed::{from_ratio, mul, div, sqrt, lerp}`, `FixedVec2::{length_sq_wide,
  normalize, clamp_length, perp, rotate}`, `FixedAngle::{sin, cos, atan2,
  shortest_diff, turn_toward}`.
- **sim_core: COMPLETE for phases 2–4.** `SimState` (capacities, clock,
  `SimRng`, `EntityAllocator`, `Components`, `MacroGrid`, `CommandLog`) with
  `snapshot`/`restore`/`hash`. `SimHasher`, `BitSet`, `Command` + ingest
  validation, `PathSlot`, `registry::{UnitStats, SteeringParams, Registries}`.
  107 tests.
- **sim_systems: Phase 3–4.** `tick()` is the §3.2 pipeline as a literal call
  sequence with unwritten steps named in place. `spatial::SpatialHash`,
  `steering` (separation, mass priority, deadband), `movement` (route
  following, exact arrival, settle rule), `path` (tile A*), `pathing`
  (budgeted route assignment). 58 tests incl. `tests/gate.rs`.
- **sim_content: loaders.** `engine.yaml` → `Capacities`, `units.yaml` →
  `UnitRegistry`, `tech_tree.yaml` → `TechRegistry`, `steering.yaml` →
  `SteeringParams`. All validate rather than trust; all tested against the
  REAL asset files via `include_str!`. 39 tests.
- **presentation: Phase 3.5 debug view.** wgpu 30 + winit 0.30, instanced
  circles and squares, camera, selection, interpolation, Frame-0 feedback,
  ring formation offsets. Read-only over `SimState` by signature. 21 tests.
- **trilateral_app:** wiring + a scaffold map with obstacles.
- **tools:** `headless_sim` (the real arena), `gen_tables` (offline).
- bot · netcode · replay: still stubs.
- bot · netcode · replay · presentation: stubs
- trilateral_app: hello-world · tools: headless_sim placeholder-hash stub

## ADR Log

### ADR-001 — Dormant CI jobs until their code exists
**Date:** 2026-08-03
**Decision:** `allocation-gate`, `bench-regression` and `map-check` are
commented out in `.github/workflows/ci.yml`, each annotated with the phase
that revives it (2, 3, 6 respectively).
**Context:** TECH_SPEC §9 and the ci.yml header declare all eight jobs
required to merge. But those three invoke `tools/tests/alloc_gate.rs`,
`tools/src/bin/bench_match.rs` and `tools/src/bin/map_check.rs`, none of which
exist at Phase 0. On the first push they fail with "no bin target named ...".
The Phase 0 exit gate asks only for: 3-platform build, identical empty-state
hash, ban_floats green — which `build-test`, `lint`, `determinism` and
`determinism-compare` fully cover.
**Alternatives:** (a) stub the three binaries so they exit 0 — rejected, a
green check that asserts nothing is a lie told to your future self, and the
allocation gate in particular would be trusted before it measures anything;
(b) `if: false` — rejected, GitHub's treatment of skipped jobs as satisfying
required checks is a subtlety not worth depending on.
**Consequences:** Branch protection requires five checks today, not eight.
Each revival is a line item in its phase and must re-add the check to branch
protection at the same time (`gh api -X PUT
repos/motiva-dot/trilateral/branches/main/protection`). IMPLEMENTATION_PLAN's "the arena never
shrinks" rule now has a sibling: the job list only ever grows.

### ADR-002 — `publish = false` + `deny.toml` are coupled
**Date:** 2026-08-03
**Decision:** `[workspace.package] publish = false` in the root Cargo.toml;
`deny.toml` sets `[licenses.private] ignore = true`.
**Context:** The `lint` job runs `cargo-deny-action` with no config file
committed. Default cargo-deny fails the license check on our own crates,
which carry no `license` field — a red CI on day one for a reason unrelated
to any engine law.
**Consequences:** Removing `publish = false` silently turns CI red. The two
settings must move together. **Every member crate needs an explicit
`publish.workspace = true` line** — workspace `[workspace.package]` fields are
opt-in, not inherited by default, and without that line cargo-deny still sees
ten "public, unlicensed" crates *and* rejects every intra-workspace `path`
dependency as a `wildcard` (its `allow-wildcard-paths` escape hatch only
applies to non-published crates). Both failures have one cause and one fix.
**A new crate added in a later phase must carry that line from birth.** `deny.toml` is also where CLAUDE.md §1.3 (The
Dependency Ban) is actually *enforced* — `[bans].deny` is empty today because
the graph is only serde + smallvec; it must be populated as wgpu/winit/quinn/
kira enter the outer crates, so they can never be pulled down into sim code.

### ADR-003 — Trig tables are generated offline and committed, never built
**Date:** 2026-08-04
**Decision:** `crates/trilateral_fixed/src/tables.rs` is produced by
`cargo run -p tools --bin gen_tables` and committed as integer literals. There
is no `build.rs`.
**Context:** The tables need `sin`/`atan`, which means floats somewhere. A
build script confines the float to build time, which *sounds* safe.
**Alternatives:** (a) `build.rs` — **rejected**: it recomputes the table on
every machine that builds the project, so a slightly different libm on one
developer's box silently produces a different table and a desync that surfaces
at Phase 5 with no obvious cause. This is the cross-platform float hazard
fixed-point exists to eliminate, reintroduced through the back door.
(b) `const fn` integer CORDIC at compile time — viable and elegant, but needs
the atan constants anyway, and const-eval of 130k operations buys nothing over
a committed table you can read.
**Consequences:** Regenerating the tables and getting different numbers is a
determinism-breaking change requiring an ADR; it invalidates every stored
replay. The generator's output cross-checked the hand-written `PI`/`TAU`/
`FRAC_PI_2` constants exactly, and `atan(2^0)` emerged as 536870912 = 2^32/8 =
exactly 45 degrees, which is the table validating itself.

### ADR-004 — Rounding asymmetry between `mul` and `div` is deliberate
**Date:** 2026-08-04
**Decision:** `Fixed::mul` rounds toward negative infinity (arithmetic `>>`),
`Fixed::div` truncates toward zero (integer `/`). Per TECH_SPEC §2 verbatim.
**Consequences:** They disagree for negative operands. This is **not** a bug to
be tidied later: the golden hash and every future replay encode it. Changing
either rule is a determinism break needing its own ADR. Both rules are stated
in the `fixed.rs` module header and pinned by tests so that the next person to
notice the asymmetry finds the reason before the edit button.

### ADR-005 — Component arrays are runtime-sized `Box<[T]>`, not const arrays
**Date:** 2026-08-04 · **Resolves:** Open Spec Conflict #1 · **Approved by the
human architect.**
**Decision:** `Components` holds `Box<[T]>` of length `max_entities`, read from
`assets/data/engine.yaml` and allocated once at match start. CLAUDE.md §3.1 has
been amended accordingly — it previously showed `[T; MAX_ENTITIES]`.
**Context:** CLAUDE.md §3.1 and TECH_SPEC §3 disagreed. Const arrays make
`max_entities` a hardcoded gameplay number in Rust, which §1.8 forbids
outright, and they force a recompile to change prototype scale.
**Alternatives:** (a) const generics `Components<const N: usize>` — pushes N
into every signature in `sim_systems` and still bakes the value in at compile
time; (b) keep const arrays and exempt `max_entities` from §1.8 — rejected,
because the exemption is the thin end of exactly the wedge §1.8 exists to stop.
**Consequences:** §1.4 (no steady-state allocation) is unaffected: allocation
happens at match setup, never per tick, and the allocation gate measures ticks
1,000–2,000. Boxed slices are `Copy`-element and contiguous, so `snapshot()` is
still a flat copy and `hash()` still folds contiguous memory in declared order —
the two properties the SoA design exists to provide. Cost is one pointer
indirection per component array per system, which is amortised across a whole
array traversal.
**Note for Phase 2:** arrays must be *pre-touched* after allocation (write
zeroes across them) so first-touch page faults do not land inside tick 1,000.

### ADR-006 — `SimHasher` is our own fold, not xxh3 or rapidhash
**Date:** 2026-08-04
**Decision:** The state hasher is a small vendored 64-bit fold built from a
multiply-xorshift finaliser, named `SimHasher`. CLAUDE.md §3.1 previously named
"rapidhash or xxh3 with fixed seed, vendored"; that naming is withdrawn.
**Context:** §1.3 permits sim crates only `trilateral_fixed`, `serde` and
`smallvec`, so any hasher must be hand-written inside `sim_core`. Vendoring
xxh3 correctly is a substantial job, and a *subtly incorrect* xxh3 is worse
than an honest custom hash: it would carry a name implying interoperability and
external test vectors it does not actually satisfy.
**Alternatives:** (a) vendor xxh3 — rejected on correctness risk versus zero
benefit, since nothing outside this repo ever consumes these hashes;
(b) vendor xxHash64, which is simpler — still claims compatibility we would
have to verify against official vectors to be entitled to claim.
**Consequences:** We need determinism and avalanche, not interoperability. Our
own golden vectors are pinned in `sim_core`'s tests. If an external tool ever
needs to reproduce a state hash, that is the moment to revisit this — and it
would be a determinism-breaking change invalidating stored replays.

### ADR-007 — YAML parser is `serde-saphyr`
**Date:** 2026-08-04 · Dependency of `sim_content` only, never a sim crate.
**Decision:** `serde-saphyr` 1.0.0, `default-features = false`,
`features = ["deserialize"]`.
**Context (checked on crates.io, not recalled — law 10):**

| crate | version | last updated | notes |
|---|---|---|---|
| `serde_yaml` | 0.9.34+deprecated | — | dtolnay deprecated it |
| `serde_yml` | 0.0.13 | — | **now itself deprecated**: "unmaintained… thin compatibility shim" |
| `libyml` | 0.0.6 | — | likewise deprecated |
| `serde_norway` | 0.9.42 | **Dec 2024** | ~20 months stale; depends on `unsafe-libyaml-norway` |
| `yaml_serde` | 0.10.4 | Mar 2026 | official YAML Organization fork |
| `serde-saphyr` | **1.0.0** | **Jul 2026** | pure Rust, panic-free by design, 3.7M downloads |

**Decision drivers, in order:**
1. **Panic-freedom.** MARKET_POSITION #4 makes balance community-forkable, so
   this parser will be fed files written by strangers. A panic in the loader is
   a crash on someone else's mod. `serde-saphyr` makes this an explicit design
   goal; verified here with malformed, truncated, control-character and
   self-referential-alias inputs — all return errors.
2. **No `unsafe`.** `serde_norway` inherits `unsafe-libyaml-norway`, a
   transpiled-C parser. `sim_core` and `sim_content` are both
   `#![forbid(unsafe_code)]`; a pure-Rust parser keeps that meaningful.
3. **Maintenance.** Updated days before adoption, versus 20 months stale.
4. **Stability.** 1.0.0, so semver actually means something.

**Consequences:** 12 transitive packages; `cargo deny` passes on all of them.
No `Value` type — deserialisation is strongly typed only. That is a *feature*
here: it pairs with `#[serde(deny_unknown_fields)]` so a typo in a community
balance file is reported rather than silently ignored. Dynamic/untyped YAML
handling is not available, which matters only if hot-reload ever needs to diff
arbitrary documents.
**Not yet done:** `Options` exposes parser limits. Before shipping community
content, set alias-expansion and depth limits (YAML "billion laughs").

### ADR-008 — Runtime unit types live in `sim_core`, parse-time in `sim_content`
**Date:** 2026-08-04
**Decision:** `UnitStats`, `AttackStats`, `Registries` in `sim_core::registry`;
the serde `Raw*` structs with their `f64` fields stay in `sim_content`, which
converts once at load.
**Context:** §1.3 lets `sim_systems` depend on `sim_core` and nothing else, but
systems need unit speeds. Without the split there is no legal way for a system
to read a stat.
**Consequences:** The YAML schema can gain a field or change a spelling without
touching anything the simulation compiles against, and no serde attribute can
quietly become load-bearing on tick-rate code. Costs a small amount of
duplication, which is the point rather than a regret.

### ADR-009 — Phase 4 built steering before pathfinding
**Date:** 2026-08-04
**Decision:** Collision and the settle rule shipped before A*, inverting the
order in IMPLEMENTATION_PLAN.
**Context:** The first play session's only substantive finding was "they didn't
collide". There was no terrain at that point, so A* had nothing to path
*around*, while collision is what makes a group read as a group.
**Consequences:** None structurally — the two are independent. Recorded because
the plan's order is otherwise the contract, and a future reader comparing plan
to history should find the reason here rather than infer carelessness.

### ADR-010 — Strict diagonal corner rule in pathfinding
**Date:** 2026-08-04
**Decision:** A diagonal step requires BOTH adjacent orthogonal tiles open, not
merely one.
**Context:** The permissive rule is common in grid games where units are points.
Ours are circles of radius 0.2–0.5 moving continuously, and a building fills
its whole tile.
**Consequences:** Units detour around single building corners rather than
clipping past them. Without this the pathfinder promises routes the collision
system then refuses to walk — the two layers disagreeing about the same world.

### ADR-011 — `min_push` deadband in separation
**Date:** 2026-08-04
**Decision:** Separation pushes below `min_push` (default 1/4096 tile) are
dropped to zero.
**Context:** With 20 units a crowd converged to exactly zero movement; with 150
it never converged at all, still moving after 30,000 ticks. Separation is not a
pure pairwise force once `max_neighbours` bites — in a dense pile each unit
resolves against only some of its overlaps, forces are unbalanced, and the
configuration rotates indefinitely. No value of `separation_response` fixes it.
**Alternatives:** raise `max_neighbours` (does not fix it in principle, only
postpones the density at which it appears); accept residual motion (a settled
army that never stops twitching).
**Consequences:** Termination is structural: once every push is below the
threshold nothing moves, permanently and bit-exactly. Sub-threshold overlaps
persist — 0.008 pixels at normal zoom.
**Lesson recorded deliberately:** the 20-unit result was true and got
generalised. Only testing the number the plan specified exposed that the
property did not scale.

### ADR-012 — HPA* and flow fields NOT NEEDED at prototype scale
**Date:** 2026-08-04 · **RESOLVED by measurement, same day.**
**Resolution:** the bench answered it. Pathfinding costs **0.002 ms per tick**
at 1,200 entities against a 1.5 ms budget — **0.2% used**. Tile A* with a
16-search-per-tick cap is nowhere near its ceiling, so HPA* over 16x16 chunks
and an LRU flow-field cache would be speculative work on a problem that does
not exist, and both add invalidation logic that can desync. They are not built
and should not be built until a measurement says otherwise.
**TECH_SPEC §4 now overstates what is required** and should be amended to say
tile A* plus a per-tick budget suffices at prototype scale, with HPA* named as
a contingency rather than a requirement. Flagged, not silently diverged.
**The real hot spot is elsewhere:** steering is at 89% of its budget, almost
entirely from the three relaxation passes that fixed "mushy". Measured at
0.857 / 1.359 / 2.166 ms for 1/2/3 passes. See benches/baselines.toml.

**Original reasoning, kept:**
**Decision:** Phase 4 ships with tile A* plus a per-tick search budget. HPA*
over 16×16 chunks and integration flow fields are not built yet.
**Context:** TECH_SPEC §4 specifies both. At 128×128 with 16 searches per tick,
tile A* may already sit inside the 1.5 ms pathfinding budget — in which case a
chunk graph and an LRU field cache are speculative work on a problem we do not
have, and both add invalidation logic that can desync.
**Consequences:** The 1,200-mover bench decides. If pathing is inside budget,
these stay unbuilt and TECH_SPEC §4 needs amending to say so; if not, they are
the fix. Either way the answer comes from a measurement rather than from the
spec's assumption.

## Gotchas & Lessons

- **`SimState::hash()` folds `clock.tick`, so it can never test "nothing
  changed over time".** Comparing hashes across a span of ticks is asking an
  impossible question, and the inevitable "not equal" reads convincingly as a
  bug. This produced a false vibration diagnosis that cost real time. Compare
  the components you actually mean.

- **A one-ULP asymmetry became a real behavioural bug.** Deriving the second
  unit's separation push as `total - move_i` conserves the correction exactly
  but makes equal-mass pairs move by amounts differing by one ULP — and a pair
  that pushes itself asymmetrically every tick acquires net drift, so a crowd
  never converges. Symmetry beat conservation. In fixed point, "one ULP" is a
  real number that accumulates rather than a rounding artefact that washes out.

- **`gen` is a reserved keyword in edition 2024.** Cost one compile cycle.

- **wgpu 30 differs substantially from the 24 the doc suite named**:
  `Queue::present` rather than `SurfaceTexture::present`, `multiview_mask`,
  `immediate_size` replacing `push_constant_ranges`, `CurrentSurfaceTexture` as
  an enum rather than a `Result`, Option-wrapped vertex buffer layouts. Read
  the vendored source rather than guessing; and prefer
  `Surface::get_default_config` over hand-building a config, so the next major
  version does not break the file for no benefit.

- **An empty demo map made the pathfinder invisible.** A straight line across
  open ground looks identical whether A* produced it or a beeline did. Nothing
  about movement can be judged by eye until there is something to path around.

- **Cargo feature unification defeats the `float_bridge` quarantine.**
  TECH_SPEC §1 says `float_bridge` is confined to `sim_content`. It is not:
  Cargo features are additive and unified across the graph, so once
  `sim_content` enables it, the same `trilateral_fixed` rlib linked into
  `sim_core` also has `from_f64`/`to_f64` compiled in. The quarantine is a
  convention, not a compile-time wall. What actually enforces it is
  `ban_floats.sh` — any call to `Fixed::from_f64` in a sim crate contains the
  token `f64` and is rejected. Documented in `sim_content/src/lib.rs`.

- **`tech_tree.yaml` shipped with two dangling `track.next` references.**
  `bas_weapons_1` and `con_weapons_1` both pointed at a `_2` that was never
  written — the murmur track was spelled out in full and the other two were
  abbreviated. Found immediately by the loader's reference validation, on the
  very first run against real content. Filled in by mirroring the murmur
  escalation, marked PLACEHOLDER COSTS in the file, and **still needs a balance
  pass** (OPERATIONS §5 keeps numbers human). Sibling of Open Spec Conflict #4.

- **An empty content file must be an error, not an empty registry.** A
  self-referential YAML alias (`&a *a`) parses to a null document and
  deserialises to an empty map without panicking. Silently yielding a registry
  with zero techs means "this race has no tech tree", which no legitimate file
  means. Hence `ContentError::Empty`.

- **`ban_floats.sh` needed a second fix at Phase 1, as predicted.** Per-line
  `// FLOAT_EXCEPTION:` markers do not work for a whole module: the script
  matches line by line, so a marker above a function signature never applies to
  it. Added file-level exemption via `//! FLOAT_EXCEPTION_FILE: <reason>`,
  which **prints the waiver and its reason on every run** — an invisible waiver
  is a waiver that rots. Verified the exemption is file-scoped, not
  crate-scoped: a float in `trilateral_fixed` outside `float_bridge.rs` still
  fails.

- **`FixedAngle::from_degrees` is exact only for power-of-two divisors of the
  turn.** 2^32/360 is not an integer, so most degree values round, and the
  difference of two rounded angles is not the rounded difference — 350 and 10
  degrees land one raw unit off `from_degrees(20)`. Use `from_turn_ratio` with
  a power of two whenever exactness matters. Pinned as a test.

- **`sqrt`'s maximality cannot be expressed via `Fixed::sq`.** At 1 ULP both
  65536^2 and 65537^2 floor to the same `Fixed` after the `>>32`. The contract
  lives in i128, before the shift discards it.

- **`ban_floats.sh` was silently over-reporting.** Its EXCEPTION_PATTERNS are
  anchored (`^\s*//`) but were being matched against `grep -n` output, which
  is `path:lineno:source` — so the anchors never matched and every doc comment
  merely *mentioning* `f64` would have counted as a violation. Fixed by
  stripping the two leading fields before the exception test. Lesson: the
  linter is itself untested code; the first real float it catches is the only
  proof it works. Re-check it at Phase 1 when `trilateral_fixed` gains its
  genuine `float_bridge` exceptions.

- **`.gitattributes` with `*.sh text eol=lf` is load-bearing on Windows.**
  The `lint` job runs `./scripts/ban_floats.sh` on ubuntu. A CRLF checkout
  from this Windows dev machine fails with `bad interpreter: /usr/bin/env
  bash^M` — a confusing red that looks like a script bug.

- **The executable bit does not survive Windows → git → Linux runner.**
  `bootstrap_repo.sh` runs `chmod +x scripts/*.sh`, but Git for Windows has
  `core.filemode` off, so both scripts were committed `100644` and the first
  CI run failed with `./scripts/ban_floats.sh: Permission denied` (exit 126) —
  after fmt and clippy had already passed. Fixed two ways: `git update-index
  --chmod=+x` to set the real mode, and CI now invokes `bash ./scripts/...`
  so the exec bit is no longer load-bearing. **Any future script added from
  this machine needs the `update-index` treatment**; `.gitattributes` cannot
  express file modes, so there is no declarative guard for this one.

- **`--locked` everywhere in CI means `Cargo.lock` must be committed** and
  must be regenerated + committed in the same commit as any dependency change,
  or every CI job fails before it starts.

## Open Spec Conflicts (raise before the phase that hits them)

1. **Component array sizing — Phase 2.** CLAUDE.md §3.1 declares components as
   `[T; MAX_ENTITIES]` (a compile-time const), while TECH_SPEC §3 says
   capacities load at match start from `assets/data/engine.yaml`
   (`max_entities` prototype 2048) with "arrays boxed and pre-touched". These
   are different designs. Runtime sizing is the one consistent with CLAUDE.md
   §1.8 (no gameplay number hardcoded in Rust) and §1.4 (pre-size from
   YAML-declared maxima), so `Box<[T]>` allocated once at match start is the
   likely resolution — but it needs a decision and a doc update, not a silent
   choice in code.

2. **"Vendored" hasher and RNG — Phase 2.** §1.3 permits sim crates only
   serde + smallvec; §1.2 specifies xoshiro256** and §3.1 "rapidhash or xxh3
   ... vendored". Both must therefore be hand-written inside `sim_core`, not
   pulled from crates.io. That is correct and intended — note it so a future
   session does not reach for `rand` or `xxhash-rust` and file an ADR to
   justify it.

3. **ARM CI leg vs. repo visibility — before first push.** `ubuntu-24.04-arm`
   runners are free on public repos; on a private repo they require a paid
   plan. If the repo is private on the free tier, the ARM legs of `build-test`
   and `determinism` will not get a runner. Options: make the repo public,
   pay, or drop the ARM leg (keeping Linux-x64 + Windows-x64 — two
   architectures, but both little-endian x86/ARM divergence in float leakage
   is exactly what the ARM leg is *for*, so dropping it weakens the guard
   most). Decide before branch protection is configured.

4. **AI YAML references a tech id that does not exist.**
   `assets/data/ai/murmur_standard.yaml` band 3 prefers `mur_weapons_track`;
   `assets/data/tech_tree.yaml` defines `mur_weapons_1/2/3` with a
   `track: { next: ... }` chain and no such id. Harmless today (no loader),
   but the Phase 10 rule evaluator must reject unknown ids loudly rather than
   no-op, or the bot will silently skip its upgrade path.
