# OPERATIONS.md — Commanding Claude Code + Living Doc Templates v2
# Project: TRILATERAL
# (Human-facing. Consolidates v1's CLAUDE_CODE_GUIDE + Ledger/SessionLog templates.)

---

## 1. Repo Initialization
```bash
mkdir trilateral && cd trilateral && git init
# copy this doc suite in; CLAUDE.md MUST sit at repo root
mkdir -p docs && mv 00_AUDIT.md PRD.md GAME_DESIGN.md TECH_SPEC.md \
  IMPLEMENTATION_PLAN.md MARKET_POSITION.md OPERATIONS.md docs/
chmod +x scripts/*.sh
git add -A && git commit -m "Governance suite v2"
```
`.claude/settings.json`: allow `cargo*`, `./scripts/*`, `rustup*`, and
read-only shell basics; deny `sudo*` and destructive globs.

## 2. Session Prompts (Copy-Paste)

**First-ever alignment session:**
> Read CLAUDE.md at the repo root, then docs/00_AUDIT.md. Confirm in your own
> words: (1) why Fixed is Q32.32 on i64 and what bug that prevents, (2) why
> netcode is lockstep-first not rollback, (3) why we use a custom SoA SimState
> instead of an ECS crate, (4) the 6-step SOP, (5) what the cross-platform
> determinism CI job proves that ban_floats.sh cannot. Then read
> docs/IMPLEMENTATION_PLAN.md Phase 0 and list its tasks. Write no code yet.

**Phase start:**
> Read CLAUDE.md, docs/ARCHITECTURE_LEDGER.md, latest docs/SESSION_LOG.md
> entry. We are starting Phase N: <name>. Read that phase in
> docs/IMPLEMENTATION_PLAN.md, restate its exit gate, then begin task 1 under
> the SOP.

**Resume mid-phase:**
> Read CLAUDE.md, the Ledger, and the latest Session Log entry. Continue
> Phase N from "<next step>" recorded there. SOP applies.

**Determinism failure:**
> The determinism job failed. Do NOT guess. Run
> ./scripts/run_desync_trace.sh, identify (tick, entity, component,
> last-writing system), state the root cause in one paragraph, then fix the
> root cause and re-run the arena.

**Drift containment:**
> STOP. You are working only in crates/<crate>/. Re-read CLAUDE.md §4
> (Ubiquitous Language) and §6 (Never Do). Restate the current task in one
> sentence and continue within scope.

## 3. Anti-Hallucination Playbook (What Actually Works)
1. **Negative constraints up front** — the training prior is Unity tutorials;
   every task prompt for sim code repeats: "fixed-point only, no delta time,
   no HashMap, exact-value assertions."
2. **Contract-first** — you write the fn signature + the failing test; Claude
   fills the body. Especially for math and collision.
3. **Exact-value tests** — never "approximately"; fixed-point makes exact
   expected values computable, and exactness is itself a determinism test.
4. **Scope the visible files** — name what may be read, what may be edited.
5. **Explain-before-code** for any system touching ordering, RNG, or ties.
6. **Watch for drift signals** — invented type names, phantom crates, skipped
   SOP steps, floats in sim diffs. Any one ⇒ end session, resume fresh.
7. **Never let it "fix" a desync by re-rolling** — root cause or nothing.

## 4. Phase Gates Are the Contract
The exit gates in IMPLEMENTATION_PLAN.md are non-negotiable. If a gate is
red, the phase is not done, regardless of how much code exists. You (human)
personally run the P1/P2/P3 PRD exit tests — feel cannot be delegated.

## 5. What Stays Human
Balance numbers (YAML sessions while playing), map layouts, art direction
(palette, shader feel), keybind defaults, the paid/F2P/open decision, and
every ADR that adds a sim-crate dependency.

---

## 6. TEMPLATE — docs/ARCHITECTURE_LEDGER.md
```markdown
# ARCHITECTURE_LEDGER.md — Claude Code's Long-Term Memory
## Current State
- Phase: 0 (not started) | Last session: — | Tests: — | CI: —
## Crate Status
(trilateral_fixed / sim_core / sim_systems / sim_content / bot / netcode /
 replay / presentation / trilateral_app / tools — status, key type names,
 gotchas, one line each; update every session)
## ADR Log
### ADR-000 (template): Date / Decision / Context / Alternatives / Consequences
## Gotchas & Lessons
(append surprising behaviors, exact-value pitfalls, table-edge cases)
```

## 7. TEMPLATE — docs/SESSION_LOG.md
```markdown
# SESSION_LOG.md
## Session YYYY-MM-DD (#n)
Phase: N | Completed: … | Tests added/passing: x / y
Files touched: … | Known issues: …
Next session should: 1) … 2) … 3) …
```
