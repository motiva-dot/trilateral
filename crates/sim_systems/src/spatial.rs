//! `SpatialHash` — uniform grid broad phase. TECH_SPEC §4.
//!
//! # The determinism requirement that shapes the whole design
//! Queries return handles **sorted by entity index**, always. Not "usually",
//! not "in bucket order" — sorted. §6.9 resolves every tie by handle index, so
//! the order a query returns candidates in decides which enemy a unit picks
//! when two are equidistant, which is gameplay. A hash map iteration order or
//! an insertion-order bucket would make that depend on memory layout.
//!
//! Cell size is `2 x max_collider_radius` taken from the whole registry, not
//! from the units currently on the map, so it cannot change mid-match.
//!
//! Rebuilt wholesale each tick rather than updated incrementally. At prototype
//! scale that is a few thousand integer operations and it removes an entire
//! class of bug — a stale entry surviving a despawn. Incremental update is a
//! Phase 4 optimisation to make only if the bench says so.

use sim_core::{EntityIndex, SimState};
use trilateral_fixed::{Fixed, FixedVec2};

#[derive(Clone, Debug)]
pub struct SpatialHash {
    /// Width of one cell, in tiles.
    cell_size: Fixed,
    /// Grid dimensions in cells.
    cols: i32,
    rows: i32,
    /// `cells[c]` holds entity indices in ascending order.
    cells: Vec<Vec<EntityIndex>>,
}

impl SpatialHash {
    /// `world_tiles` is the map extent; `cell_size` comes from the registry's
    /// largest collider.
    ///
    /// # Panics
    /// If `cell_size` is not positive — a zero cell size would divide by zero
    /// on every insert, and this is a setup error, not a runtime condition.
    pub fn new(world_tiles: i32, cell_size: Fixed) -> SpatialHash {
        assert!(
            cell_size > Fixed::ZERO,
            "SpatialHash cell_size must be positive"
        );
        let span = Fixed::from_int(world_tiles).div(cell_size).to_int_floor() as i32 + 1;
        let cols = span.max(1);
        let rows = span.max(1);
        SpatialHash {
            cell_size,
            cols,
            rows,
            cells: vec![Vec::new(); (cols * rows) as usize],
        }
    }

    #[inline]
    pub fn cell_size(&self) -> Fixed {
        self.cell_size
    }

    #[inline]
    fn cell_of(&self, p: FixedVec2) -> usize {
        // Clamp rather than wrap: an entity pushed outside the world by a bug
        // should land in an edge cell and stay findable, not alias onto the
        // opposite edge and produce phantom neighbours.
        let cx =
            p.x.div(self.cell_size)
                .to_int_floor()
                .clamp(0, self.cols as i64 - 1);
        let cy =
            p.y.div(self.cell_size)
                .to_int_floor()
                .clamp(0, self.rows as i64 - 1);
        (cy * self.cols as i64 + cx) as usize
    }

    /// Discard and rebuild from the live entities.
    ///
    /// Iterating `0..capacity` in order means each cell's contents come out
    /// ascending for free, with no sort.
    pub fn rebuild(&mut self, state: &SimState) {
        for c in &mut self.cells {
            c.clear();
        }
        for i in 0..state.c.capacity() {
            if state.c.alive.get(i) {
                let cell = self.cell_of(state.c.pos[i as usize]);
                self.cells[cell].push(i);
            }
        }
    }

    /// Entity indices whose cell overlaps the square of side `2*radius`
    /// centred on `centre`, ascending.
    ///
    /// Broad phase only — results are candidates, not hits. The caller does
    /// the exact distance test.
    pub fn query_square(&self, centre: FixedVec2, radius: Fixed, out: &mut Vec<EntityIndex>) {
        out.clear();
        let min = FixedVec2::new(centre.x - radius, centre.y - radius);
        let max = FixedVec2::new(centre.x + radius, centre.y + radius);
        let x0 = min
            .x
            .div(self.cell_size)
            .to_int_floor()
            .clamp(0, self.cols as i64 - 1);
        let x1 = max
            .x
            .div(self.cell_size)
            .to_int_floor()
            .clamp(0, self.cols as i64 - 1);
        let y0 = min
            .y
            .div(self.cell_size)
            .to_int_floor()
            .clamp(0, self.rows as i64 - 1);
        let y1 = max
            .y
            .div(self.cell_size)
            .to_int_floor()
            .clamp(0, self.rows as i64 - 1);

        for cy in y0..=y1 {
            for cx in x0..=x1 {
                let cell = (cy * self.cols as i64 + cx) as usize;
                out.extend_from_slice(&self.cells[cell]);
            }
        }
        // Cells are individually ascending but the sweep visits several, so the
        // concatenation is not. Sorting here is what makes the query's contract
        // hold — see the module note on why that matters.
        out.sort_unstable();
    }

    /// Entities strictly within `radius` of `centre`, ascending. Narrow phase.
    pub fn query_circle(
        &self,
        state: &SimState,
        centre: FixedVec2,
        radius: Fixed,
        out: &mut Vec<EntityIndex>,
    ) {
        self.query_square(centre, radius, out);
        // Squared radius in raw Q32.32 bits, to compare against
        // `distance_sq_wide` without narrowing back to `Fixed`.
        let r2 = ((radius.to_bits() as i128) * (radius.to_bits() as i128)) >> 32;
        out.retain(|&i| state.c.pos[i as usize].distance_sq_wide(centre) <= r2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{ArchetypeId, Capacities, PlayerId, Spawn};

    fn caps(n: u32) -> Capacities {
        Capacities {
            max_entities: n,
            max_projectiles: 8,
            max_commands_per_tick: 16,
            max_players: 4,
            cmd_queue_slots: 16,
            modifier_slots: 8,
            command_log_reserve: 64,
        }
    }

    fn state_with(points: &[(i32, i32)]) -> SimState {
        let mut s = SimState::new(caps(64), 1, 64);
        for &(x, y) in points {
            s.spawn(Spawn {
                archetype: ArchetypeId(0),
                owner: PlayerId(0),
                pos: FixedVec2::from_ints(x, y),
                hp: 10,
            })
            .unwrap();
        }
        s
    }

    fn hash() -> SpatialHash {
        SpatialHash::new(128, Fixed::ONE)
    }

    #[test]
    fn a_query_finds_what_is_there() {
        let s = state_with(&[(1, 1), (2, 2), (50, 50)]);
        let mut h = hash();
        h.rebuild(&s);
        let mut out = Vec::new();
        h.query_square(FixedVec2::from_ints(1, 1), Fixed::from_int(2), &mut out);
        assert!(out.contains(&0));
        assert!(out.contains(&1));
        assert!(
            !out.contains(&2),
            "distant entity should not be a candidate"
        );
    }

    #[test]
    fn results_are_ascending_by_index() {
        // The contract §6.9 depends on. Spawn in an order that would produce a
        // non-ascending result if the cells were simply concatenated.
        let s = state_with(&[(9, 9), (0, 0), (5, 5), (1, 1), (8, 8)]);
        let mut h = hash();
        h.rebuild(&s);
        let mut out = Vec::new();
        h.query_square(FixedVec2::from_ints(5, 5), Fixed::from_int(10), &mut out);
        let mut sorted = out.clone();
        sorted.sort_unstable();
        assert_eq!(out, sorted, "query results were not ascending");
    }

    #[test]
    fn rebuild_forgets_despawned_entities() {
        let mut s = state_with(&[(1, 1), (2, 2)]);
        let h0 = s.entities.iter_live().next().unwrap();
        let mut h = hash();
        h.rebuild(&s);
        let mut out = Vec::new();
        h.query_square(FixedVec2::from_ints(1, 1), Fixed::from_int(3), &mut out);
        assert_eq!(out.len(), 2);

        s.despawn(h0);
        h.rebuild(&s);
        h.query_square(FixedVec2::from_ints(1, 1), Fixed::from_int(3), &mut out);
        assert_eq!(out, vec![1], "stale entry survived a despawn");
    }

    #[test]
    fn query_circle_rejects_corners_that_query_square_accepts() {
        // (3,3) is inside a radius-5 square around the origin but outside the
        // radius-5 circle: distance is 4.24... no — 3,3 IS inside. Use (4,4),
        // distance 5.657, outside a radius-5 circle.
        let s = state_with(&[(0, 0), (4, 4)]);
        let mut h = hash();
        h.rebuild(&s);
        let mut sq = Vec::new();
        let mut ci = Vec::new();
        h.query_square(FixedVec2::ZERO, Fixed::from_int(5), &mut sq);
        h.query_circle(&s, FixedVec2::ZERO, Fixed::from_int(5), &mut ci);
        assert!(sq.contains(&1), "square should accept the corner");
        assert!(!ci.contains(&1), "circle should reject the corner");
        assert!(ci.contains(&0));
    }

    #[test]
    fn entities_outside_the_world_clamp_rather_than_wrap() {
        // A bug that flings a unit off the map must not make it appear as a
        // neighbour of something on the opposite edge.
        let s = state_with(&[(-50, -50), (0, 0)]);
        let mut h = hash();
        h.rebuild(&s);
        let mut out = Vec::new();
        h.query_square(FixedVec2::from_ints(127, 127), Fixed::ONE, &mut out);
        assert!(out.is_empty(), "far-corner query found a clamped entity");
    }

    #[test]
    fn rebuilding_twice_gives_the_same_answer() {
        let s = state_with(&[(1, 1), (7, 3), (40, 40)]);
        let mut h = hash();
        h.rebuild(&s);
        let mut a = Vec::new();
        h.query_square(FixedVec2::from_ints(5, 5), Fixed::from_int(50), &mut a);
        h.rebuild(&s);
        let mut b = Vec::new();
        h.query_square(FixedVec2::from_ints(5, 5), Fixed::from_int(50), &mut b);
        assert_eq!(a, b);
    }

    #[test]
    #[should_panic(expected = "cell_size must be positive")]
    fn a_zero_cell_size_is_refused() {
        SpatialHash::new(128, Fixed::ZERO);
    }
}
