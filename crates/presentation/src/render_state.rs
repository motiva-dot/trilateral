//! `RenderState` — the presentation-owned mirror of the simulation.
//!
//! TECH_SPEC §8: hold the previous and current tick's drawable values, lerp
//! between them with an `f64` alpha. Nothing here is ever read by the
//! simulation, and nothing here is hashed.
//!
//! # Why mirror at all instead of reading `SimState` directly
//! Because a rendered frame lands *between* ticks. Drawing `SimState` as-is
//! means drawing whatever the last completed tick produced, which at 30 Hz on
//! a 144 Hz monitor shows each position for four or five frames and reads as
//! stepped. The mirror is what makes 30 Hz look smooth, and it is why §8 says
//! never to draw raw fixed positions.

use bytemuck::{Pod, Zeroable};
use sim_core::{Registries, SimState};
use trilateral_fixed::Fixed;

/// One drawn shape. Packed for direct upload as an instance buffer.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Instance {
    /// Centre, in normalised device coordinates.
    pub ndc: [f32; 2],
    /// Radius as an NDC half-extent, x and y separately so a non-square
    /// window does not turn circles into ellipses.
    pub half: [f32; 2],
    pub colour: [f32; 4],
}

/// One entity's drawable values at a single tick.
#[derive(Clone, Copy, Debug, Default)]
struct UnitView {
    alive: bool,
    x: f64,
    y: f64,
    radius: f64,
    owner: u8,
}

pub struct RenderState {
    prev: Vec<UnitView>,
    curr: Vec<UnitView>,
    /// Reused every frame so drawing allocates nothing after start-up.
    instances: Vec<Instance>,
    /// Entity indices the player has selected. Presentation-only — selection
    /// is not simulation state, which is why a replay viewer can select
    /// freely without perturbing anything.
    pub selected: Vec<u32>,
}

impl RenderState {
    pub fn new(capacity: u32) -> RenderState {
        let n = capacity as usize;
        RenderState {
            prev: vec![UnitView::default(); n],
            curr: vec![UnitView::default(); n],
            instances: Vec::with_capacity(n),
            selected: Vec::new(),
        }
    }

    /// Capture the simulation after a tick. Call once per tick, never per frame.
    ///
    /// Takes `&SimState` — presentation cannot write simulation state (§1.6),
    /// and the signature is the enforcement.
    pub fn capture(&mut self, state: &SimState, reg: &Registries) {
        core::mem::swap(&mut self.prev, &mut self.curr);
        for i in 0..state.c.capacity() {
            let n = i as usize;
            let alive = state.c.alive.get(i);
            let radius = reg
                .units
                .by_archetype(state.c.archetype[n])
                .map(|u| u.collider_radius)
                .unwrap_or(Fixed::from_ratio(1, 4));
            self.curr[n] = UnitView {
                alive,
                x: state.c.pos[n].x.to_f64(),
                y: state.c.pos[n].y.to_f64(),
                radius: radius.to_f64(),
                owner: state.c.owner[n].0,
            };
        }
    }

    /// A newly spawned entity has no previous position to lerp from.
    ///
    /// Without this it would streak across the map from wherever the slot's
    /// last occupant died — the classic recycled-slot visual bug, and one that
    /// only appears once a match has had casualties.
    #[inline]
    fn from_view(prev: &UnitView, curr: &UnitView) -> (f64, f64) {
        if prev.alive {
            (prev.x, prev.y)
        } else {
            (curr.x, curr.y)
        }
    }

    /// Build the instance buffer for a frame at interpolation factor `alpha`.
    pub fn instances(&mut self, camera: &crate::Camera, alpha: f64) -> &[Instance] {
        self.instances.clear();
        let scale = camera.tile_to_ndc_scale();
        for i in 0..self.curr.len() {
            let c = self.curr[i];
            if !c.alive {
                continue;
            }
            let (px, py) = Self::from_view(&self.prev[i], &c);
            let x = px + (c.x - px) * alpha;
            let y = py + (c.y - py) * alpha;
            let selected = self.selected.contains(&(i as u32));
            self.instances.push(Instance {
                ndc: camera.world_to_ndc([x, y]),
                half: [(c.radius as f32) * scale[0], (c.radius as f32) * scale[1]],
                colour: owner_colour(c.owner, selected),
            });
        }
        &self.instances
    }

    /// Entity index nearest to a world point within `radius` tiles, if any.
    ///
    /// Ties break by lowest index, matching §6.9. Selection is cosmetic, but
    /// using the same rule everywhere means one fewer place for two clients to
    /// behave differently if selection ever does feed a command.
    pub fn pick(&self, world: [f64; 2], radius: f64) -> Option<u32> {
        let mut best: Option<(f64, u32)> = None;
        for (i, v) in self.curr.iter().enumerate() {
            if !v.alive {
                continue;
            }
            let dx = v.x - world[0];
            let dy = v.y - world[1];
            let d2 = dx * dx + dy * dy;
            let reach = v.radius + radius;
            if d2 <= reach * reach {
                match best {
                    Some((bd, _)) if bd <= d2 => {}
                    _ => best = Some((d2, i as u32)),
                }
            }
        }
        best.map(|(_, i)| i)
    }

    /// Entity indices inside a world-space rectangle, ascending.
    pub fn pick_box(&self, a: [f64; 2], b: [f64; 2]) -> Vec<u32> {
        let (x0, x1) = (a[0].min(b[0]), a[0].max(b[0]));
        let (y0, y1) = (a[1].min(b[1]), a[1].max(b[1]));
        self.curr
            .iter()
            .enumerate()
            .filter(|(_, v)| v.alive && v.x >= x0 && v.x <= x1 && v.y >= y0 && v.y <= y1)
            .map(|(i, _)| i as u32)
            .collect()
    }
}

/// Faction colours. Deliberately flat and high-contrast — this view exists to
/// be read at a glance, not to look good.
fn owner_colour(owner: u8, selected: bool) -> [f32; 4] {
    let base: [f32; 3] = match owner {
        0 => [0.30, 0.65, 1.00],
        1 => [1.00, 0.45, 0.30],
        2 => [0.45, 0.90, 0.50],
        u8::MAX => [0.65, 0.62, 0.55], // neutral
        _ => [0.80, 0.75, 0.35],
    };
    if selected {
        [
            (base[0] * 0.4 + 0.6).min(1.0),
            (base[1] * 0.4 + 0.6).min(1.0),
            (base[2] * 0.4 + 0.6).min(1.0),
            1.0,
        ]
    } else {
        [base[0], base[1], base[2], 1.0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::registry::{Role, Shape, UnitCost, UnitRegistry, UnitStats};
    use sim_core::{ArchetypeId, Capacities, PlayerId, Spawn};
    use trilateral_fixed::FixedVec2;

    fn caps() -> Capacities {
        Capacities {
            max_entities: 16,
            max_projectiles: 4,
            max_commands_per_tick: 8,
            max_players: 4,
            cmd_queue_slots: 8,
            modifier_slots: 4,
            command_log_reserve: 16,
        }
    }

    fn reg() -> Registries {
        Registries {
            units: UnitRegistry::new(vec![UnitStats {
                id: "u".into(),
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
                collider_radius: Fixed::from_ratio(1, 2),
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

    fn spawn(s: &mut SimState, x: i32, y: i32, owner: u8) {
        s.spawn(Spawn {
            archetype: ArchetypeId(0),
            owner: PlayerId(owner),
            pos: FixedVec2::from_ints(x, y),
            hp: 10,
        })
        .unwrap();
    }

    #[test]
    fn interpolation_lands_halfway_at_alpha_one_half() {
        let r = reg();
        let mut s = SimState::new(caps(), 1);
        spawn(&mut s, 0, 0, 0);
        let mut rs = RenderState::new(16);
        rs.capture(&s, &r);
        s.c.pos[0] = FixedVec2::from_ints(10, 0);
        rs.capture(&s, &r);

        let mut cam = crate::Camera::new();
        cam.centre = [0.0, 0.0];
        cam.viewport = [800.0, 600.0];
        let at_half = rs.instances(&cam, 0.5)[0].ndc[0];
        let at_zero = rs.instances(&cam, 0.0)[0].ndc[0];
        let at_one = rs.instances(&cam, 1.0)[0].ndc[0];
        assert!(at_zero < at_half && at_half < at_one, "not interpolating");
        assert!(
            ((at_half - at_zero) - (at_one - at_half)).abs() < 1e-4,
            "midpoint was not halfway"
        );
    }

    #[test]
    fn a_newly_spawned_unit_does_not_streak_from_the_slots_last_occupant() {
        // The recycled-slot bug: without the guard, a unit spawning into a
        // dead unit's slot lerps across the map from wherever that one died.
        let r = reg();
        let mut s = SimState::new(caps(), 1);
        spawn(&mut s, 50, 50, 0);
        let mut rs = RenderState::new(16);
        rs.capture(&s, &r);

        let h = s.entities.iter_live().next().unwrap();
        s.despawn(h);
        rs.capture(&s, &r);
        spawn(&mut s, 1, 1, 0); // reuses slot 0
        rs.capture(&s, &r);

        let mut cam = crate::Camera::new();
        cam.centre = [1.0, 1.0];
        cam.viewport = [800.0, 600.0];
        let at_zero = rs.instances(&cam, 0.0)[0].ndc;
        assert!(
            at_zero[0].abs() < 1e-5 && at_zero[1].abs() < 1e-5,
            "new unit streaked from the old occupant: {at_zero:?}"
        );
    }

    #[test]
    fn dead_entities_are_not_drawn() {
        let r = reg();
        let mut s = SimState::new(caps(), 1);
        spawn(&mut s, 1, 1, 0);
        spawn(&mut s, 2, 2, 0);
        let mut rs = RenderState::new(16);
        rs.capture(&s, &r);
        assert_eq!(rs.instances(&crate::Camera::new(), 0.0).len(), 2);

        let h = s.entities.iter_live().next().unwrap();
        s.despawn(h);
        rs.capture(&s, &r);
        assert_eq!(rs.instances(&crate::Camera::new(), 0.0).len(), 1);
    }

    #[test]
    fn picking_finds_the_nearest_and_breaks_ties_by_lowest_index() {
        let r = reg();
        let mut s = SimState::new(caps(), 1);
        spawn(&mut s, 10, 10, 0);
        spawn(&mut s, 10, 10, 0); // exactly co-located
        spawn(&mut s, 40, 40, 0);
        let mut rs = RenderState::new(16);
        rs.capture(&s, &r);

        assert_eq!(
            rs.pick([10.0, 10.0], 0.1),
            Some(0),
            "tie must go to index 0"
        );
        assert_eq!(rs.pick([40.0, 40.0], 0.1), Some(2));
        assert_eq!(rs.pick([0.0, 0.0], 0.1), None, "picked empty ground");
    }

    #[test]
    fn box_selection_returns_ascending_indices() {
        let r = reg();
        let mut s = SimState::new(caps(), 1);
        for i in 0..5 {
            spawn(&mut s, i, i, 0);
        }
        let mut rs = RenderState::new(16);
        rs.capture(&s, &r);
        let got = rs.pick_box([3.5, 3.5], [-0.5, -0.5]); // reversed corners
        assert_eq!(got, vec![0, 1, 2, 3], "reversed drag or wrong order");
    }

    #[test]
    fn selection_changes_colour_without_touching_the_simulation() {
        let r = reg();
        let mut s = SimState::new(caps(), 1);
        spawn(&mut s, 1, 1, 0);
        let mut rs = RenderState::new(16);
        rs.capture(&s, &r);
        let before = s.hash();
        let plain = rs.instances(&crate::Camera::new(), 0.0)[0].colour;
        rs.selected.push(0);
        let lit = rs.instances(&crate::Camera::new(), 0.0)[0].colour;
        assert_ne!(plain, lit, "selection should be visible");
        assert_eq!(s.hash(), before, "presentation mutated simulation state");
    }

    #[test]
    fn an_unknown_archetype_still_draws_at_a_default_size() {
        // A debug view must never blank out because content is missing.
        let r = Registries::default();
        let mut s = SimState::new(caps(), 1);
        spawn(&mut s, 1, 1, 0);
        let mut rs = RenderState::new(16);
        rs.capture(&s, &r);
        let inst = rs.instances(&crate::Camera::new(), 0.0);
        assert_eq!(inst.len(), 1);
        assert!(inst[0].half[0] > 0.0);
    }
}
