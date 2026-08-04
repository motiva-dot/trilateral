# ARCHITECTURE_LEDGER.md — Claude Code's Long-Term Memory

## Current State
- Phase: 0 (in progress) | Last session: 2026-08-03 (#1, day zero)
- Tests: none yet (no code) | CI: authored, not yet run on GitHub
- Host dev machine: Windows 11, MSVC toolchain, Git Bash present (the `.sh`
  guardrail scripts run natively — no `xtask` port needed).

## Crate Status
- trilateral_fixed: stub (deps: serde) — Phase 1 target
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
**Consequences:** Branch protection at Phase 0 requires four checks, not
seven. Each revival is a line item in its phase and must re-add the check to
branch protection at the same time. IMPLEMENTATION_PLAN's "the arena never
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

## Gotchas & Lessons

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
