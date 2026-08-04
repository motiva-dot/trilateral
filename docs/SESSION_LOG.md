# SESSION_LOG.md

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
