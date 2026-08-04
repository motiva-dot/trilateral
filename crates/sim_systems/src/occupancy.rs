//! Stamping static entities into the tile grid. TECH_SPEC §4.
//!
//! Bases, resource nodes and buildings block tiles rather than colliding.
//! That is what lets the pathfinder route *around* them: a static object that
//! only existed as a collider would be invisible to A*, which would happily
//! plot a line straight through it and leave steering to shove units back out
//! forever — a unit walking on the spot against a building it cannot see.
//!
//! Called explicitly after spawning statics. When building placement lands it
//! stamps as part of construction, and salvage clears — which is why terrain
//! walkability and occupancy are stored separately on `MacroGrid`.

use sim_core::{Registries, SimState, Tile};
use trilateral_fixed::Fixed;

/// Mark every tile covered by a static entity as occupied by it.
///
/// Footprint comes from `collider_radius`, so the tiles blocked are the tiles
/// the shape actually covers — one source of truth for "how big is this".
pub fn stamp_static_occupancy(state: &mut SimState, reg: &Registries) {
    let live: Vec<_> = state.entities.iter_live().collect();
    for h in live {
        let n = h.index as usize;
        let Some(u) = reg.units.by_archetype(state.c.archetype[n]) else {
            continue;
        };
        if u.move_speed > Fixed::ZERO {
            continue;
        }
        let p = state.c.pos[n];
        let r = u.collider_radius;
        let min = state
            .grid
            .tile_at(p - trilateral_fixed::FixedVec2::new(r, r));
        let max = state
            .grid
            .tile_at(p + trilateral_fixed::FixedVec2::new(r, r));
        for y in min.y..=max.y {
            for x in min.x..=max.x {
                let t = Tile::new(x, y);
                // Tile CENTRE inside the radius, not bounding box overlap.
                //
                // A bounding box massively over-blocks: a 0.75-radius node
                // would take a 3x3 block, putting every reachable tile further
                // from the node than a worker can reach — so workers path
                // next to a resource they can never touch. The tile a static
                // stands on is always blocked, however small it is.
                let centre_inside = state.grid.tile_centre(t).distance(p) <= r;
                if centre_inside || t == state.grid.tile_at(p) {
                    state
                        .grid
                        .set_occupancy(t, sim_core::OptionalHandle::some(h));
                }
            }
        }
    }
}
