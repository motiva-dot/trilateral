# PRD.md — Product Requirements Document v2
# Project: TRILATERAL (working title)

---

## 1. Vision

A fiercely competitive 1v1 RTS in the Brood War lineage — three deeply
asymmetric races, a two-resource macro economy, and a mechanical skill ceiling
worth a decade of mastery — built on a modern deterministic engine whose
architecture unlocks features no legacy RTS can match: perfectly seekable
kilobyte replays, a first-class bot API, ghost-replay training, and fully
community-forkable balance. Clean geometric 2D art keeps the game readable at
any zoom, tiny to download, and playable on modest hardware.

**Not the pitch:** "SC2 with new factions." The 2026 market has repeatedly
punished that pitch. **The pitch:** the sharpest-feeling, most learnable, most
open competitive RTS ever shipped — where the engine itself is the feature.

---

## 2. Design Pillars

1. **Macro is the game.** Races differ most in *how you produce and expand*,
   not just in unit stats. Economy tension (Ore vs Flux, expansion timing,
   worker count) drives every decision, exactly as in Brood War.
2. **Deterministic skill expression.** No combat RNG. Stutter-step, surrounds,
   walls, facing micro, and spellcasting reward mechanics; outcomes are earned.
3. **Readable at a glance.** Geometric silhouettes (Murmur circles, Bastion
   squares, Concord triangles), faction color, and crisp SDF edges. A
   spectator parses the fight instantly from any zoom level.
4. **Zero-perceived latency.** Frame-0 acknowledgment on every command; the
   400-APM player never feels the network.
5. **The engine is a platform.** Headless, deterministic, data-driven — so
   replays, bots, trainers, and mods are cheap for us and for the community.

---

## 3. The Races (Summary — full spec in GAME_DESIGN.md)

| | **Murmur** (Circles) | **Bastion** (Squares) | **Concord** (Triangles) |
|---|---|---|---|
| Fantasy | The swarm that flows | The wall that advances | The blade that aims |
| Production | Pooled brood capacity at bases; all units from one queue economy | Parallel structures + add-ons; workers occupied while building | Lattice power fields; instant warp-in near lattice pylons |
| Economy hook | Bloom fields (territory) speed friendly movement & are required to build on | Repair + salvage; buildings are walls | Shields regenerate; expensive units, unforgiving losses |
| Collision | Circle/RVO — units flow and surround | AABB — units and buildings lock space | SAT triangles — facing matters, wedge formations |
| Weakness | Fragile units, needs constant reinforcement | Slow, immobile, punished off-position | Low unit count, punished for waste |

Two resources everywhere: **Ore** (abundant, worker-scaling) and **Flux**
(scarce, gates tier 2/3 tech and elite units). Supply per race via race-themed
supply structures/units.

---

## 4. Target Scale & Feel

| Metric | Prototype | Ship Target |
|---|---|---|
| Sim tick rate | 30 Hz locked | 30 Hz (60 Hz experiment post-ship) |
| Render rate | Monitor rate, interpolated | Same |
| Max entities | 1,200 | 4,000 |
| Sim budget/tick | ≤ 8 ms @ 1,200 entities | ≤ 20 ms @ 4,000 |
| Input→visual feedback | < 1 frame | Same |
| Input→sim execution | ≤ 4 ticks (133 ms, masked) | Adaptive 2–4 ticks |
| Map size | 128×128 tiles | 256×256 |
| Players | 1v1 (+ bot) | 1v1 ranked; 2v2 later |
| Download size | < 100 MB | < 200 MB |
| Min spec | 2015 integrated GPU | Same |

---

## 5. Prototype Roadmap (What "Done" Means)

### P1 — One Race Plays Real (the vertical slice)
Murmur vs scripted Murmur bot on one map. Full loop: 2-resource economy,
worker saturation, expansion, Bloom mechanic, tier-1 units + 2 tier-1 techs,
production, combat with stutter-step, fog, minimap, control groups, replays,
determinism CI green on 3 platforms.
**Exit test:** a Brood War player plays 3 games and reports the *feel* —
responsiveness, pathing, macro rhythm — as "sharp," with a written punch list.

### P2 — Three Races, Real Asymmetry
Bastion + Concord complete through tier 2; all 6 matchups playable vs bot;
tech trees live through the modifier pipeline; per-race AI decision trees;
2 tournament-legal maps; hotkey remapping.
**Exit test:** each matchup produces visibly different game shapes (timing
attack, macro game, all-in) depending on bot opening book.

### P3 — Two Humans, One Truth
Online lockstep 1v1 (direct + relay fallback), reconnect via catch-up, desync
detection with auto-saved trace logs, observer client, replay viewer with
seek/speed controls, ghost-trainer v0 (race your own economy curve).
**Exit test:** 20 consecutive online games across mixed platforms, zero
desyncs, and a blind latency test where players cannot identify 80 ms vs LAN.

### Beyond prototype (ship track, not scheduled here)
Ladder + MMR, map pool rotation, spectator/casting tools, tier-3 + abilities
pass, balance telemetry, bot API stabilization + docs, editor, campaign-lite
tutorial ("learn the macro loop in 30 minutes").

---

## 6. Competitive Product Requirements (from MARKET_POSITION.md)

These are requirements, not aspirations, because they are our differentiation:

1. **Replays**: kilobyte files, instant seek to any tick (snapshot keyframes),
   watch-from-any-player-view, shareable by paste.
2. **Ghost trainer**: play against the recorded command stream of any replay
   (including your own) — the deterministic engine makes "race your ghost's
   economy" a first-class practice mode no competitor offers.
3. **Bot API**: the headless sim + `InputSource` trait is a supported,
   documented surface. Community bots and practice partners at every skill
   level; the sim runs faster-than-realtime for training.
4. **Forkable balance**: the entire game's numbers are YAML. Community balance
   mods and "PTR" experiments are file swaps, hash-tagged so ladder integrity
   is preserved.
5. **Runs on anything**: the vector aesthetic is a strategic choice — min-spec
   reach and instant readability, not a budget compromise.

---

## 7. Explicit Design Decisions (Logged, Arguable, Decided)

| Decision | Choice | Rationale |
|---|---|---|
| Combat RNG | None | Watchable, learnable, fair; audit item 8 |
| Selection cap | Unlimited + multi-building select | Skill lives in decisions & micro, not UI friction; BW's 12-cap is heritage, not essence |
| Auto-workers | First worker cycle auto-splits at match start; nothing else automated | Respect macro skill; remove only the rote 10 seconds |
| Smart-cast | On by default, toggleable | Modern baseline |
| High ground | Vision denial + damage-taken modifier uphill | Deterministic BW spirit |
| Unit pathing quality | Good, not perfect — clumping and shoving are tunable YAML, because *some* friction creates positional skill | The BW lesson: perfect pathing deletes a skill axis |
| F2P vs paid | Undecided; architecture must not assume storefront | Stormgate cautionary tale — decide from strength later |

---

## 8. Out of Scope (Prototype)
3D, campaign, >2 players, mobile/console, editor UI (maps are hand-authored
RON), matchmaking service, monetization, localized audio.
