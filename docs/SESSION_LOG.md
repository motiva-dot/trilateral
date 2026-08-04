# SESSION_LOG.md

## FEEL CHECKPOINT 1 — 2026-08-04. First real feel data. VERDICT: needs work.

Architect ran the obstacle map and reported, verbatim in substance:
1. **"Units do give up early."**
2. **"I didn.t see much"** (of the pillar-field oscillation question).
3. **"It is mushy and doesn.t feel distinct enough for later expectations."**
4. **"This needs work."**

This is the first feedback in the project that changed engine behaviour rather
than numbers, and it is worth more than the 304 tests that preceded it: all of
(1) and (3) were invisible to every test we had, because both are about how
something looks over time rather than whether a value is correct.

**Response to (1) — giving up early.** Two changes, one tuning and one real.
- settle_stuck_ticks 12 -> 45, settle_progress_fraction 0.25 -> 0.06. A unit
  must now be almost completely stopped for 1.5 seconds, not 0.4.
- THE ACTUAL BUG: the settle rule could not distinguish JAMMED from QUEUING.
  A unit pressed against someone walking the same way is waiting in line, and
  telling it to give up is precisely what was seen at the corridor mouth.
  Steering now marks a unit `queued` when it overlaps a Moving neighbour that
  is ahead in its own direction of travel, and a queued unit accrues patience
  at a quarter rate. It still settles eventually, so a genuinely dead queue
  cannot deadlock.

**Response to (3) — mushiness.** Also two changes, one tuning and one real.
- separation_response 0.45 -> 0.85.
- THE ACTUAL CAUSE: a single pass cannot resolve a CHAIN. If A overlaps B and
  B overlaps C, pushing B off A drives it into C, and the leftover is exactly
  what "mushy" looks like. Separation now runs 3 relaxation passes per tick,
  each a pure two-pass computation over positions-plus-corrections-so-far, so
  iterating costs determinism nothing.
- Plus a visual half: units now draw with a darkened rim. Two touching units
  without one read as a single larger blob, which is the same complaint even
  when the collision underneath is perfect.

**Response to (2).** Taken as inconclusive rather than as a pass. Also fixed a
reason it may have been unobservable: the demo roster was all light units, so
mass priority was implemented and invisible. The supply sac (mass 8) is now on
screen among mass-1 mites.

**Still unjudged, and the agenda for Feel Checkpoint 2:** whether 0.85 is now
too crisp rather than too mushy, whether the corridor files properly, whether
45 ticks of patience is too forgiving in the opposite direction, and whether
the rim helps or just adds noise.

## PLAY SESSION — 2026-08-04 (second). Collision.
Architect ran the debug view after Phase 4 landed. Verdict: **"the collision
seemed ok."**

That is the first confirmation that separation, mass priority and the settle
rule behave acceptably to a human rather than only to a test. Taken as a pass.

**What it does NOT yet tell us**, recorded so the next session does not mistake
this for a completed Feel Checkpoint 1. "Seemed ok" is the absence of a
reported problem, which is weaker than a verified good. The specific things
the obstacle map was built to expose, still unjudged:
- whether a group ordered through the two-tile CORRIDOR files through it or
  jams at the mouth — the case where the settle rule is most visible, and
  where a wrong `settle_stuck_ticks` shows up as units giving up too early;
- whether crossing the PILLAR FIELD produces oscillation, since constant small
  course corrections are where naive steering wobbles;
- whether `separation_response: 0.45` reads as billiard balls, as mush, or as
  about right — the one number most likely to be wrong;
- whether heavier units visibly shove lighter ones, which is only testable
  once mixed archetypes are on screen together.

None of these are blockers. They are the agenda for a real Feel Checkpoint 1,
which is best run after the bench settles ADR-012, since HPA* would change the
movement characteristics being judged.

## Session 2026-08-04 — Phases 2, 3, 3.5 and the Phase 4 gate
Phases: 2 → complete, 3 → complete, 3.5 → complete, 4 → gate met with two
items open (ADR-012). Tests: **304**, green in debug and release, on three
architectures. 13 PRs, all CI-green before merge.

**The determinism arena became real, twice over.** Phase 2 gave it an actual
`SimState` to fold instead of a hardcoded `0xC0FFEE`; Phase 3 made it run the
real `tick()` pipeline, so it now folds the results of 10,000 ticks of command
execution, arrival arithmetic, vector normalisation and spatial reindexing
across 600 units. Every green before that proved the state store, not the
simulation. Verified byte-identical between an ARM64 CI runner and this
Windows machine.

**Delivered:** SoA `SimState` with snapshot/hash; `Command` + validation at
ingest; four YAML loaders that validate rather than trust; Brood War-derived
unit stats with a documented 23.81fps → 30Hz conversion; the §3.2 tick
pipeline; `SpatialHash`; circle separation with mass priority and the settle
rule; `MacroGrid`; deterministic tile A*; route following with a per-tick
pathing budget; and the Phase 3.5 debug view.

**Decisions recorded as ADR-005 through ADR-012.** Two amended the spec rather
than working around it: component arrays became runtime-sized (§1.8 forbids a
hardcoded `MAX_ENTITIES`), and the "vendored xxh3" hasher was withdrawn in
favour of an honest custom fold, because a subtly incorrect xxh3 would carry a
name implying test vectors it does not satisfy.

**Five mistakes worth remembering, all mine:**
1. A test asserted `100_000²` fits in Q32.32. It does not.
2. `sqrt`'s maximality cannot be stated via `Fixed::sq` at 1 ULP.
3. I diagnosed a crowd-vibration bug that did not exist, because
   `SimState::hash()` folds `clock.tick` and my test asked an impossible
   question. I believed the result before checking the question.
4. A one-ULP asymmetry in separation caused real, accumulating drift.
5. **The important one:** a 20-unit crowd converged to exactly zero and I
   generalised it. 150 units never converged. Only running the number the plan
   specified found it. The gate was right to name a number.

**Play session (the first, under the v3 rule):** the architect ran the debug
view, drag-selected, right-clicked, watched three groups move, and reported
it fine on a laptop trackpad. The one substantive finding — "they didn't
collide" — became Phase 4's first work, and inverted its internal order
(ADR-009).

**Known state of the content:** every gameplay number in the repo is a
placeholder. BW-derived unit stats are calibrated against published sources
for Drone/Zergling/Hydralisk and recalled for the rest (marked UNVERIFIED in
the file). `collider_radius`, `mass`, `turn_rate`, attack arc and the
frontswing/backswing split have no BW equivalent and are invented — and are
precisely the values that determine feel. `steering.yaml` is four guesses.
Weapon/armour ladders are a flat 100/200/300 at the architect's instruction.

**Next session should:**
1. Build the 1,200-mover bench and revive the `bench-regression` CI job
   (ADR-001). It decides ADR-012 — whether HPA* is needed at all.
2. Run Feel Checkpoint 1 against the obstacle map: shoving, group cohesion
   through the corridor, oscillation, settle behaviour. The numbers above are
   what that session tunes.
3. Then Phase 6 (economy + Brood Pool) per the v3 build order — the first
   phase that produces something recognisably a *game* rather than an engine.

## PLAY SESSION — 2026-08-04, first ever. Phases 0–3.5 complete.
The v3 standing rule: a phase whose gate is green but which has not been
*played* is not done. This is the first entry under that rule, and the first
time anything in this project has been judged by eye rather than by assertion.

**Architect's report, verbatim in substance:** ran the debug view; drag-select
worked; right-click moved all three groups of dots and they moved; **they did
not collide**; escape quit cleanly; felt fine on a laptop trackpad.

**Verdict: passed.** Everything the phase promised, works.

**The one observation that matters is "they didn't collide."** That is correct
behaviour today and a complete description of what Phase 4 is for. There is no
steering, no local avoidance and no collision — units are ghosts that pass
through one another. It is also the single largest gap between this and
something that reads as an *army* rather than as 500 independent dots, which
is why Phase 4's work is being reordered around it (see below).

**Trackpad feel is a real data point**, not a footnote. Box-select and
right-click both being comfortable without a mouse means the input handling is
not quietly assuming precision pointing — worth preserving.

**Nothing was reported as wrong**, which given this is the first look is more
suspicious than reassuring. Specific things to watch on the next play session,
once steering exists: whether motion still reads smooth when units are pushing
against each other, and whether the right-click marker still feels instant when
the ordered units cannot immediately move.

**Consequence for Phase 4 ordering.** The plan lists pathfinding before
steering. That order is being inverted: there is no terrain and no obstacles
yet, so A* has nothing to path *around*, while collision is the thing that
makes a group look like a group. Feel first, per the v3 build order's own
logic. HPA* and flow fields follow once there is a map to need them.

## Session 2026-08-04 (#3) — Phase 1 COMPLETE. The thesis is proven.
Phase: 1 → complete | Tests added/passing: 80 / 80, debug and release,
on x86-64 Linux, ARM64 Linux and x86-64 Windows.

**THE GOLDEN HASH AGREED ACROSS THREE ARCHITECTURES: `0x3373a9ec5352e0b7`.**
Ten thousand mixed fixed-point operations — add, sub, mul, div, sqrt,
floor/ceil/round/fract, sin, cos, atan2, vector length, normalize, rotate,
lerp — composed from seed 42 and folded into one FNV-1a value. Same number on
all three. This is the first evidence on real hardware that the determinism
thesis holds, and everything built from here inherits it.

Delivered (PR #4): `Fixed` Q32.32 on i64 with i128 widening; `FixedVec2` with
an i128 `length_sq_wide` that cannot overflow; `FixedAngle` as a wrapping u32
where a full turn is 2^32; committed 4097-entry quarter-wave sin table;
32-iteration CORDIC `atan2`; `turn_toward`/`shortest_diff` for Phase 5 facing;
`float_bridge` behind a feature flag.

Also merged this session: PR #2 (Phase 0 closure docs), PR #3 (v3 build order
— feel brought forward, economy before combat, Brood Pool into Phase 6).

**Three mistakes worth remembering, all caught by tests I had written:**
1. A test asserted 100_000^2 fits in Q32.32. It does not — the integer range
   is +/-2.147e9. Replaced with an explicit range test.
2. `sqrt`'s maximality postcondition stated via `Fixed::sq` is false at 1 ULP.
   It belongs in i128, before the `>>32` discards the distinction.
3. `from_degrees(350)` minus `from_degrees(10)` is one raw unit off
   `from_degrees(20)`, because 2^32/360 is not an integer. Not a bug — but now
   a pinned test, so the trap is documented where someone will hit it.

**Design decisions recorded as ADR-003 (tables generated offline, never via
build.rs) and ADR-004 (the mul/div rounding asymmetry is deliberate).**

**Honest caveat.** `determinism-compare` still compares `headless_sim`'s
`0xC0FFEE` placeholder — that job proves the artifact plumbing, not the maths.
The maths is proven by the golden hash running inside `build-test` on all
three legs. When Phase 2 gives `headless_sim` a real `SimState` to hash, the
arena becomes the stronger of the two guards; until then the golden hash is.

**Test count note:** 80 test functions. Loop-driven sweeps (4096-angle range
check, 512-angle Pythagorean identity, 360-degree atan2 round trip) push
actual cases well past TECH_SPEC's "250+" bar. If that bar meant 250 distinct
functions, it needs expanding — flagged for the architect rather than padded.

**Still open:** no `LICENSE` on a public repo; alignment session not formally
run (much of its content was covered in conversation, not recorded here);
Ledger Open Spec Conflicts #1 (component array sizing) and #2 (vendored hasher
and RNG) both land in Phase 2 and need deciding before code.

**Next session should:**
1. Resolve Open Spec Conflict #1 — `[T; MAX_ENTITIES]` const arrays
   (CLAUDE.md §3.1) vs runtime `Box<[T]>` sized from engine.yaml
   (TECH_SPEC §3). Decide, write the ADR, update the doc, then build.
2. Phase 2: EntityAllocator, Components SoA, SimClock, hand-written SimRng
   and state hasher, snapshot()/hash(), zero-on-free.
3. Give `headless_sim` a real empty `SimState` to hash so the CI arena stops
   comparing a placeholder.

## Session 2026-08-04 (#2) — Phase 0 exit gate MET
Phase: 0 → complete | Tests added/passing: 0 / 0 (still no code — correct)

Repo published public at github.com/motiva-dot/trilateral (chosen over
private so the `ubuntu-24.04-arm` legs run free — see Ledger → Open Spec
Conflicts §3, now resolved).

**First CI run (`08e46fe`): 7/8.** All three `build-test` legs, all three
`determinism` legs and `determinism-compare` passed on the first attempt.
`lint` failed at `./scripts/ban_floats.sh: Permission denied` (exit 126):
`bootstrap_repo.sh` runs `chmod +x`, but Git for Windows has `core.filemode`
off, so both scripts were committed `100644`. Fixed via
`git update-index --chmod=+x` **and** by having CI invoke
`bash ./scripts/ban_floats.sh`, so the exec bit is no longer load-bearing.
PR #1.

**Second run (PR #1): 8/8 green.** Critically, `lint` step 9 (`cargo-deny`)
*executed* this time rather than being skipped behind the earlier failure —
so `deny.toml` + the ten `publish.workspace = true` lines are now proven on
CI, not just locally. Merged with `--rebase` (linear history: `git bisect`
over a determinism regression is far easier without merge commits).

**Phase 0 exit gate — all three conditions met:**
- workspace builds on CI on 3 platforms ✅
- empty-state hash identical across platforms ✅ (`determinism-compare`
  byte-for-byte on x86-64 Linux / ARM64 Linux / x86-64 Windows)
- ban_floats green ✅

**Branch protection enabled on `main`** — 5 required checks, admins included,
linear history, no force-push, no deletion. The invariant is now
unmergeable-around, which is the point.

**Honest caveat on what is actually proven:** `determinism-compare` is
currently comparing `headless_sim`'s hardcoded `0xC0FFEE` placeholder. It
proves the CI plumbing and the artifact-comparison mechanism work end to end.
It proves nothing about any arithmetic. Phase 1's committed golden fuzz hash
is the first time this check has real maths to be wrong about.

**Still open:** no `LICENSE` file on a public repo (sits awkwardly against
MARKET_POSITION's openness argument); alignment session not yet run.

**Next session should:**
1. Run the alignment session (OPERATIONS §2) — or record what was already
   established, and log it as entry #3.
2. Decide the licence.
3. Begin Phase 1 `trilateral_fixed` under the SOP. Decide first whether the
   test contracts are human-authored (OPERATIONS §3.2 prescribes exactly that
   for math crates) or agent-authored and human-reviewed.

## Session 2026-08-03 (#1) — Day Zero: machine setup + Phase 0 (local half)
Phase: 0 | Completed: DAY_ZERO_SETUP steps 1–6, local half of Phase 0
Tests added/passing: 0 / 0 (no code yet — correct for Phase 0)

**Machine setup (was bare):** installed Visual Studio Build Tools 2022
17.14.37516 with the VC++ workload, then rustup → stable 1.97.1
(x86_64-pc-windows-msvc) with clippy + rustfmt. Git 2.55 and Git Bash were
already present, so `scripts/*.sh` run natively — the `xtask` port that
DAY_ZERO_SETUP §5 holds in reserve is not needed. `gh` is not installed.

**Repo:** `bootstrap_repo.sh` run against the reassembled v2 doc suite.
Workspace builds; branch renamed `master` → `main` to match ci.yml and the
planned branch protection.

**Four things in the suite would have made the first CI run red.** Fixed:
1. `ci.yml` jobs `allocation-gate`, `bench-regression`, `map-check` invoke
   `tools` targets that do not exist until Phases 2/3/6 → commented out with
   per-phase revival notes. See ADR-001.
2. No `deny.toml` committed, and our crates carry no `license` field →
   `cargo-deny-action` fails the license check. Added `deny.toml` +
   `publish = false`. See ADR-002.
3. `cargo fmt --check` failed on the bootstrap script's own
   `headless_sim.rs` (one over-long `format!`). Ran `cargo fmt --all`.
4. No `.gitattributes`; a CRLF checkout of `scripts/ban_floats.sh` breaks the
   ubuntu `lint` job with `bad interpreter: ...bash^M`. Added, with
   `core.autocrlf false` set repo-locally.

**Plus one guardrail bug:** `ban_floats.sh` tested its comment-exemption
regexes against `grep -n` output (`path:lineno:source`), so the `^\s*//`
anchors could never match. Fixed, then verified both ways: a planted
`let v: f32` in `sim_core` is caught; a doc comment mentioning `f64` is
exempt.

**Local gate results (all green):**
`cargo build --workspace` · `cargo test --workspace --locked` (0 tests, 11
targets) · `cargo clippy --workspace --all-targets --locked -- -D warnings`
· `cargo fmt --all -- --check` · `./scripts/ban_floats.sh` ·
`cargo deny check` · `headless_sim --arena ci` emits the 11 checkpoint lines
with LF endings on Windows (so the Windows and Linux artifacts are
byte-identical, which is what `determinism-compare` relies on).

**Dependency versions (CLAUDE.md law 10 — checked, not trusted):** the
resolver locked serde 1.0.229 / serde_core 1.0.229 / serde_derive 1.0.229 /
smallvec 1.15.2 (+ proc-macro2, quote, syn, unicode-ident). `Cargo.lock` is
committed and is the version truth from here. The outer-crate list in
TECH_SPEC §1 (wgpu, winit, kira, quinn, glyphon, criterion, bytemuck,
serde_yaml/serde_yml) is still unchecked — those crates enter at Phases 9/12
and get checked then.

**Known issues / not done:**
- No GitHub remote yet, so **CI has never run**. The Phase 0 exit gate
  ("workspace builds on CI on 3 platforms; empty-state hash identical across
  them") is therefore NOT met. Everything CI does has been reproduced locally
  on one platform only.
- `ubuntu-24.04-arm` on a private repo needs a paid plan — decide repo
  visibility before the first push (Ledger → Open Spec Conflicts §3).
- Git identity was unset on this machine; set repo-locally to
  makad / motiva@gmail.com. Change if that is not the desired commit author.
- The alignment session (DAY_ZERO_SETUP §7 / OPERATIONS §2) has **not** been
  run. This entry is day-zero setup, not that session.

**Next session should:**
1. Create the GitHub repo, `git push -u origin main`, watch all four CI jobs,
   and get `determinism-compare` green — that closes the Phase 0 gate.
2. Enable branch protection on `main` requiring `determinism-compare`
   (plus `build-test`, `lint`).
3. Run the alignment session prompt (OPERATIONS §2) and record its
   confirmations here as entry #2 — before any Phase 1 code.
