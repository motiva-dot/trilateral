//! Tile A\*. TECH_SPEC §4, the layer HPA\* will sit on top of.
//!
//! # A\* is where determinism is hardest, and it is entirely about ties
//! Two tiles with the same `f` score are equally good, and a search that picks
//! between them by "whichever the heap happened to pop" produces a different
//! path on a different machine — same length, different route, and after a few
//! seconds of movement, a different game. Every ordering decision here is
//! therefore total and explicit:
//!
//! * The open set is a binary heap ordered by `(f, h, tile_index)`. `f` first
//!   for correctness, `h` second so the search prefers tiles nearer the goal
//!   among equals (a real speed-up, not just a tiebreak), and finally tile
//!   index, which is unique — so no two entries can ever compare equal.
//! * Neighbours are visited in a fixed compass order, not in whatever order a
//!   nested loop produces.
//! * Costs are integers: 10 orthogonal, 14 diagonal. No `sqrt`, no `Fixed`, no
//!   rounding to disagree about. 14/10 approximates sqrt(2) to within 1%, which
//!   is far below what a player can perceive and exactly reproducible.
//!
//! # No hash containers (§1.2)
//! `g`, `came_from` and `closed` are flat arrays sized to the map, allocated
//! once and reused. A `HashMap<Tile, _>` would be the obvious implementation
//! and would introduce hasher-dependent iteration into the middle of gameplay.
//!
//! # Corner cutting
//! A diagonal step is only legal if *both* adjacent orthogonal tiles are open.
//! Without that rule units slip through the diagonal gap between two buildings,
//! which looks like a bug to everyone who sees it and makes walls leak.

use std::collections::BinaryHeap;

use sim_core::{MacroGrid, Tile};

/// Orthogonal step cost. Diagonals are 14, approximating sqrt(2)*10.
pub const COST_ORTHO: u32 = 10;
pub const COST_DIAG: u32 = 14;

/// Compass order, fixed. Orthogonals first so that on open ground the search
/// expands in a predictable cross before considering diagonals.
const NEIGHBOURS: [(i32, i32); 8] = [
    (0, -1),
    (-1, 0),
    (1, 0),
    (0, 1),
    (-1, -1),
    (1, -1),
    (-1, 1),
    (1, 1),
];

/// Heap entry. `Ord` is reversed so `BinaryHeap` behaves as a min-heap.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Node {
    f: u32,
    h: u32,
    index: u32,
}

impl Ord for Node {
    fn cmp(&self, other: &Node) -> std::cmp::Ordering {
        // Reversed on f and h (min-heap), and finally on index so the ordering
        // is TOTAL. If two entries could compare Equal, the heap's internal
        // order would decide the path.
        other
            .f
            .cmp(&self.f)
            .then_with(|| other.h.cmp(&self.h))
            .then_with(|| other.index.cmp(&self.index))
    }
}

impl PartialOrd for Node {
    fn partial_cmp(&self, other: &Node) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Reusable search buffers, sized to the map once.
pub struct PathScratch {
    g: Vec<u32>,
    came_from: Vec<u32>,
    closed: Vec<bool>,
    open: BinaryHeap<Node>,
    /// Search generation, so the arrays need not be cleared between searches.
    /// Clearing a 128x128 map every call is 16k writes per path request.
    stamp: Vec<u32>,
    generation: u32,
}

const NO_PARENT: u32 = u32::MAX;

impl PathScratch {
    pub fn new(tile_count: u32) -> PathScratch {
        let n = tile_count as usize;
        PathScratch {
            g: vec![0; n],
            came_from: vec![NO_PARENT; n],
            closed: vec![false; n],
            open: BinaryHeap::with_capacity(256),
            stamp: vec![0; n],
            generation: 0,
        }
    }
}

/// Octile heuristic, scaled to match the step costs. Admissible: it never
/// overestimates, which is what makes A\* return a shortest path rather than
/// merely a path.
#[inline]
fn heuristic(a: Tile, b: Tile) -> u32 {
    let dx = (a.x as i32 - b.x as i32).unsigned_abs();
    let dy = (a.y as i32 - b.y as i32).unsigned_abs();
    let (lo, hi) = if dx < dy { (dx, dy) } else { (dy, dx) };
    COST_DIAG * lo + COST_ORTHO * (hi - lo)
}

/// Find a shortest tile path from `start` to `goal`.
///
/// Returns tiles from start to goal inclusive, or `None` if unreachable.
/// `None` rather than a best-effort partial path: a unit that cannot reach its
/// destination should be told so and settle, not walk hopefully into a wall.
pub fn find_path(
    grid: &MacroGrid,
    scratch: &mut PathScratch,
    start: Tile,
    goal: Tile,
    out: &mut Vec<Tile>,
) -> bool {
    out.clear();
    if !grid.in_bounds(start) || !grid.in_bounds(goal) || !grid.is_walkable(goal) {
        return false;
    }
    if start == goal {
        out.push(start);
        return true;
    }

    scratch.generation = scratch.generation.wrapping_add(1);
    // Generation 0 is "never visited", so skip it on wrap rather than
    // resurrecting stale entries from 4 billion searches ago.
    if scratch.generation == 0 {
        scratch.generation = 1;
        scratch.stamp.fill(0);
    }
    let stamp_now = scratch.generation;
    scratch.open.clear();

    let start_i = grid.index(start);
    let goal_i = grid.index(goal);
    scratch.g[start_i as usize] = 0;
    scratch.came_from[start_i as usize] = NO_PARENT;
    scratch.closed[start_i as usize] = false;
    scratch.stamp[start_i as usize] = stamp_now;
    let h0 = heuristic(start, goal);
    scratch.open.push(Node {
        f: h0,
        h: h0,
        index: start_i,
    });

    while let Some(node) = scratch.open.pop() {
        let ci = node.index as usize;
        if scratch.closed[ci] && scratch.stamp[ci] == stamp_now {
            continue; // stale duplicate
        }
        scratch.closed[ci] = true;
        scratch.stamp[ci] = stamp_now;

        if node.index == goal_i {
            reconstruct(grid, scratch, start_i, goal_i, out);
            return true;
        }

        let cur = grid.from_index(node.index);
        let cur_g = scratch.g[ci];

        for (k, (dx, dy)) in NEIGHBOURS.iter().enumerate() {
            let nx = cur.x as i32 + dx;
            let ny = cur.y as i32 + dy;
            if nx < 0 || ny < 0 || nx >= grid.width() as i32 || ny >= grid.height() as i32 {
                continue;
            }
            let nt = Tile::new(nx as u16, ny as u16);
            if !grid.is_walkable(nt) {
                continue;
            }
            let diagonal = k >= 4;
            if diagonal {
                // Corner cutting: both orthogonal neighbours must be open, or
                // units slip diagonally between two buildings and walls leak.
                let side_a = Tile::new(cur.x, ny as u16);
                let side_b = Tile::new(nx as u16, cur.y);
                if !grid.is_walkable(side_a) || !grid.is_walkable(side_b) {
                    continue;
                }
            }

            let ni = grid.index(nt) as usize;
            let step = if diagonal { COST_DIAG } else { COST_ORTHO };
            let tentative = cur_g + step;

            let visited = scratch.stamp[ni] == stamp_now;
            if visited && (scratch.closed[ni] || tentative >= scratch.g[ni]) {
                continue;
            }
            scratch.g[ni] = tentative;
            scratch.came_from[ni] = node.index;
            scratch.closed[ni] = false;
            scratch.stamp[ni] = stamp_now;
            let h = heuristic(nt, goal);
            scratch.open.push(Node {
                f: tentative + h,
                h,
                index: ni as u32,
            });
        }
    }
    false
}

fn reconstruct(
    grid: &MacroGrid,
    scratch: &PathScratch,
    start_i: u32,
    goal_i: u32,
    out: &mut Vec<Tile>,
) {
    let mut i = goal_i;
    loop {
        out.push(grid.from_index(i));
        if i == start_i {
            break;
        }
        let p = scratch.came_from[i as usize];
        if p == NO_PARENT {
            break;
        }
        i = p;
    }
    out.reverse();
}

/// Drop waypoints that lie on a straight run, keeping the path's shape.
///
/// A tile path across open ground is one waypoint per tile, which is both
/// wasteful to store and makes units visibly aim at tile centres. Collapsing
/// collinear runs keeps the corners — the only points that matter — and is
/// what lets a fixed-capacity path slot hold a long route.
pub fn simplify(path: &mut Vec<Tile>) {
    if path.len() < 3 {
        return;
    }
    let mut write = 1;
    for read in 1..path.len() - 1 {
        let prev = path[write - 1];
        let cur = path[read];
        let next = path[read + 1];
        let d1 = (cur.x as i32 - prev.x as i32, cur.y as i32 - prev.y as i32);
        let d2 = (next.x as i32 - cur.x as i32, next.y as i32 - cur.y as i32);
        // Keep only where the direction changes.
        if d1.0.signum() != d2.0.signum() || d1.1.signum() != d2.1.signum() {
            path[write] = cur;
            write += 1;
        }
    }
    let last = path[path.len() - 1];
    path[write] = last;
    path.truncate(write + 1);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(w: u16, h: u16) -> MacroGrid {
        MacroGrid::new(w, h)
    }

    fn path(grid: &MacroGrid, a: (u16, u16), b: (u16, u16)) -> Option<Vec<Tile>> {
        let mut sc = PathScratch::new(grid.tile_count());
        let mut out = Vec::new();
        if find_path(
            grid,
            &mut sc,
            Tile::new(a.0, a.1),
            Tile::new(b.0, b.1),
            &mut out,
        ) {
            Some(out)
        } else {
            None
        }
    }

    #[test]
    fn a_path_to_yourself_is_a_single_tile() {
        let g = open(8, 8);
        assert_eq!(path(&g, (3, 3), (3, 3)).unwrap(), vec![Tile::new(3, 3)]);
    }

    #[test]
    fn a_straight_line_on_open_ground_is_straight() {
        let g = open(16, 16);
        let p = path(&g, (0, 5), (6, 5)).unwrap();
        assert_eq!(p.len(), 7);
        for (i, t) in p.iter().enumerate() {
            assert_eq!(
                *t,
                Tile::new(i as u16, 5),
                "path bowed off the straight line"
            );
        }
    }

    #[test]
    fn a_diagonal_on_open_ground_costs_fourteen_per_step() {
        // 5 diagonal steps, not 10 orthogonal ones. If the costs were equal
        // A* would return an L shape and units would move like rooks.
        let g = open(16, 16);
        let p = path(&g, (0, 0), (5, 5)).unwrap();
        assert_eq!(p.len(), 6, "expected a pure diagonal, got {p:?}");
        assert_eq!(p[3], Tile::new(3, 3));
    }

    #[test]
    fn a_wall_is_routed_around() {
        let mut g = open(16, 16);
        for y in 0..12 {
            g.set_walkable(Tile::new(8, y), false);
        }
        let p = path(&g, (2, 2), (14, 2)).unwrap();
        assert!(p.len() > 12, "suspiciously short: went through the wall?");
        for t in &p {
            assert!(g.is_walkable(*t), "path crossed a blocked tile at {t:?}");
        }
        assert_eq!(*p.first().unwrap(), Tile::new(2, 2));
        assert_eq!(*p.last().unwrap(), Tile::new(14, 2));
    }

    #[test]
    fn an_enclosed_goal_is_unreachable_rather_than_approximated() {
        // None, not a best-effort partial path: a unit that cannot arrive
        // should be told so and settle, not walk hopefully into a wall.
        let mut g = open(16, 16);
        for d in -1..=1 {
            g.set_walkable(Tile::new((8 + d) as u16, 7), false);
            g.set_walkable(Tile::new((8 + d) as u16, 9), false);
        }
        g.set_walkable(Tile::new(7, 8), false);
        g.set_walkable(Tile::new(9, 8), false);
        assert!(path(&g, (0, 0), (8, 8)).is_none());
    }

    #[test]
    fn a_blocked_goal_tile_fails_immediately() {
        let mut g = open(8, 8);
        g.set_walkable(Tile::new(4, 4), false);
        assert!(path(&g, (0, 0), (4, 4)).is_none());
    }

    #[test]
    fn diagonals_cannot_cut_a_corner_between_two_blockers() {
        // The rule that stops units slipping through walls. Block (1,0) and
        // (0,1); the diagonal from (0,0) to (1,1) must not be taken.
        let mut g = open(4, 4);
        g.set_walkable(Tile::new(1, 0), false);
        g.set_walkable(Tile::new(0, 1), false);
        let p = path(&g, (0, 0), (1, 1));
        assert!(
            p.is_none(),
            "unit squeezed diagonally through a sealed corner"
        );
    }

    #[test]
    fn a_single_blocker_also_forbids_the_diagonal() {
        // STRICT corner rule: a diagonal needs BOTH orthogonal neighbours open,
        // not merely one.
        //
        // The permissive rule (block only when both are walls) is common in
        // grid games where units are points. Ours are not: they are circles of
        // radius 0.3-0.5 moving continuously, and a building fills its whole
        // tile. A unit clipping diagonally past a single building corner would
        // visibly overlap the building — the pathfinder would be promising a
        // route that the collision system then refuses to let it walk.
        //
        // So the detour is correct: (0,0) -> (0,1) -> (1,1), three tiles.
        let mut g = open(4, 4);
        g.set_walkable(Tile::new(1, 0), false);
        let p = path(&g, (0, 0), (1, 1)).unwrap();
        assert_eq!(p.len(), 3, "cut the corner past a building: {p:?}");
        assert_eq!(p[1], Tile::new(0, 1), "took the wrong way round");
    }

    #[test]
    fn an_open_corner_still_allows_the_diagonal() {
        // The strict rule must not forbid diagonals in genuinely open space,
        // or units move like rooks everywhere.
        let g = open(4, 4);
        let p = path(&g, (0, 0), (1, 1)).unwrap();
        assert_eq!(p.len(), 2, "refused a diagonal on open ground");
    }

    #[test]
    fn buildings_block_paths_and_salvage_reopens_them() {
        let mut g = open(8, 8);
        for y in 0..8 {
            g.set_occupancy(
                Tile::new(4, y),
                sim_core::OptionalHandle::some(sim_core::EntityHandle::new(1, 1)),
            );
        }
        assert!(
            path(&g, (0, 4), (7, 4)).is_none(),
            "wall of buildings leaked"
        );
        g.set_occupancy(Tile::new(4, 4), sim_core::OptionalHandle::NONE);
        assert!(path(&g, (0, 4), (7, 4)).is_some(), "salvage did not reopen");
    }

    #[test]
    fn the_same_query_always_returns_the_identical_path() {
        // The property everything else depends on. Open ground has many equal
        // shortest paths; the search must always choose the same one.
        let g = open(32, 32);
        let a = path(&g, (1, 1), (30, 17)).unwrap();
        for _ in 0..20 {
            assert_eq!(path(&g, (1, 1), (30, 17)).unwrap(), a);
        }
    }

    #[test]
    fn reusing_scratch_gives_the_same_answers_as_a_fresh_one() {
        // The generation-stamp optimisation must be invisible. If a stale
        // entry ever leaked between searches this would diverge.
        let mut g = open(24, 24);
        for y in 4..20 {
            g.set_walkable(Tile::new(12, y), false);
        }
        let mut sc = PathScratch::new(g.tile_count());
        let queries = [
            ((0u16, 0u16), (23u16, 23u16)),
            ((5, 20), (20, 5)),
            ((0, 12), (23, 12)),
        ];
        let mut reused = Vec::new();
        for q in queries {
            let mut out = Vec::new();
            find_path(
                &g,
                &mut sc,
                Tile::new(q.0.0, q.0.1),
                Tile::new(q.1.0, q.1.1),
                &mut out,
            );
            reused.push(out);
        }
        for (i, q) in queries.iter().enumerate() {
            let fresh = path(&g, q.0, q.1).unwrap();
            assert_eq!(reused[i], fresh, "scratch reuse changed query {i}");
        }
    }

    #[test]
    fn paths_are_shortest_not_merely_valid() {
        // Admissible heuristic => optimal. Check against the known cost.
        let g = open(16, 16);
        let p = path(&g, (0, 0), (10, 4)).unwrap();
        // 4 diagonals + 6 orthogonals = 4*14 + 6*10 = 116; 11 tiles inclusive.
        assert_eq!(p.len(), 11, "not the shortest route: {p:?}");
    }

    #[test]
    fn simplify_keeps_corners_and_drops_straight_runs() {
        let mut p: Vec<Tile> = (0..6).map(|x| Tile::new(x, 0)).collect();
        p.extend((1..4).map(|y| Tile::new(5, y)));
        simplify(&mut p);
        assert_eq!(
            p,
            vec![Tile::new(0, 0), Tile::new(5, 0), Tile::new(5, 3)],
            "simplify lost the shape"
        );
    }

    #[test]
    fn simplify_leaves_short_paths_alone() {
        let mut p = vec![Tile::new(0, 0), Tile::new(1, 1)];
        let before = p.clone();
        simplify(&mut p);
        assert_eq!(p, before);
    }

    #[test]
    fn simplify_preserves_endpoints() {
        let g = open(32, 32);
        let mut p = path(&g, (0, 0), (31, 9)).unwrap();
        let (first, last) = (p[0], *p.last().unwrap());
        simplify(&mut p);
        assert_eq!(p[0], first);
        assert_eq!(*p.last().unwrap(), last);
    }

    #[test]
    fn a_maze_is_solved_and_the_route_is_legal() {
        // A serpentine corridor: the search has to commit to long detours.
        let mut g = open(21, 21);
        for row in (2..20).step_by(4) {
            let gap = if (row / 4) % 2 == 0 { 20 } else { 0 };
            for x in 0..21u16 {
                if x != gap {
                    g.set_walkable(Tile::new(x, row), false);
                }
            }
        }
        let p = path(&g, (0, 0), (0, 20)).expect("maze unsolved");
        for w in p.windows(2) {
            let (a, b) = (w[0], w[1]);
            let step = (a.x as i32 - b.x as i32)
                .abs()
                .max((a.y as i32 - b.y as i32).abs());
            assert_eq!(step, 1, "path teleported from {a:?} to {b:?}");
            assert!(g.is_walkable(b), "path entered a wall at {b:?}");
        }
    }
}
