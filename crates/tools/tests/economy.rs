//! The harvest loop, end to end. GAME_DESIGN §1, IMPLEMENTATION_PLAN Phase 6.
//!
//! # Why this lives in `tools` and not in `sim_systems`
//! §1.3 lets `sim_systems` depend on `sim_core` and nothing else, and a
//! dev-dependency on `sim_content` would erode that by the back door — it
//! would appear in the dependency graph `cargo deny` checks, and the next
//! person to need "just one more crate, only for tests" would have a
//! precedent. `tools` is already allowed both, so integration tests that need
//! real content belong here.
//!
//! The Phase 6 gate asks for "12 workers saturate a base". These tests build
//! toward that by pinning the properties the macro rhythm depends on — most
//! importantly that saturation *emerges from slot contention* rather than from
//! a scripted curve, which is what makes it tunable by feel later.

use sim_content::{steering_from_yaml, units_from_yaml};
use sim_core::registry::Role;
use sim_core::{
    ArchetypeId, Capacities, Command, EntityHandle, IssuedCommand, PlayerId, Registries, SimState,
    Spawn, Tile, UnitState,
};
use sim_systems::{SimContext, tick};
use trilateral_fixed::{Fixed, FixedVec2};

const UNITS_YAML: &str = include_str!("../../../assets/data/units.yaml");
const STEERING_YAML: &str = include_str!("../../../assets/data/steering.yaml");
const MAP: u16 = 64;

fn caps() -> Capacities {
    Capacities {
        max_entities: 128,
        max_projectiles: 8,
        max_commands_per_tick: 128,
        max_players: 4,
        cmd_queue_slots: 16,
        modifier_slots: 8,
        command_log_reserve: 512,
    }
}

/// The real content, so these tests measure the actual game's numbers.
fn registries() -> Registries {
    Registries {
        units: units_from_yaml(UNITS_YAML).expect("units.yaml"),
        steering: steering_from_yaml(STEERING_YAML).expect("steering.yaml"),
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

fn spawn(
    s: &mut SimState,
    a: ArchetypeId,
    owner: u8,
    x: i32,
    y: i32,
    resource: u32,
) -> EntityHandle {
    s.spawn(Spawn {
        archetype: a,
        owner: PlayerId(owner),
        pos: at(x, y),
        hp: 100,
        resource,
    })
    .unwrap()
}

/// A base at (20,20) with one Ore node three tiles away, and `workers` drones.
fn base_scene(reg: &Registries, workers: i32) -> (SimState, Vec<EntityHandle>, EntityHandle) {
    let mut s = SimState::new(caps(), 42, MAP);
    s.players[0].active = true;
    spawn(&mut s, arch(reg, "mur_nest"), 0, 20, 20, 0);
    let node = spawn(&mut s, arch(reg, "ore_node"), u8::MAX, 25, 20, 1500);
    let drone = arch(reg, "mur_drone");
    let hs: Vec<_> = (0..workers)
        .map(|i| spawn(&mut s, drone, 0, 18 + (i % 4), 22 + (i / 4), 0))
        .collect();
    sim_systems::occupancy::stamp_static_occupancy(&mut s, reg);
    (s, hs, node)
}

fn order_harvest(s: &mut SimState, h: EntityHandle, node: EntityHandle) {
    s.ingest(IssuedCommand {
        tick: s.clock.tick,
        player: PlayerId(0),
        subject: h,
        command: Command::Harvest { node },
    })
    .expect("harvest command should validate");
}

fn context(s: &SimState, reg: &Registries) -> SimContext {
    SimContext::new(MAP as i32, reg, s.c.capacity(), s.grid.tile_count())
}

fn run(s: &mut SimState, reg: &Registries, ctx: &mut SimContext, ticks: u32) {
    for _ in 0..ticks {
        tick(s, reg, ctx);
    }
}

// ---------------------------------------------------------------------------

#[test]
fn the_content_defines_a_base_to_deposit_at() {
    let reg = registries();
    let nest = reg.units.get("mur_nest").expect("mur_nest missing");
    assert_eq!(nest.role, Role::Base);
    assert_eq!(nest.hp, 1250, "BW Hatchery");
}

#[test]
fn a_single_worker_completes_the_loop_and_banks_ore() {
    let reg = registries();
    let (mut s, hs, node) = base_scene(&reg, 1);
    let mut ctx = context(&s, &reg);
    order_harvest(&mut s, hs[0], node);

    // A trip is 60 ticks of mining plus travel each way; 400 is comfortably
    // more than one full cycle at BW drone speed over three tiles.
    run(&mut s, &reg, &mut ctx, 900);

    assert!(s.players[0].ore > 0, "worker banked nothing in 900 ticks");
    assert!(
        s.c.resource_left[node.index as usize] < 1500,
        "the node was never mined"
    );
    assert_eq!(
        s.players[0].ore + s.c.cargo[hs[0].index as usize],
        1500 - s.c.resource_left[node.index as usize],
        "ore appeared or vanished — the loop is not conserving"
    );
}

#[test]
fn ore_is_conserved_exactly_over_a_long_run() {
    // The property that matters most: this is a closed system. Every unit of
    // ore is in the node, in a worker, or in the bank — never anywhere else,
    // and never counted twice.
    let reg = registries();
    let (mut s, hs, node) = base_scene(&reg, 6);
    let mut ctx = context(&s, &reg);
    for h in &hs {
        order_harvest(&mut s, *h, node);
    }
    for _ in 0..40 {
        run(&mut s, &reg, &mut ctx, 100);
        let carried: u32 = hs.iter().map(|h| s.c.cargo[h.index as usize]).sum();
        assert_eq!(
            s.players[0].ore + carried + s.c.resource_left[node.index as usize],
            1500,
            "ore was created or destroyed at tick {}",
            s.clock.tick.0
        );
    }
}

#[test]
fn saturation_emerges_from_slot_contention_rather_than_a_curve() {
    // GAME_DESIGN §1.1: no scripted diminishing returns. An Ore node has two
    // harvest slots, so the third and fourth workers must add much less than
    // the first two — purely because they spend part of each cycle waiting.
    let reg = registries();
    let income = |workers: i32| -> u32 {
        let (mut s, hs, node) = base_scene(&reg, workers);
        let mut ctx = context(&s, &reg);
        for h in &hs {
            order_harvest(&mut s, *h, node);
        }
        run(&mut s, &reg, &mut ctx, 1800); // one minute of game time
        s.players[0].ore
    };

    let one = income(1);
    let two = income(2);
    let four = income(4);

    assert!(one > 0, "a single worker earned nothing");
    assert!(two > one, "the second worker added nothing: {one} -> {two}");

    // Two slots: doubling from two to four workers must NOT double income.
    let marginal_first = two.saturating_sub(one);
    let marginal_rest = four.saturating_sub(two);
    assert!(
        marginal_rest < marginal_first * 2,
        "workers 3-4 added {marginal_rest} against {marginal_first} for worker 2 \
         — slot contention is not biting, so saturation is not emerging"
    );
}

#[test]
fn a_mined_out_node_stands_its_workers_down() {
    let reg = registries();
    let mut s = SimState::new(caps(), 7, MAP);
    s.players[0].active = true;
    spawn(&mut s, arch(&reg, "mur_nest"), 0, 20, 20, 0);
    // Only 8 ore: one trip's worth.
    let node = spawn(&mut s, arch(&reg, "ore_node"), u8::MAX, 25, 20, 8);
    let h = spawn(&mut s, arch(&reg, "mur_drone"), 0, 21, 20, 0);
    sim_systems::occupancy::stamp_static_occupancy(&mut s, &reg);
    let mut ctx = context(&s, &reg);
    order_harvest(&mut s, h, node);

    run(&mut s, &reg, &mut ctx, 800);
    assert_eq!(
        s.c.resource_left[node.index as usize], 0,
        "node not emptied"
    );
    assert_eq!(s.players[0].ore, 8, "the last partial load was lost");
    assert_eq!(
        s.c.state[h.index as usize],
        UnitState::Idle,
        "worker kept mining an empty node"
    );
}

#[test]
fn stop_cancels_harvesting_rather_than_pausing_it() {
    let reg = registries();
    let (mut s, hs, node) = base_scene(&reg, 1);
    let mut ctx = context(&s, &reg);
    order_harvest(&mut s, hs[0], node);
    run(&mut s, &reg, &mut ctx, 120);

    s.ingest(IssuedCommand {
        tick: s.clock.tick,
        player: PlayerId(0),
        subject: hs[0],
        command: Command::Stop,
    })
    .unwrap();
    run(&mut s, &reg, &mut ctx, 5);
    assert_eq!(s.c.state[hs[0].index as usize], UnitState::Idle);
    let banked = s.players[0].ore;
    run(&mut s, &reg, &mut ctx, 600);
    assert_eq!(s.players[0].ore, banked, "a stopped worker resumed mining");
}

#[test]
fn flux_banks_separately_from_ore() {
    // Two resources is the whole economic design (GAME_DESIGN §1.2). A worker
    // carrying Flux must not bank Ore.
    let reg = registries();
    let mut s = SimState::new(caps(), 3, MAP);
    s.players[0].active = true;
    spawn(&mut s, arch(&reg, "mur_nest"), 0, 20, 20, 0);
    let fissure = spawn(&mut s, arch(&reg, "flux_fissure"), u8::MAX, 25, 20, 2500);
    let h = spawn(&mut s, arch(&reg, "mur_drone"), 0, 21, 20, 0);
    sim_systems::occupancy::stamp_static_occupancy(&mut s, &reg);
    let mut ctx = context(&s, &reg);
    order_harvest(&mut s, h, fissure);

    run(&mut s, &reg, &mut ctx, 600);
    assert!(s.players[0].flux > 0, "no flux banked");
    assert_eq!(s.players[0].ore, 0, "flux was banked as ore");
}

#[test]
fn a_worker_whose_base_dies_holds_its_cargo() {
    let reg = registries();
    let (mut s, hs, node) = base_scene(&reg, 1);
    let mut ctx = context(&s, &reg);
    order_harvest(&mut s, hs[0], node);
    // Long enough to be carrying something.
    run(&mut s, &reg, &mut ctx, 200);
    let base = s
        .entities
        .iter_live()
        .find(|h| {
            reg.units
                .by_archetype(s.c.archetype[h.index as usize])
                .map(|u| u.role == Role::Base)
                .unwrap_or(false)
        })
        .unwrap();
    s.despawn(base);
    // Whatever was banked BEFORE the base died is legitimately banked. What
    // must not happen is banking after, into a base that is not there.
    let banked_before = s.players[0].ore;
    run(&mut s, &reg, &mut ctx, 600);
    assert_eq!(
        s.players[0].ore, banked_before,
        "banked ore with no base standing"
    );
}

#[test]
fn the_economy_is_deterministic() {
    fn run_once() -> u64 {
        let reg = registries();
        let (mut s, hs, node) = base_scene(&reg, 12);
        let mut ctx = context(&s, &reg);
        for h in &hs {
            order_harvest(&mut s, *h, node);
        }
        run(&mut s, &reg, &mut ctx, 1200);
        s.hash()
    }
    assert_eq!(run_once(), run_once());
}

#[test]
fn harvesting_across_terrain_still_works() {
    // The loop has to survive the pathfinder: a node behind a wall means the
    // worker walks around it every trip.
    let reg = registries();
    let mut s = SimState::new(caps(), 11, MAP);
    s.players[0].active = true;
    spawn(&mut s, arch(&reg, "mur_nest"), 0, 10, 20, 0);
    for y in 10..30u16 {
        if y != 29 {
            s.grid.set_walkable(Tile::new(16, y), false);
        }
    }
    let node = spawn(&mut s, arch(&reg, "ore_node"), u8::MAX, 22, 20, 1500);
    let h = spawn(&mut s, arch(&reg, "mur_drone"), 0, 11, 20, 0);
    sim_systems::occupancy::stamp_static_occupancy(&mut s, &reg);
    let mut ctx = context(&s, &reg);
    order_harvest(&mut s, h, node);

    run(&mut s, &reg, &mut ctx, 3000);
    assert!(
        s.players[0].ore > 0,
        "no ore banked when the node was behind a wall"
    );
}
