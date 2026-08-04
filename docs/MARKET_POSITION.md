# MARKET_POSITION.md — Competitive Positioning v2 (August 2026)
# Project: TRILATERAL

---

## 1. The Landscape, Honestly

- **StarCraft II / Brood War** remain the competitive gold standard and the
  incumbent to which every new RTS is compared — maintained but not evolving.
- **Stormgate** (Frost Giant): the cautionary tale of the decade. Ex-Blizzard
  pedigree + $40M-class funding + "SC2 successor" positioning → launch
  reception collapsed and the player base evaporated. Lesson: pedigree and
  familiarity do not retain players; a new ladder-first RTS must give lapsed
  players a *structural* reason to switch.
- **ZeroSpace**: entered Early Access July 2026; ambitious (RPG campaign,
  Galactic War meta) but reception mixed with performance/polish complaints.
  Lesson: breadth of modes cannot compensate for technical roughness; feel
  and stability are the product.
- **Beyond All Reason (BAR)**: the genre's quiet success — free, open-source,
  community-owned, modest graphics, enormous goodwill and steady growth.
  Lesson: openness is a durable moat that AAA competitors cannot copy.
- **AoE II/IV, Tempest Rising, Immortal: Gates of Pyre**: healthy niches;
  none contests the "sharp 1v1 macro RTS" throne directly.
- Structural headwinds: RTS esports monetization is weak; multiplayer-only
  launches churn hard; the audience that *watches* RTS dwarfs the audience
  that ladders.

## 2. Positioning Statement

**For lapsed Brood War/SC2 players and the RTS-curious, TRILATERAL is the
competitive 1v1 RTS whose engine is the feature**: it feels sharper than
anything shipping (Frame-0 response, deterministic outcomes), it teaches
better than anything shipping (seekable replays, ghost training, honest bots
at every level), and it is more open than anything a studio can afford to be
(YAML-forkable balance, first-class bot API, sub-100MB, runs on a potato).

We do not out-Blizzard Blizzard on content volume. We out-engineer everyone
on the things determinism makes nearly free for us and prohibitively
expensive for engines built on Unity/Unreal/legacy netcode.

## 3. Differentiators (Each Maps to a PRD Requirement)

| # | Differentiator | Why incumbents can't easily match |
|---|---|---|
| 1 | **Seekable kilobyte replays** — scrub backward/forward instantly, watch from any player, share by paste | Requires deterministic sim + snapshot keyframes designed in from day 1 |
| 2 | **Ghost trainer** — race your own (or a pro's) recorded macro live | Only possible when replays are command streams into a deterministic engine |
| 3 | **Honest bot API** — headless faster-than-realtime sim, documented InputSource; community bots, RL research, graded sparring partners | Legacy engines can't run headless or deterministic; our bot never cheats, so practice transfers |
| 4 | **Forkable balance** — every number is YAML; community PTRs are file swaps; content hashes keep ladder pure | Data-driven to the bone; studios gate balance behind patches for control we don't need |
| 5 | **Feel** — Frame-0 feedback, no combat RNG, event-loss-proof input, hardware cursor | Cheap for us, cultural retrofit for others |
| 6 | **Reach** — <100MB, 2015 integrated GPU min-spec, crisp at any zoom | Vector/SDF aesthetic as strategy, not budget apology |
| 7 | **Watchability** — geometric readability + observer tools + backward-scrub casting | Casters can rewind live analysis; nobody else can |

## 4. What We Deliberately Do NOT Compete On (Prototype Era)
Campaign/story production values; 3D spectacle; mode breadth (co-op, 3v3);
esports prize ecosystems; console/mobile. Each is a later bet from strength,
not a launch requirement — the anti-Stormgate discipline.

## 5. Open Strategy Question (Decide Post-Prototype, From Data)
Distribution: paid (Tempest Rising model) vs F2P (Stormgate's albatross) vs
open-core (BAR's moat). The architecture stays storefront-agnostic; the
bot API and balance forkability are valuable under every model. Revisit when
P3 exit tests pass and external playtesting begins.

## 6. Risks & Mitigations
- **"Another indie RTS" invisibility** → the bot API + ghost trainer are
  demo-able hooks content creators can show in 60 seconds; lead marketing
  with the engine tricks, not faction lore.
- **Solo/agentic dev scope creep** → the phase gates in IMPLEMENTATION_PLAN
  are the contract; P1's exit test (a BW player says "sharp") is the only
  early metric that matters.
- **Balance with 3 asymmetric races and no playtest population** → bot
  round-robin telemetry (win rates by book/matchup, headless, thousands of
  games overnight) as a first-pass balance instrument — another thing only a
  deterministic headless engine gets for free.
