# GAME_DESIGN.md — Races, Economy, Tech Trees, and AI Decision Trees v2
# Project: TRILATERAL

> Design authority for gameplay content. All numbers here are STARTING VALUES
> that live in YAML — this doc defines structure and intent; the YAML defines
> truth; playtesting defines the future.

---

## 1. The Two-Resource Economy

### 1.1 Ore (the macro clock)
- 6–9 nodes per base location, 1,500 Ore each, 2 harvest slots per node.
- Worker trip: reserve slot → harvest 8 Ore over 60 ticks → return to base.
- Saturation emerges from slot contention: ~2 workers/node optimal, 3rd adds
  ~40% of a full worker. No scripted diminishing-returns table.
- Everything costs Ore. Ore income measures your macro mechanics.

### 1.2 Flux (the strategy clock)
- 1–2 Flux fissures per base; require a race extractor built on top;
  3 harvest slots; 4 Flux per trip over 75 ticks; 2,500 per fissure.
- Gates: all tier-2/3 tech, all elite units, most abilities.
- The eternal question every 30 seconds: more Ore units now, or Flux tech
  later? Expansion timing = Flux timing.

### 1.3 Supply
- Stored ×2 internally (`supply_x2`). Displayed halved. Cap 200 displayed.
- Race-themed: Murmur mobile supply units, Bastion depots (wall pieces!),
  Concord lattice pylons (supply + power field — losing one hurts twice).

---

## 2. Race Design

Design law: **races differ in their macro mechanics first, unit stats second.**
Each race has (a) a production mechanic, (b) a territory/defense mechanic,
(c) a resource quirk, and (d) a signature vulnerability.

### 2.1 MURMUR — the swarm that flows (circles)

**Production — The Brood Pool.** Every Murmur base generates Brood Points
(1 per 90 ticks, stockpile cap 3 per base, +1 rate per Pool Nexus upgrade).
Any unit is spawned by spending Brood Points + resources at any base. One
shared production currency instead of parallel buildings: Murmur macro skill
= never floating Brood Points, and choosing *what* the swarm becomes at the
moment of spending (the larva tension, generalized).

**Territory — Bloom.** Bases and Bloom Nodes slowly spread Bloom fields.
Murmur structures MUST be placed on Bloom; friendly units move +20% on it;
it grants creeping map vision. Killing Bloom Nodes shrinks it. Murmur plays
map-control chess with a living board.

**Resource quirk.** Workers are also the cheapest combat unit (weak bite);
drone-pulls are a real threat. Murmur expansions are cheapest in Ore but cost
a Brood Point — expanding trades army now for economy later, sharply.

**Vulnerability.** Individual units are paper. Off Bloom, the swarm is slow.
Losing bases loses production capacity itself (Brood generation), so Murmur
death-spirals hard when behind.

**Tier structure (template):**
- T1: Mite (cheap melee, 0.5 supply pattern via supply_x2=1), Lasher (ranged
  light), Drone (worker/weak melee), Bloom Node, Supply Sac (mobile supply).
- T2 (Flux): Hydralisk-role ranged core, burrowable ambusher, swarm-heal
  support, Bloom-speed upgrade, Pool Nexus (+Brood rate).
- T3 (heavy Flux): Colossus-mass breaker unit (mass 15, splash), global
  Bloom-recall ability.

### 2.2 BASTION — the wall that advances (squares)

**Production — Foundries & Add-ons.** Classic parallel production buildings;
each Foundry accepts one Add-on (Armory add-on unlocks heavy units from THAT
foundry; Bunker add-on lets it produce while garrisoning). Bastion macro skill
= construction round-trips: **workers are occupied for the full build time**
(SCV-style), and buildings under construction can be attacked and denied.

**Territory — Everything is a wall.** All Bastion buildings block pathing and
have high armor; Depot supply buildings are cheap wall segments that can lower
(gate behavior). Repair: workers restore building/vehicle HP for Ore.
**Salvage:** refund 50% of a building's cost over a 10-second pack-up — the
advancing wall literally relocates.

**Resource quirk.** Bastion's Flux extractor doubles as a defensive turret
socket (one turret add-on). Their expansions are the most expensive but the
most defensible.

**Vulnerability.** Slowest units, worst reactive mobility. Punished brutally
when the wall is out of position or when caught mid-salvage. Occupied workers
mean every building is an economy tax.

**Tier structure (template):**
- T1: Marine-role ranged infantry, worker, Depot (wall/supply), Foundry,
  static turret.
- T2 (Flux): siege-mode artillery (deploy/undeploy, min range), shield-wall
  heavy infantry, Armory add-on, repair-speed tech, mine layer.
- T3 (heavy Flux): fortress crawler (building-unit hybrid, mass 20),
  orbital scan ability (temporary fog reveal — the Flux sink).

### 2.3 CONCORD — the blade that aims (triangles)

**Production — The Lattice.** Pylon-role Lattice Spires project power fields;
production structures only function inside them. Tier-2 unlocks **Warp
Resonance**: units warp in at any Lattice field on the map after a channel —
reinforce the front instantly, at +25% cost premium per warp. Concord macro
skill = lattice geography: power your production, extend your reach, protect
the spires (unpowered structures go dark).

**Territory — Shields.** All Concord units/structures have regenerating
shields (fast out-of-combat regen). Concord wins long sieges of attrition and
punishes half-hearted pokes; loses catastrophically to committed all-ins that
break through before shields matter.

**Resource quirk.** Highest per-unit costs, heaviest Flux dependence.
A Concord army is few, expensive, and each loss is a strategic event.

**Facing mechanics.** Triangle units have a 60° front arc: +25% damage dealt
from front arc, +1 armor vs attacks striking the front, −2 armor vs rear.
Turn rates are the slowest. Concord micro = formation facing; fighting
surrounded Concord is how Murmur wins.

**Vulnerability.** Unit count, turn rates, spire dependence. Snipe the spire,
darken the base.

**Tier structure (template):**
- T1: Lancer (core ranged), worker, Lattice Spire (supply+power), Gateway-role
  Bastile, shield battery.
- T2 (Flux): Warp Resonance tech, blink skirmisher, area-denial caster
  (deterministic zone damage), immortal-role anti-heavy.
- T3 (heavy Flux): Arbiter-role capital (cloaking field aura), mass-recall.

---

## 3. Tech Tree Template (Structure)

Each race's tree in `assets/data/tech_tree.yaml` follows one schema:

```
tier: 1 | 2 | 3
kind: unit_unlock | upgrade | ability_unlock | mechanic
cost: { ore, flux, ticks }
requires: [building ids and/or tech ids]
effects: [list of Modifier specs]           # e.g. {stat: attack_damage, add: 1, applies_to: tag:ranged}
```
Rules:
- Tier gates are buildings (build the T2 structure to see T2 options), Flux
  costs enforce commitment, and every tier has at least one *choice pair* —
  two mutually-competing timings (e.g., speed upgrade vs range upgrade) so
  scouting matters.
- All effects route through the modifier pipeline. If an effect can't be
  expressed as modifiers, it needs an ADR before it needs code.
- Weapon/armor upgrades: 3 levels each for ground weapons / armor per race,
  +1 effect per level, escalating Flux costs — the classic long-game sink.

## 4. Building Tree Template (Structure)

Per race in `buildings.yaml`: `base` (townhall), `supply`, `extractor` (Flux),
`production` (1–3 kinds), `tech` (tier gates), `defense` (1–2 kinds),
`mechanic` (Bloom Node / Add-ons / Lattice Spire). Every building: footprint
(tiles), HP/armor, cost, build_ticks, and — for production — trainable list +
queue size. Placement validation: buildable tiles + race predicate (Murmur:
on Bloom; Concord production: in power; Bastion: anywhere — that's the point).

---

## 5. AI Decision Trees (Data-Driven Bots)

### 5.1 Architecture
The bot = opening book + decision tree, per race, in
`assets/data/ai/<race>_<style>.yaml`, evaluated every `decision_period` ticks
(default 8) under an APM cap. Bot sees only fog-legal state.

### 5.2 Opening book
Ordered steps keyed on supply/resource conditions:
```yaml
opening:
  - at: { supply: 9 }    do: { build: supply }
  - at: { supply: 10 }   do: { train: worker, until_workers: 16 }
  - at: { supply: 12 }   do: { build: production_1 }
  - at: { ore: 400, minute: 3 } do: { expand: nearest }
  - end_book: { minute: 5 }     # falls through to decision tree
```

### 5.3 Decision tree (condition → action, first match wins, priorities)
```yaml
rules:
  - if: { under_attack_at: any_base }        do: { defend: attacked_base, pull_workers: if_desperate }
  - if: { enemy_army_seen: "> mine * 1.3" }  do: { posture: turtle, add: static_defense }
  - if: { flux_bank: "> 500" }               do: { research: next_affordable_tier }
  - if: { supply_blocked: true }             do: { build: supply, count: 2 }
  - if: { army_supply: "> 60", upgrades: ">= t2" } do: { attack: main_base_path }
  - if: { minute: "> 8", bases: "< enemy_bases" }  do: { expand: safest }
  - default:                                 do: { train: army_mix, mix: standard }
```
Observations available to conditions: own economy/army/tech, scouted enemy
(base count, army snapshot, tech seen — all timestamped and fog-legal),
map control proxies. Actions compile to ordinary `Command`s.

### 5.4 Difficulty
`apm_cap`, `decision_period`, `micro_level` (0: a-move; 1: stutter-step;
2: focus-fire + retreat-hurt), and book selection. Cheating (resource
bonuses, vision) is never used — difficulty is speed and micro, keeping bots
honest sparring partners and keeping the bot API credible.

---

## 6. Maps (Prototype)

RON format: tile grid (walkable/buildable/elevation 0-2), Ore/Flux
placements, start locations, watchtower-style vision points optional.
Prototype maps: **Proving Grounds** (2p, standard 4-base layout, one
contested center) and **Crossfire** (2p, short rush distance variant).
A `map_check` tool validates mirror symmetry and per-base resource parity —
run in CI on every map file.

---

## 7. Design Debt Register (Known Open Questions)
- Murmur Brood Point stockpile cap: 3 feels right, untested.
- Concord warp premium 25%: knob for the "deathball reinforcement" risk.
- Bastion salvage at 50%: exploitable for tax-free re-walling? Watch it.
- Bloom spread rate vs denial: the whole matchup lives in this number.
- Facing-arc micro at 30Hz: verify turn-rate granularity feels analog.
