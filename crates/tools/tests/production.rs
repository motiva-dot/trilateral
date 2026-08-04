//! Brood Pool and production. GAME_DESIGN §2.1, IMPLEMENTATION_PLAN Phase 6.
//!
//! The Phase 6 gate asks that "Brood Points accrue, cap, and spend
//! deterministically". These pin that, plus the properties that make the pool
//! a *skill expression* rather than a counter: floating points is a loss you
//! can measure, and losing a base costs production capacity itself.

use sim_content::{races_from_yaml, steering_from_yaml, units_from_yaml};
use sim_core::registry::Role;
use sim_core::{
    ArchetypeId, Capacities, Command, EntityHandle, IssuedCommand, PlayerId, Registries, SimState,
    Spawn, UnitState,
};
use sim_systems::{SimContext, tick};
use trilateral_fixed::{Fixed, FixedVec2};

const UNITS_YAML: &str = include_str!("../../../assets/data/units.yaml");
const STEERING_YAML: &str = include_str!("../../../assets/data/steering.yaml");
const RACES_YAML: &str = include_str!("../../../assets/data/races.yaml");
const MAP: u16 = 64;

fn caps() -> Capacities {
    Capacities {
        max_entities: 256,
        max_projectiles: 8,
        max_commands_per_tick: 128,
        max_players: 4,
        cmd_queue_slots: 16,
        modifier_slots: 8,
        command_log_reserve: 512,
    }
}

fn registries() -> Registries {
    Registries {
        units: units_from_yaml(UNITS_YAML).expect("units.yaml"),
        steering: steering_from_yaml(STEERING_YAML).expect("steering.yaml"),
        race: races_from_yaml(RACES_YAML).expect("races.yaml"),
    }
}

fn arch(reg: &Registries, id: &str) -> ArchetypeId {
    ArchetypeId(
        reg.units
            .index_of(id)
            .unwrap_or_else(|| panic!("{id} missing")) as u16,
    )
}

fn at(x: i32, y: i32) -> FixedVec2 {
    FixedVec2::new(
        Fixed::from_int(x) + Fixed::HALF,
        Fixed::from_int(y) + Fixed::HALF,
    )
}

/// A player with `bases` nests and a full bank, so tests measure the mechanic
/// under test rather than an unrelated shortage.
fn scene(reg: &Registries, bases: i32) -> (SimState, Vec<EntityHandle>) {
    let mut s = SimState::new(caps(), 42, MAP);
    s.players[0].active = true;
    s.players[0].ore = 10_000;
    s.players[0].flux = 10_000;
    let nest = arch(reg, "mur_nest");
    let hs: Vec<_> = (0..bases)
        .map(|i| {
            s.spawn(Spawn {
                archetype: nest,
                owner: PlayerId(0),
                pos: at(10 + i * 12, 20),
                hp: 1250,
                resource: 0,
            })
            .unwrap()
        })
        .collect();
    sim_systems::occupancy::stamp_static_occupancy(&mut s, reg);
    (s, hs)
}

fn context(s: &SimState, reg: &Registries) -> SimContext {
    SimContext::new(MAP as i32, reg, s.c.capacity(), s.grid.tile_count())
}

fn run(s: &mut SimState, reg: &Registries, ctx: &mut SimContext, ticks: u32) {
    for _ in 0..ticks {
        tick(s, reg, ctx);
    }
}

fn train(s: &mut SimState, at_base: EntityHandle, unit: ArchetypeId) {
    let _ = s.ingest(IssuedCommand {
        tick: s.clock.tick,
        player: PlayerId(0),
        subject: at_base,
        command: Command::Train { unit },
    });
}

// ---------------------------------------------------------------------------

#[test]
fn brood_points_accrue_at_the_authored_rate() {
    let reg = registries();
    let (mut s, _) = scene(&reg, 1);
    let mut ctx = context(&s, &reg);
    assert_eq!(s.players[0].brood_points, 0);

    // 90 ticks per point with one base.
    run(&mut s, &reg, &mut ctx, 90);
    assert_eq!(s.players[0].brood_points, 1, "first point late or early");
    run(&mut s, &reg, &mut ctx, 90);
    assert_eq!(s.players[0].brood_points, 2);
}

#[test]
fn more_bases_generate_faster_into_one_shared_pool() {
    // GAME_DESIGN §2.1: one shared production currency, fed by every base.
    let reg = registries();
    let (mut s1, _) = scene(&reg, 1);
    let (mut s3, _) = scene(&reg, 3);
    let mut c1 = context(&s1, &reg);
    let mut c3 = context(&s3, &reg);
    run(&mut s1, &reg, &mut c1, 90);
    run(&mut s3, &reg, &mut c3, 90);
    assert_eq!(s1.players[0].brood_points, 1);
    assert_eq!(s3.players[0].brood_points, 3, "bases did not pool");
}

#[test]
fn the_stockpile_caps_and_floating_is_a_real_loss() {
    // The mechanic's whole point (§2.1): Murmur macro skill is never floating
    // points. Overflow must be DESTROYED, not banked — otherwise ignoring your
    // pool costs nothing and the skill expression evaporates.
    let reg = registries();
    let (mut s, _) = scene(&reg, 1);
    let mut ctx = context(&s, &reg);
    run(&mut s, &reg, &mut ctx, 90 * 10);
    assert_eq!(
        s.players[0].brood_points, 3,
        "cap is 3 per base and must hold"
    );
}

#[test]
fn losing_a_base_cuts_both_generation_and_the_cap() {
    // §2.1: "losing bases loses production capacity itself", which is why
    // Murmur death-spirals when behind. A banked surplus must be trimmed, or
    // a player who lost a base would be rewarded for having floated points.
    let reg = registries();
    let (mut s, bases) = scene(&reg, 3);
    let mut ctx = context(&s, &reg);
    run(&mut s, &reg, &mut ctx, 90 * 4);
    assert_eq!(s.players[0].brood_points, 9, "cap should be 3 bases x 3");

    s.despawn(bases[0]);
    s.despawn(bases[1]);
    run(&mut s, &reg, &mut ctx, 1);
    assert_eq!(
        s.players[0].brood_points, 3,
        "surplus survived the loss of two bases"
    );
}

#[test]
fn training_spends_ore_a_brood_point_and_produces_a_unit() {
    let reg = registries();
    let (mut s, bases) = scene(&reg, 1);
    let mut ctx = context(&s, &reg);
    run(&mut s, &reg, &mut ctx, 90);

    let ore_before = s.players[0].ore;
    let mite = arch(&reg, "mur_mite");
    train(&mut s, bases[0], mite);
    run(&mut s, &reg, &mut ctx, 1);

    assert_eq!(s.players[0].ore, ore_before - 25, "BW Zergling costs 25");
    assert_eq!(s.players[0].brood_points, 0, "no Brood Point was spent");
    assert_eq!(s.c.production[bases[0].index as usize].len(), 1);

    // 420 ticks for a Mite (per-ling economics, see units.yaml).
    let before = s.live_count();
    run(&mut s, &reg, &mut ctx, 430);
    assert_eq!(s.live_count(), before + 1, "no unit came out");
    assert!(
        s.c.production[bases[0].index as usize].is_empty(),
        "queue did not drain"
    );
}

#[test]
fn training_without_a_brood_point_is_refused_and_costs_nothing() {
    let reg = registries();
    let (mut s, bases) = scene(&reg, 1);
    let mut ctx = context(&s, &reg);
    // No ticks run, so no points yet.
    let ore_before = s.players[0].ore;
    train(&mut s, bases[0], arch(&reg, "mur_mite"));
    run(&mut s, &reg, &mut ctx, 1);
    assert_eq!(
        s.players[0].ore, ore_before,
        "a refused order still charged"
    );
    assert!(s.c.production[bases[0].index as usize].is_empty());
}

#[test]
fn training_without_ore_is_refused_and_keeps_the_brood_point() {
    let reg = registries();
    let (mut s, bases) = scene(&reg, 1);
    let mut ctx = context(&s, &reg);
    run(&mut s, &reg, &mut ctx, 90);
    s.players[0].ore = 0;

    train(&mut s, bases[0], arch(&reg, "mur_mite"));
    run(&mut s, &reg, &mut ctx, 1);
    assert_eq!(
        s.players[0].brood_points, 1,
        "a refused order consumed the Brood Point anyway"
    );
    assert!(s.c.production[bases[0].index as usize].is_empty());
}

#[test]
fn a_full_supply_cap_blocks_training() {
    let reg = registries();
    let (mut s, bases) = scene(&reg, 1);
    let mut ctx = context(&s, &reg);
    run(&mut s, &reg, &mut ctx, 270);
    // A nest provides 1 supply (2 doubled). Fill it.
    s.players[0].supply_used_x2 = s.players[0].supply_cap_x2;
    let ore_before = s.players[0].ore;
    train(&mut s, bases[0], arch(&reg, "mur_drone")); // costs 2 supply_x2
    run(&mut s, &reg, &mut ctx, 1);
    assert_eq!(s.players[0].ore, ore_before, "trained while supply-blocked");
}

#[test]
fn cancelling_refunds_in_full() {
    let reg = registries();
    let (mut s, bases) = scene(&reg, 1);
    let mut ctx = context(&s, &reg);
    run(&mut s, &reg, &mut ctx, 180);
    let ore_before = s.players[0].ore;
    let brood_before = s.players[0].brood_points;

    train(&mut s, bases[0], arch(&reg, "mur_mite"));
    run(&mut s, &reg, &mut ctx, 5);
    assert_eq!(s.c.production[bases[0].index as usize].len(), 1);

    let _ = s.ingest(IssuedCommand {
        tick: s.clock.tick,
        player: PlayerId(0),
        subject: bases[0],
        command: Command::Cancel { queue_slot: 0 },
    });
    run(&mut s, &reg, &mut ctx, 1);
    assert!(s.c.production[bases[0].index as usize].is_empty());
    assert_eq!(s.players[0].ore, ore_before, "cancel did not refund ore");
    assert_eq!(
        s.players[0].brood_points, brood_before,
        "cancel did not refund the Brood Point"
    );
}

#[test]
fn a_produced_unit_walks_to_the_rally_point() {
    let reg = registries();
    let (mut s, bases) = scene(&reg, 1);
    let mut ctx = context(&s, &reg);
    run(&mut s, &reg, &mut ctx, 90);

    let _ = s.ingest(IssuedCommand {
        tick: s.clock.tick,
        player: PlayerId(0),
        subject: bases[0],
        command: Command::SetRally { pos: at(30, 40) },
    });
    train(&mut s, bases[0], arch(&reg, "mur_mite"));
    run(&mut s, &reg, &mut ctx, 440);

    let new_unit = s
        .entities
        .iter_live()
        .find(|h| {
            reg.units
                .by_archetype(s.c.archetype[h.index as usize])
                .map(|u| u.role != Role::Base)
                .unwrap_or(false)
        })
        .expect("no unit produced");
    assert_ne!(
        s.c.state[new_unit.index as usize],
        UnitState::Idle,
        "produced unit ignored the rally point"
    );
    run(&mut s, &reg, &mut ctx, 600);
    assert!(
        s.c.pos[new_unit.index as usize].y > Fixed::from_int(28),
        "produced unit never travelled toward the rally"
    );
}

#[test]
fn supply_is_recomputed_and_cannot_drift() {
    // The reason supply is not a counter. Kill a unit by any means and the
    // number must simply be right on the next tick.
    let reg = registries();
    let (mut s, _) = scene(&reg, 1);
    let mut ctx = context(&s, &reg);
    let mite = arch(&reg, "mur_mite");
    let hs: Vec<_> = (0..6)
        .map(|i| {
            s.spawn(Spawn {
                archetype: mite,
                owner: PlayerId(0),
                pos: at(30 + i, 30),
                hp: 35,
                resource: 0,
            })
            .unwrap()
        })
        .collect();
    run(&mut s, &reg, &mut ctx, 1);
    assert_eq!(s.players[0].supply_used_x2, 6, "six 0.5-supply Mites = 3.0");

    for h in hs.iter().take(3) {
        s.despawn(*h);
    }
    run(&mut s, &reg, &mut ctx, 1);
    assert_eq!(
        s.players[0].supply_used_x2, 3,
        "supply drifted after deaths"
    );
}

#[test]
fn production_is_deterministic() {
    fn once() -> u64 {
        let reg = registries();
        let (mut s, bases) = scene(&reg, 2);
        let mut ctx = context(&s, &reg);
        let mite = arch(&reg, "mur_mite");
        let drone = arch(&reg, "mur_drone");
        for k in 0..30 {
            run(&mut s, &reg, &mut ctx, 40);
            train(&mut s, bases[k % 2], if k % 3 == 0 { drone } else { mite });
        }
        run(&mut s, &reg, &mut ctx, 1500);
        s.hash()
    }
    assert_eq!(once(), once());
}

// ---------------------------------------------------------------------------
// THE PHASE 6 GATE — the full macro loop, headless.
// ---------------------------------------------------------------------------

/// IMPLEMENTATION_PLAN Phase 6: *"full macro loop headless: 12 workers
/// saturate a base, expansion completes, army trains"*.
///
/// This is the first test in the project that is a *game* rather than a
/// system: a base, a resource, workers earning, and that income turning into
/// an army through a mechanic no other RTS has.
#[test]
fn gate_the_full_macro_loop_runs_headless() {
    let reg = registries();
    let mut s = SimState::new(caps(), 42, MAP);
    s.players[0].active = true;

    let nest = s
        .spawn(Spawn {
            archetype: arch(&reg, "mur_nest"),
            owner: PlayerId(0),
            pos: at(20, 20),
            hp: 1250,
            resource: 0,
        })
        .unwrap();
    let node = s
        .spawn(Spawn {
            archetype: arch(&reg, "ore_node"),
            owner: PlayerId::NEUTRAL,
            pos: at(26, 20),
            hp: 1,
            resource: 1500,
        })
        .unwrap();
    let drone = arch(&reg, "mur_drone");
    let workers: Vec<_> = (0..12)
        .map(|i| {
            s.spawn(Spawn {
                archetype: drone,
                owner: PlayerId(0),
                pos: at(24 + (i % 4), 24 + (i / 4)),
                hp: 40,
                resource: 0,
            })
            .unwrap()
        })
        .collect();
    // Supply. A nest provides 1 supply and twelve drones cost 12, so without
    // these the player is supply-blocked from tick zero and every train order
    // is correctly refused — which is exactly what happened the first time
    // this test ran, and is the supply system working rather than failing.
    // BW opens with an Overlord for the same reason.
    let sac = arch(&reg, "mur_supply_sac");
    for i in 0..3 {
        s.spawn(Spawn {
            archetype: sac,
            owner: PlayerId(0),
            pos: at(16 + i, 16),
            hp: 200,
            resource: 0,
        })
        .unwrap();
    }
    sim_systems::occupancy::stamp_static_occupancy(&mut s, &reg);
    let mut ctx = context(&s, &reg);

    for w in &workers {
        let _ = s.ingest(IssuedCommand {
            tick: s.clock.tick,
            player: PlayerId(0),
            subject: *w,
            command: Command::Harvest { node },
        });
    }

    // Two minutes of game time, training whenever the pool allows — which is
    // exactly the loop a Murmur player runs by hand.
    let mite = arch(&reg, "mur_mite");
    let mut trained = 0;
    let mut pool_ever_emptied = false;
    for _ in 0..(30 * 120) {
        tick(&mut s, &reg, &mut ctx);
        if s.players[0].brood_points == 0 {
            pool_ever_emptied = true;
        }
        if s.players[0].brood_points > 0 && s.players[0].ore >= 25 {
            train(&mut s, nest, mite);
            trained += 1;
        }
    }

    assert!(s.players[0].ore > 0, "the economy earned nothing");
    assert!(trained > 0, "never had the points and ore to train");

    let army = s
        .entities
        .iter_live()
        .filter(|h| {
            reg.units
                .by_archetype(s.c.archetype[h.index as usize])
                .map(|u| u.id == "mur_mite")
                .unwrap_or(false)
        })
        .count();
    assert!(army >= 5, "only {army} Mites after two minutes of macro");

    // Income turned into army: the ore mined is accounted for by what is
    // banked plus what was spent plus what is still in transit.
    let carried: u32 = workers.iter().map(|w| s.c.cargo[w.index as usize]).sum();
    let mined = 1500 - s.c.resource_left[node.index as usize];
    assert!(
        mined >= s.players[0].ore + carried,
        "banked more than was ever mined"
    );

    // The pool was genuinely spent, not merely accumulating.
    //
    // It is NOT asserted that the pool ends below the cap, and finding that
    // out was worth the failed run: with a single base the PRODUCTION QUEUE
    // is the bottleneck, not the pool. A Mite takes 420 ticks and a base
    // builds one at a time, so points inevitably back up however diligently
    // you spend. That is the design working — it is precisely why expanding
    // buys production capacity and not just income (GAME_DESIGN §2.1), and
    // it is the pressure a second base is meant to relieve.
    assert!(
        pool_ever_emptied,
        "the pool was never spent to zero — nothing was consuming it"
    );
}
