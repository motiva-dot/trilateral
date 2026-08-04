//! Command execution and movement integration. §3.2 steps 2 and 14.
//!
//! # This is the file the whole "feel" argument runs through
//! Nothing here is clever yet — no pathfinding, no avoidance, no formation.
//! A unit told to move points at its destination and integrates. Phase 4 puts
//! HPA*, flow fields and velocity-obstacle steering underneath, and the
//! StarCraft-standard bar (IMPLEMENTATION_PLAN Feel Checkpoint 1) is judged
//! there. What must already be right here is the part Phase 4 cannot fix
//! afterwards: exact arrival, no overshoot, no jitter at the destination.
//!
//! # Arrival, and why it is a `<=` and not an epsilon
//! A unit whose remaining distance is at or below one tick of movement is
//! placed *exactly* on its destination and set Idle. No epsilon, no "close
//! enough" band. An epsilon would be a tuning constant that differs between
//! unit sizes and, worse, a value two clients could compare differently at the
//! boundary. Exact placement means a stopped unit's position is a function of
//! its order, not of how many ticks it happened to take getting there.

use sim_core::components::UnitState;
use sim_core::{Command, Registries, SimState};
use trilateral_fixed::{Fixed, FixedVec2};

/// §3.2 step 2. Turn commands scheduled for this tick into state transitions.
///
/// Iterates the log in append order and applies only entries stamped for the
/// current tick — §5.1 schedules commands at `issue_tick + input_delay` so
/// every client executes them on the same tick.
pub fn command_execution(state: &mut SimState, _reg: &Registries) {
    let now = state.clock.tick;
    // Collect first: the log is borrowed immutably while we mutate components.
    // Bounded by max_commands_per_tick per player, so this is small; it will
    // become a per-entity CmdQueue drain when CmdQueue lands.
    let due: Vec<_> = state
        .cmd_log
        .for_tick(now)
        .map(|c| (c.subject, c.command))
        .collect();

    for (subject, command) in due {
        // Re-check liveness: the command was valid at ingest, but that was up
        // to input_delay ticks ago and the subject may have died since.
        if !state.is_alive(subject) {
            continue;
        }
        let i = subject.index as usize;
        match command {
            Command::Move { target } => {
                state.c.dest[i] = target;
                state.c.state[i] = UnitState::Moving;
            }
            Command::Stop | Command::HoldPosition => {
                state.c.dest[i] = FixedVec2::ZERO;
                state.c.vel[i] = FixedVec2::ZERO;
                state.c.state[i] = UnitState::Idle;
            }
            // Every other variant belongs to a system that does not exist yet.
            // Deliberately ignored rather than partially implemented: a command
            // that half-works is worse than one that visibly does nothing.
            _ => {}
        }
    }
}

/// §3.2 step 14. Integrate velocity into position.
///
/// Ascending entity index, always (§6.9). Units with no `move_speed` in the
/// registry — resource nodes, buildings — cannot move regardless of state.
pub fn movement(state: &mut SimState, reg: &Registries) {
    for i in 0..state.c.capacity() {
        if !state.c.alive.get(i) {
            continue;
        }
        let n = i as usize;
        if state.c.state[n] != UnitState::Moving {
            continue;
        }

        let speed = match reg.units.by_archetype(state.c.archetype[n]) {
            Some(u) => u.move_speed,
            // Unknown archetype: refuse to move rather than guess a speed. A
            // corrupt replay should produce a stationary unit, not one moving
            // at some default nobody chose.
            None => continue,
        };
        if speed <= Fixed::ZERO {
            state.c.state[n] = UnitState::Idle;
            continue;
        }

        let to_target = state.c.dest[n] - state.c.pos[n];
        let remaining = to_target.length();

        if remaining <= speed {
            // Arrive exactly. See the module note on why this is not an epsilon.
            state.c.pos[n] = state.c.dest[n];
            state.c.vel[n] = FixedVec2::ZERO;
            state.c.state[n] = UnitState::Idle;
            state.c.dest[n] = FixedVec2::ZERO;
            continue;
        }

        let step = to_target.normalize().scale(speed);
        state.c.vel[n] = step;
        state.c.pos[n] += step;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SimContext, tick};
    use sim_core::registry::{Role, Shape, UnitCost, UnitRegistry, UnitStats};
    use sim_core::{ArchetypeId, Capacities, EntityHandle, IssuedCommand, PlayerId, Spawn, Tick};

    fn caps() -> Capacities {
        Capacities {
            max_entities: 64,
            max_projectiles: 8,
            max_commands_per_tick: 16,
            max_players: 4,
            cmd_queue_slots: 16,
            modifier_slots: 8,
            command_log_reserve: 64,
        }
    }

    /// One archetype whose speed is exactly 1/4 tile per tick, so expected
    /// positions are exact dyadic rationals and the assertions can be exact
    /// rather than approximate (OPERATIONS §3.3).
    fn registries() -> Registries {
        Registries {
            units: UnitRegistry::new(vec![UnitStats {
                id: "runner".into(),
                faction: "test".into(),
                role: Role::Melee,
                shape: Shape::Circle,
                cost: UnitCost {
                    ore: 1,
                    flux: 0,
                    ticks: 1,
                },
                supply_x2: 2,
                provides_supply_x2: 0,
                hp: 10,
                armor: 0,
                collider_radius: Fixed::from_ratio(1, 4),
                mass: 1,
                move_speed: Fixed::from_ratio(1, 4),
                turn_rate: Fixed::from_ratio(1, 10),
                sight_range: Fixed::from_int(5),
                selection_weight: 1,
                attack: None,
                resource: None,
                tags: vec![],
            }]),
        }
    }

    fn spawn_runner(s: &mut SimState, x: i32, y: i32) -> EntityHandle {
        s.spawn(Spawn {
            archetype: ArchetypeId(0),
            owner: PlayerId(0),
            pos: FixedVec2::from_ints(x, y),
            hp: 10,
        })
        .unwrap()
    }

    fn order_move(s: &mut SimState, h: EntityHandle, at: Tick, x: i32, y: i32) {
        s.ingest(IssuedCommand {
            tick: at,
            player: PlayerId(0),
            subject: h,
            command: Command::Move {
                target: FixedVec2::from_ints(x, y),
            },
        })
        .unwrap();
    }

    // ---- THE PHASE 3 GATE -------------------------------------------------

    #[test]
    fn gate_sixty_ticks_of_straight_line_motion_is_exact() {
        // IMPLEMENTATION_PLAN Phase 3: "60-tick straight-line motion asserts
        // exact fixed values". Speed 1/4 tile/tick, so after 60 ticks the unit
        // has travelled exactly 15 tiles — no epsilon anywhere in this test.
        let reg = registries();
        let mut s = SimState::new(caps(), 1);
        let mut ctx = SimContext::new(128, &reg);
        let h = spawn_runner(&mut s, 0, 0);
        order_move(&mut s, h, Tick(0), 100, 0);

        for _ in 0..60 {
            tick(&mut s, &reg, &mut ctx);
        }
        let p = s.c.pos[h.index as usize];
        assert_eq!(p.x, Fixed::from_int(15), "x after 60 ticks");
        assert_eq!(p.y, Fixed::ZERO, "must not drift off-axis");
        assert_eq!(s.c.state[h.index as usize], UnitState::Moving);
    }

    #[test]
    fn arrival_is_exact_and_the_unit_stops() {
        let reg = registries();
        let mut s = SimState::new(caps(), 1);
        let mut ctx = SimContext::new(128, &reg);
        let h = spawn_runner(&mut s, 0, 0);
        order_move(&mut s, h, Tick(0), 2, 0);

        // 2 tiles at 1/4 tile per tick = exactly 8 ticks.
        for _ in 0..8 {
            tick(&mut s, &reg, &mut ctx);
        }
        let n = h.index as usize;
        assert_eq!(s.c.pos[n], FixedVec2::from_ints(2, 0), "arrived exactly");
        assert_eq!(s.c.state[n], UnitState::Idle, "should have stopped");
        assert_eq!(s.c.vel[n], FixedVec2::ZERO);
        assert_eq!(
            s.c.dest[n],
            FixedVec2::ZERO,
            "stale destination left behind"
        );
    }

    #[test]
    fn an_arrived_unit_does_not_jitter() {
        // The failure this guards: a unit that oscillates around its target
        // forever because each tick overshoots and the next corrects back.
        let reg = registries();
        let mut s = SimState::new(caps(), 1);
        let mut ctx = SimContext::new(128, &reg);
        let h = spawn_runner(&mut s, 0, 0);
        order_move(&mut s, h, Tick(0), 1, 0);
        for _ in 0..20 {
            tick(&mut s, &reg, &mut ctx);
        }
        let settled = s.c.pos[h.index as usize];
        for _ in 0..50 {
            tick(&mut s, &reg, &mut ctx);
            assert_eq!(s.c.pos[h.index as usize], settled, "unit jittered");
        }
    }

    #[test]
    fn a_destination_closer_than_one_step_does_not_overshoot() {
        let reg = registries();
        let mut s = SimState::new(caps(), 1);
        let mut ctx = SimContext::new(128, &reg);
        let h = spawn_runner(&mut s, 0, 0);
        // 1/8 tile away, half of one tick's travel.
        s.ingest(IssuedCommand {
            tick: Tick(0),
            player: PlayerId(0),
            subject: h,
            command: Command::Move {
                target: FixedVec2::new(Fixed::from_ratio(1, 8), Fixed::ZERO),
            },
        })
        .unwrap();
        tick(&mut s, &reg, &mut ctx);
        assert_eq!(s.c.pos[h.index as usize].x, Fixed::from_ratio(1, 8));
        assert_eq!(s.c.state[h.index as usize], UnitState::Idle);
    }

    #[test]
    fn stop_halts_a_moving_unit_where_it_stands() {
        let reg = registries();
        let mut s = SimState::new(caps(), 1);
        let mut ctx = SimContext::new(128, &reg);
        let h = spawn_runner(&mut s, 0, 0);
        order_move(&mut s, h, Tick(0), 100, 0);
        for _ in 0..4 {
            tick(&mut s, &reg, &mut ctx);
        }
        let at_stop = s.c.pos[h.index as usize];
        assert_eq!(at_stop.x, Fixed::ONE, "4 ticks at 1/4 tile");

        s.ingest(IssuedCommand {
            tick: s.clock.tick,
            player: PlayerId(0),
            subject: h,
            command: Command::Stop,
        })
        .unwrap();
        tick(&mut s, &reg, &mut ctx);
        assert_eq!(s.c.state[h.index as usize], UnitState::Idle);
        assert_eq!(s.c.pos[h.index as usize], at_stop, "moved after Stop");

        for _ in 0..10 {
            tick(&mut s, &reg, &mut ctx);
            assert_eq!(s.c.pos[h.index as usize], at_stop);
        }
    }

    #[test]
    fn a_command_scheduled_for_a_future_tick_waits_for_it() {
        // §5.1: commands execute at issue_tick + input_delay, not on arrival.
        let reg = registries();
        let mut s = SimState::new(caps(), 1);
        let mut ctx = SimContext::new(128, &reg);
        let h = spawn_runner(&mut s, 0, 0);
        order_move(&mut s, h, Tick(5), 100, 0);

        for _ in 0..5 {
            tick(&mut s, &reg, &mut ctx);
            assert_eq!(
                s.c.pos[h.index as usize],
                FixedVec2::ZERO,
                "moved before its scheduled tick"
            );
        }
        tick(&mut s, &reg, &mut ctx);
        assert_eq!(s.c.pos[h.index as usize].x, Fixed::from_ratio(1, 4));
    }

    #[test]
    fn a_unit_that_dies_before_its_command_executes_is_skipped() {
        let reg = registries();
        let mut s = SimState::new(caps(), 1);
        let mut ctx = SimContext::new(128, &reg);
        let h = spawn_runner(&mut s, 0, 0);
        order_move(&mut s, h, Tick(3), 50, 0);
        s.despawn(h);
        for _ in 0..6 {
            tick(&mut s, &reg, &mut ctx); // must not panic on a dead subject
        }
        assert!(!s.is_alive(h));
    }

    #[test]
    fn an_unknown_archetype_refuses_to_move_rather_than_guessing() {
        // A corrupt or hostile replay can name any ArchetypeId. The unit should
        // stand still, not move at some default speed nobody chose.
        let reg = registries();
        let mut s = SimState::new(caps(), 1);
        let mut ctx = SimContext::new(128, &reg);
        let h = s
            .spawn(Spawn {
                archetype: ArchetypeId(999),
                owner: PlayerId(0),
                pos: FixedVec2::ZERO,
                hp: 10,
            })
            .unwrap();
        order_move(&mut s, h, Tick(0), 100, 0);
        for _ in 0..10 {
            tick(&mut s, &reg, &mut ctx);
        }
        assert_eq!(s.c.pos[h.index as usize], FixedVec2::ZERO);
    }

    #[test]
    fn diagonal_movement_covers_the_right_distance() {
        // Normalize then scale: a diagonal move must not travel sqrt(2) times
        // too far, which is the classic bug when a direction is not normalised.
        let reg = registries();
        let mut s = SimState::new(caps(), 1);
        let mut ctx = SimContext::new(128, &reg);
        let h = spawn_runner(&mut s, 0, 0);
        order_move(&mut s, h, Tick(0), 100, 100);
        for _ in 0..40 {
            tick(&mut s, &reg, &mut ctx);
        }
        // 40 ticks at 1/4 tile = 10 tiles of travel along the diagonal.
        let travelled = s.c.pos[h.index as usize].length();
        let err = (travelled - Fixed::from_int(10)).abs();
        assert!(
            err <= Fixed::from_ratio(1, 100),
            "travelled {travelled} tiles, expected 10"
        );
    }

    #[test]
    fn many_movers_stay_hash_equal_across_identical_runs() {
        // Phase 3 gate: "two identical runs hash-equal", with movement actually
        // happening rather than an empty state ticking.
        fn run() -> u64 {
            let reg = registries();
            let mut s = SimState::new(caps(), 42);
            let mut ctx = SimContext::new(128, &reg);
            for i in 0..40i32 {
                let h = spawn_runner(&mut s, i % 8, i / 8);
                s.ingest(IssuedCommand {
                    tick: Tick((i % 5) as u64),
                    player: PlayerId(0),
                    subject: h,
                    command: Command::Move {
                        target: FixedVec2::from_ints(20 + (i % 3), 20 - (i % 4)),
                    },
                })
                .unwrap();
            }
            for _ in 0..200 {
                tick(&mut s, &reg, &mut ctx);
            }
            s.hash()
        }
        assert_eq!(run(), run());
    }
}
