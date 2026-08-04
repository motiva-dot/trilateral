# ARCHITECTURE_LEDGER.md — Claude Code's Long-Term Memory

## Current State
- Phase: **1 COMPLETE** — exit gate met 2026-08-04. Next: Phase 2
  (SoA SimState + registries). Build order is v3 — see IMPLEMENTATION_PLAN.
- Last session: 2026-08-04 (#3) | Tests: 80, green on 3 architectures
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
- sim_core: stub (deps: trilateral_fixed, serde, smallvec)
- sim_systems: stub (deps: sim_core)
- sim_content: stub (deps: sim_core)
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

## Gotchas & Lessons

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
