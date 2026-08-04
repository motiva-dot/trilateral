# DAY_ZERO_SETUP.md — From Bare Machine to First Claude Code Session
# Project: TRILATERAL

> Follow in order. Steps 1–4 are one-time machine setup (~30 min).
> Step 7 is the moment development actually begins.

---

## 1. Toolchain

**Rust** (any OS):
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # macOS/Linux
# Windows: download rustup-init.exe from https://rustup.rs
rustup default stable
rustup component add clippy rustfmt
cargo --version   # verify
```

**Git**: `git --version`; install if absent (Xcode CLT on macOS,
`apt install git` on Debian/Ubuntu, git-scm.com on Windows).

**Windows only**: install "Desktop development with C++" via Visual Studio
Build Tools (the MSVC linker Rust needs). Native Windows works fine for this
project; WSL is optional, not required.

**GPU sanity** (for Phase 9 later): any Vulkan/Metal/DX12-capable driver from
the last decade. `wgpu` handles the rest. Nothing to do now.

## 2. Claude Code

Install via the native installer (preferred over npm — self-updating, no
Node.js dependency), or use the Desktop app if you'd rather not live in a
terminal:
- Docs & installer: https://code.claude.com/docs/en/setup
- Desktop app (includes a Code tab): https://claude.com/download

Then:
```bash
claude --version   # verify install
claude doctor      # verify environment/auth health — also your first
                   # debugging step if anything misbehaves later
```
First launch authenticates via browser: sign in with your Claude
subscription, or set `ANTHROPIC_API_KEY` for pay-per-token. For a project of
this size, a subscription with high usage limits is the economical path —
check current plans at https://claude.com/pricing before deciding.

## 3. GitHub Repository

Create a **private** repo (e.g. `trilateral`). Two notes that matter:
- The CI workflow uses `ubuntu-24.04-arm` runners for the ARM leg of the
  determinism arena — free for public repos, check current availability/
  billing for private repos. If unavailable, drop that matrix leg and keep
  Linux-x64 + Windows; two architectures still catch most divergence.
- Enable branch protection on `main` requiring the `determinism-compare`
  check once CI is live (end of Phase 0). This makes the invariant
  physically unmergeable-around.

## 4. Bootstrap the Repository

Put the v2 doc suite (the files from our conversation) in a folder next to
where you want the repo, then:
```bash
./bootstrap_repo.sh trilateral ./path-to-doc-suite
cd trilateral
cargo build --workspace     # must compile — stubs only, but it compiles
git add -A && git commit -m "Day zero: skeleton + governance suite v2"
git remote add origin <your-repo-url> && git push -u origin main
```
What the script creates: the full crate skeleton per TECH_SPEC §1, docs/ with
the suite, CLAUDE.md at root (Claude Code auto-ingests it), scripts/,
.claude/settings.json permissions, .gitignore, rust-toolchain.toml, and
initialized Ledger + Session Log.

## 5. Verify the Guardrails Before the Agent Touches Anything
```bash
./scripts/ban_floats.sh                      # should pass (empty crates)
cargo clippy --workspace -- -D warnings      # should pass
```
If these don't run on your OS (Windows: use Git Bash for the .sh scripts, or
ask Claude Code in session 2 to port them to a cross-platform Rust `xtask`),
fix that FIRST. Guardrails precede code, always.

## 6. Configure the Working Agreement
Open `.claude/settings.json` (created by bootstrap) and review the allowed
commands. The default: cargo/rustup/scripts allowed, destructive and network-
mutating commands gated. Tighten to taste — you can always approve one-off
commands interactively.

## 7. Session 1 — The Alignment Session (no code)
Launch `claude` in the repo root and paste the alignment prompt from
OPERATIONS.md §2 verbatim. You are checking that the agent can articulate:
Q32.32 rationale, lockstep-first rationale, SoA-over-ECS rationale, the SOP,
and what cross-platform CI proves that grep cannot. If any answer is mushy,
correct it in-session — this conversation is cheap insurance against every
future session.

## 8. Session 2 — Phase 0 for Real
Paste the Phase-start prompt (OPERATIONS.md §2) for Phase 0. Expected output:
CI workflow live and green on GitHub, dependency versions checked against
crates.io and locked, headless_sim stub printing an identical empty-state
hash on every platform. That green `determinism-compare` check is your
foundation stone; celebrate it, then protect it forever.

## 9. Operating Cadence (the human rhythm)
- **One phase task per session.** Resist "while you're at it."
- **You run the gates.** Session ends when the SOP checklist is done and the
  Session Log is written — read that log entry yourself before closing.
- **Play early, play often.** From Phase 9 on, your YAML-tuning play sessions
  are as scheduled as coding sessions; feel debt compounds like tech debt.
- **Weekly:** skim the Ledger for drift, `cargo update` dry-run check
  quarterly, and back up replays/desync traces — they are irreplaceable
  debugging evidence.

## 10. Definition of "Set Up" (exit test for this document)
[ ] `claude doctor` clean  ·  [ ] repo pushed, CI green on all platform legs
[ ] `determinism-compare` passing and branch-protected
[ ] Alignment session transcript saved to docs/SESSION_LOG.md as entry #1
[ ] Phase 1 prompt ready to paste tomorrow
