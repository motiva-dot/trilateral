//! Local avoidance. §3.2 step 13, TECH_SPEC §5.2.
//!
//! The first play session's verdict on Phase 3.5 was "they didn't collide",
//! and that is exactly what this file is for. Units that pass through each
//! other read as 500 independent dots; units that displace each other read as
//! an army. Nothing else in Phase 4 changes that as much.
//!
//! # Two-pass, and why it has to be
//! Pass one reads every position and accumulates a displacement per entity
//! into a scratch buffer. Pass two applies. A single-pass version — resolve a
//! pair, write immediately, move on — would make the result depend on
//! iteration order, so unit 3 shoving unit 7 would produce a different world
//! than 7 shoving 3. That is fine until two clients disagree about the order,
//! at which point it is a desync. Two passes make the tick a pure function of
//! the state at its start.
//!
//! # Mass, not equality
//! §5.2: heavier shoves lighter. Displacement splits by the *other* unit's
//! share of the combined mass, so a Mite bounces off a Breaker rather than the
//! two meeting in the middle. This is a feel decision as much as a physical
//! one — it is what makes big units feel big.
//!
//! # What this is not
//! Not velocity obstacles. Not formations. Not pathfinding. This is positional
//! separation: the cheapest thing that makes a crowd behave like a crowd. RVO
//! and group anchors arrive later in Phase 4 once there is a map to path
//! across; the settle rule below is what keeps a jam from becoming a vibration
//! in the meantime.

use sim_core::{Registries, SimState};
use trilateral_fixed::{Fixed, FixedVec2};

use crate::SimContext;

/// Fill `ctx.separation` with the displacement each entity owes to its
/// neighbours this tick. Applied by `movement`, which is the single place
/// positions are written.
pub fn steering(state: &SimState, reg: &Registries, ctx: &mut SimContext) {
    for s in ctx.separation.iter_mut() {
        *s = FixedVec2::ZERO;
    }
    let p = reg.steering;
    let cap = state.c.capacity();

    for i in 0..cap {
        if !state.c.alive.get(i) {
            continue;
        }
        let ri = radius_of(state, reg, i);
        if ri <= Fixed::ZERO {
            continue;
        }
        let mass_i = mass_of(state, reg, i);

        // Query a neighbourhood wide enough for the largest possible overlap.
        let reach = ri + reg.units.max_collider_radius();
        ctx.spatial
            .query_square(state.c.pos[i as usize], reach, &mut ctx.query_scratch);

        let mut resolved = 0u16;
        for k in 0..ctx.query_scratch.len() {
            let j = ctx.query_scratch[k];
            // Each pair once. Also skips self. Because the query is ascending,
            // "the first `max_neighbours` with j > i" is a deterministic set,
            // not whichever ones happened to be nearby in memory.
            if j <= i {
                continue;
            }
            if resolved >= p.max_neighbours {
                break;
            }
            if !state.c.alive.get(j) {
                continue;
            }
            let rj = radius_of(state, reg, j);
            if rj <= Fixed::ZERO {
                continue;
            }

            let delta = state.c.pos[j as usize] - state.c.pos[i as usize];
            let sum_r = ri + rj;
            // Compare squared, widened: no square root in the rejection test,
            // which is the overwhelmingly common case.
            let sum_r_sq = ((sum_r.to_bits() as i128) * (sum_r.to_bits() as i128)) >> 32;
            if delta.length_sq_wide() >= sum_r_sq {
                continue;
            }
            resolved += 1;

            let dist = delta.length();
            let axis = if dist.is_zero() {
                // Exactly co-located. Any answer will do EXCEPT an arbitrary
                // one — two clients must pick the same escape direction. Derive
                // it from the pair's indices, which both clients agree on.
                if (i + j).is_multiple_of(2) {
                    FixedVec2::X
                } else {
                    FixedVec2::Y
                }
            } else {
                delta.normalize()
            };

            let penetration = sum_r - dist;
            let total = penetration.mul(p.separation_response);
            let mass_j = mass_of(state, reg, j);
            // Each unit takes the share proportional to the OTHER's mass:
            // heavier j, more of the correction lands on i.
            //
            // Both shares are computed independently rather than deriving the
            // second as `total - move_i`. The subtraction looks tidier and
            // conserves the total exactly, but it makes equal-mass pairs move
            // by amounts differing by one ULP — and a pair that pushes itself
            // asymmetrically every tick acquires net drift, so a crowd never
            // converges and never settles. Symmetry matters more here than
            // conserving the last bit of the correction.
            let denom = ((mass_i as i64) + (mass_j as i64)).max(1);
            let move_i = total.mul(Fixed::from_ratio(mass_j as i64, denom));
            let move_j = total.mul(Fixed::from_ratio(mass_i as i64, denom));

            ctx.separation[i as usize] -= axis.scale(move_i);
            ctx.separation[j as usize] += axis.scale(move_j);
        }
    }
}

#[inline]
fn radius_of(state: &SimState, reg: &Registries, i: u32) -> Fixed {
    reg.units
        .by_archetype(state.c.archetype[i as usize])
        .map(|u| u.collider_radius)
        .unwrap_or(Fixed::ZERO)
}

#[inline]
fn mass_of(state: &SimState, reg: &Registries, i: u32) -> u16 {
    reg.units
        .by_archetype(state.c.archetype[i as usize])
        .map(|u| u.mass.max(1))
        .unwrap_or(1)
}

/// Update the settle counter for one unit, given how far it actually got.
///
/// Returns true if the unit should give up and go Idle.
///
/// This is the rule that stops a crowd vibrating. A unit jammed against idle
/// friends near its destination will otherwise push, be pushed back, and
/// repeat forever — visually the most recognisable failure of naive RTS
/// steering, and one that no amount of tuning the separation strength fixes.
pub fn update_settle(
    state: &mut SimState,
    reg: &Registries,
    i: u32,
    actual_progress: Fixed,
    speed: Fixed,
) -> bool {
    let p = reg.steering;
    let n = i as usize;
    let threshold = speed.mul(p.settle_progress_fraction);
    if actual_progress < threshold {
        state.c.stuck_ticks[n] = state.c.stuck_ticks[n].saturating_add(1);
    } else {
        state.c.stuck_ticks[n] = 0;
    }
    state.c.stuck_ticks[n] >= p.settle_stuck_ticks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tick;
    use sim_core::registry::{Role, Shape, UnitCost, UnitRegistry, UnitStats};
    use sim_core::{ArchetypeId, Capacities, PlayerId, SimState, Spawn};

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

    fn unit(id: &str, radius_hundredths: i64, mass: u16) -> UnitStats {
        UnitStats {
            id: id.into(),
            faction: "t".into(),
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
            collider_radius: Fixed::from_ratio(radius_hundredths, 100),
            mass,
            move_speed: Fixed::from_ratio(1, 4),
            turn_rate: Fixed::from_ratio(1, 10),
            sight_range: Fixed::from_int(5),
            selection_weight: 1,
            attack: None,
            resource: None,
            tags: vec![],
        }
    }

    /// Archetype 0: light, radius 0.5, mass 1. Archetype 1: heavy, mass 20.
    fn registries() -> Registries {
        Registries {
            units: UnitRegistry::new(vec![unit("light", 50, 1), unit("heavy", 50, 20)]),
            steering: sim_core::SteeringParams::default(),
        }
    }

    fn spawn(s: &mut SimState, arch: u16, x: i64, y: i64, denom: i64) {
        s.spawn(Spawn {
            archetype: ArchetypeId(arch),
            owner: PlayerId(0),
            pos: FixedVec2::new(Fixed::from_ratio(x, denom), Fixed::from_ratio(y, denom)),
            hp: 10,
        })
        .unwrap();
    }

    fn crowd(n: i64) -> (SimState, Registries, SimContext) {
        let reg = registries();
        let mut s = SimState::new(caps(), 1);
        for i in 0..n {
            spawn(&mut s, 0, 20 + i % 3, 20 + i % 2, 1);
        }
        let ctx = ctx_for(&s, &reg);
        (s, reg, ctx)
    }

    fn ctx_for(state: &SimState, reg: &Registries) -> SimContext {
        let mut ctx = SimContext::new(128, reg, state.c.capacity());
        ctx.spatial.rebuild(state);
        ctx
    }

    #[test]
    fn separated_units_are_left_alone() {
        let reg = registries();
        let mut s = SimState::new(caps(), 1);
        spawn(&mut s, 0, 0, 0, 1);
        spawn(&mut s, 0, 10, 0, 1);
        let mut ctx = ctx_for(&s, &reg);
        steering(&s, &reg, &mut ctx);
        assert_eq!(ctx.separation[0], FixedVec2::ZERO);
        assert_eq!(ctx.separation[1], FixedVec2::ZERO);
    }

    #[test]
    fn overlapping_units_push_apart_along_the_line_between_them() {
        let reg = registries();
        let mut s = SimState::new(caps(), 1);
        spawn(&mut s, 0, 0, 0, 1);
        spawn(&mut s, 0, 1, 0, 2); // 0.5 apart, radii sum 1.0 -> overlapping
        let mut ctx = ctx_for(&s, &reg);
        steering(&s, &reg, &mut ctx);
        assert!(
            ctx.separation[0].x < Fixed::ZERO,
            "left unit should go left"
        );
        assert!(
            ctx.separation[1].x > Fixed::ZERO,
            "right unit should go right"
        );
        assert_eq!(ctx.separation[0].y, Fixed::ZERO, "no sideways drift");
    }

    #[test]
    fn equal_masses_share_the_correction_equally() {
        let reg = registries();
        let mut s = SimState::new(caps(), 1);
        spawn(&mut s, 0, 0, 0, 1);
        spawn(&mut s, 0, 1, 0, 2);
        let mut ctx = ctx_for(&s, &reg);
        steering(&s, &reg, &mut ctx);
        let a = ctx.separation[0].x.abs();
        let b = ctx.separation[1].x.abs();
        assert_eq!(a, b, "equal masses moved unequally");
    }

    #[test]
    fn heavier_units_shove_lighter_ones() {
        // §5.2, and the reason big units feel big.
        let reg = registries();
        let mut s = SimState::new(caps(), 1);
        spawn(&mut s, 0, 0, 0, 1); // light, mass 1
        spawn(&mut s, 1, 1, 0, 2); // heavy, mass 20
        let mut ctx = ctx_for(&s, &reg);
        steering(&s, &reg, &mut ctx);
        let light = ctx.separation[0].x.abs();
        let heavy = ctx.separation[1].x.abs();
        assert!(
            light > heavy,
            "light moved {light}, heavy moved {heavy} — mass priority inverted"
        );
        // 20:1 mass ratio should be a large, not marginal, difference.
        assert!(light > heavy.mul(Fixed::from_int(5)));
    }

    #[test]
    fn exactly_co_located_units_still_separate_deterministically() {
        // dist == 0 has no meaningful axis. Any answer will do EXCEPT an
        // arbitrary one — both clients must choose the same escape direction.
        let reg = registries();
        let mut s = SimState::new(caps(), 1);
        spawn(&mut s, 0, 5, 5, 1);
        spawn(&mut s, 0, 5, 5, 1);
        let mut ctx = ctx_for(&s, &reg);
        steering(&s, &reg, &mut ctx);
        assert_ne!(ctx.separation[0], FixedVec2::ZERO, "stayed stacked forever");
        assert_eq!(ctx.separation[0], -ctx.separation[1], "not symmetric");

        // And identical inputs give the identical escape, twice.
        let mut ctx2 = ctx_for(&s, &reg);
        steering(&s, &reg, &mut ctx2);
        assert_eq!(ctx.separation[0], ctx2.separation[0]);
    }

    #[test]
    fn separation_is_symmetric_regardless_of_spawn_order() {
        // The property two-pass buys: pair resolution does not depend on which
        // unit the loop happened to reach first.
        let reg = registries();
        let mut a = SimState::new(caps(), 1);
        spawn(&mut a, 0, 0, 0, 1);
        spawn(&mut a, 0, 1, 0, 2);
        let mut ca = ctx_for(&a, &reg);
        steering(&a, &reg, &mut ca);

        let mut b = SimState::new(caps(), 1);
        spawn(&mut b, 0, 1, 0, 2);
        spawn(&mut b, 0, 0, 0, 1);
        let mut cb = ctx_for(&b, &reg);
        steering(&b, &reg, &mut cb);

        // Indices swap, magnitudes must not.
        assert_eq!(ca.separation[0].x.abs(), cb.separation[1].x.abs());
    }

    #[test]
    fn an_isolated_pair_acquires_no_net_drift() {
        // The invariant the one-ULP asymmetry violated, and the reason a crowd
        // would otherwise never converge: equal-mass units pushing each other
        // must move the pair's midpoint by exactly nothing, forever.
        let reg = registries();
        let mut s = SimState::new(caps(), 1);
        spawn(&mut s, 0, 0, 0, 1);
        spawn(&mut s, 0, 1, 0, 2);
        let midpoint = |st: &SimState| (st.c.pos[0] + st.c.pos[1]).scale(Fixed::from_ratio(1, 2));
        let before = midpoint(&s);
        let mut ctx = ctx_for(&s, &reg);
        for _ in 0..500 {
            tick(&mut s, &reg, &mut ctx);
        }
        assert_eq!(midpoint(&s), before, "the pair drifted");
    }

    #[test]
    fn a_crowd_reaches_exact_stillness_rather_than_vibrating_forever() {
        // THE failure this phase exists to prevent, and the property fixed
        // point buys that floats would not.
        //
        // A separation rule that resolves a fraction of each overlap per tick
        // converges geometrically. In floating point the residual would shrink
        // without ever reaching zero, so a settled crowd would keep twitching
        // at the last decimal place forever. In Q32.32 the correction
        // eventually rounds to exactly zero and the crowd stops — measurably,
        // bit-for-bit, permanently.
        //
        // Measured convergence from a 20-unit pile stacked on three points:
        // 37.4 tiles of total movement over ticks 0-200, 0.36 over 200-400,
        // 0.004 over 400-600, and exactly 0 from tick ~800 onward. The horizon
        // below is deliberately past that, not tuned to just scrape by.
        // NOTE FOR ANYONE WRITING A SIMILAR TEST: do not compare
        // `SimState::hash()` across a span of ticks. The hash folds
        // `clock.tick`, so it changes every tick by construction and can never
        // report "nothing happened". Compare the components you actually mean.
        // Getting this wrong once already produced a convincing false alarm.
        let (mut s, reg, mut ctx) = crowd(20);
        for _ in 0..1000 {
            tick(&mut s, &reg, &mut ctx);
        }
        let positions: Vec<_> = (0..s.c.capacity()).map(|i| s.c.pos[i as usize]).collect();

        for _ in 0..200 {
            tick(&mut s, &reg, &mut ctx);
        }
        for i in 0..s.c.capacity() {
            assert_eq!(
                s.c.pos[i as usize], positions[i as usize],
                "unit {i} moved after the crowd had settled"
            );
        }
    }

    #[test]
    fn a_crowd_actually_separates_rather_than_merely_stopping() {
        // The lazy way to pass the test above is to never move anything.
        let reg = registries();
        let mut s = SimState::new(caps(), 1);
        for _ in 0..8 {
            spawn(&mut s, 0, 20, 20, 1); // all exactly co-located
        }
        let mut ctx = ctx_for(&s, &reg);
        for _ in 0..300 {
            tick(&mut s, &reg, &mut ctx);
        }
        // Every pair should now be at least most of a diameter apart.
        let live: Vec<u32> = s.entities.iter_live().map(|h| h.index).collect();
        for a in 0..live.len() {
            for b in (a + 1)..live.len() {
                let d = s.c.pos[live[a] as usize].distance(s.c.pos[live[b] as usize]);
                assert!(
                    d > Fixed::from_ratio(3, 4),
                    "units {a} and {b} still overlapping at {d}"
                );
            }
        }
    }

    #[test]
    fn steering_is_reproducible_across_identical_runs() {
        fn run() -> u64 {
            let reg = registries();
            let mut s = SimState::new(caps(), 9);
            for i in 0..30i64 {
                spawn(&mut s, (i % 2) as u16, 20 + i % 5, 20 + i % 4, 1);
            }
            let mut ctx = ctx_for(&s, &reg);
            for _ in 0..200 {
                tick(&mut s, &reg, &mut ctx);
            }
            s.hash()
        }
        assert_eq!(run(), run());
    }
}
