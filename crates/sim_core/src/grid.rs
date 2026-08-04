//! `MacroGrid` — the tile layer. CLAUDE.md §3.1, TECH_SPEC §4.
//!
//! Per tile: walkable, buildable, elevation 0–2, and the building occupying it.
//! Bloom and power bits arrive with Phase 8's race mechanics.
//!
//! # Tiles are the pathfinding world; `Fixed` is the movement world
//! Units live at continuous `FixedVec2` positions and path across a discrete
//! tile grid. The conversion is `floor`, and it is deliberately the *only*
//! conversion — a rounding rule that differed between "which tile am I in" and
//! "which tile is this obstacle in" would make a unit path into a wall it
//! believes it is standing beside.
//!
//! # Why elevation is a `u8` and not a `Fixed`
//! §5.3: high ground grants vision denial and a damage-taken modifier. It is a
//! discrete tier, not a height field — there is no ramp interpolation and no
//! sub-tile slope. Storing it as a level rather than a height keeps the
//! comparison exact and the hash small.

use serde::{Deserialize, Serialize};
use trilateral_fixed::{Fixed, FixedVec2};

use crate::bitset::BitSet;
use crate::hash::SimHasher;
use crate::ids::OptionalHandle;

/// A tile coordinate. Separate from `EntityIndex` so the two cannot be mixed.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct Tile {
    pub x: u16,
    pub y: u16,
}

impl Tile {
    #[inline]
    pub const fn new(x: u16, y: u16) -> Tile {
        Tile { x, y }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct MacroGrid {
    width: u16,
    height: u16,
    walkable: BitSet,
    buildable: BitSet,
    elevation: Box<[u8]>,
    /// The building standing on this tile, if any. Units do not occupy tiles —
    /// they collide continuously (see `sim_systems::steering`). Buildings do,
    /// which is what makes a Bastion wall a wall.
    occupancy: Box<[OptionalHandle]>,
}

impl MacroGrid {
    /// A grid that is entirely open.
    ///
    /// # Panics
    /// If either dimension is zero.
    pub fn new(width: u16, height: u16) -> MacroGrid {
        assert!(
            width > 0 && height > 0,
            "MacroGrid dimensions must be positive"
        );
        let n = width as usize * height as usize;
        let mut walkable = BitSet::new(n as u32);
        let mut buildable = BitSet::new(n as u32);
        for i in 0..n as u32 {
            walkable.set(i, true);
            buildable.set(i, true);
        }
        MacroGrid {
            width,
            height,
            walkable,
            buildable,
            elevation: vec![0u8; n].into_boxed_slice(),
            occupancy: vec![OptionalHandle::NONE; n].into_boxed_slice(),
        }
    }

    #[inline]
    pub fn width(&self) -> u16 {
        self.width
    }

    #[inline]
    pub fn height(&self) -> u16 {
        self.height
    }

    #[inline]
    pub fn tile_count(&self) -> u32 {
        self.width as u32 * self.height as u32
    }

    #[inline]
    pub fn in_bounds(&self, t: Tile) -> bool {
        t.x < self.width && t.y < self.height
    }

    #[inline]
    pub fn index(&self, t: Tile) -> u32 {
        t.y as u32 * self.width as u32 + t.x as u32
    }

    #[inline]
    pub fn from_index(&self, i: u32) -> Tile {
        Tile::new(
            (i % self.width as u32) as u16,
            (i / self.width as u32) as u16,
        )
    }

    /// Which tile a continuous position falls in.
    ///
    /// Floor, and negative coordinates clamp to zero rather than wrapping. A
    /// unit shoved off the map edge by a bug should report an edge tile, not
    /// an enormous one on the far side.
    #[inline]
    pub fn tile_at(&self, p: FixedVec2) -> Tile {
        let x = p.x.to_int_floor().clamp(0, self.width as i64 - 1);
        let y = p.y.to_int_floor().clamp(0, self.height as i64 - 1);
        Tile::new(x as u16, y as u16)
    }

    /// The centre of a tile, in world coordinates. Path waypoints aim here.
    #[inline]
    pub fn tile_centre(&self, t: Tile) -> FixedVec2 {
        FixedVec2::new(
            Fixed::from_int(t.x as i32) + Fixed::HALF,
            Fixed::from_int(t.y as i32) + Fixed::HALF,
        )
    }

    /// Out-of-bounds tiles are not walkable, rather than a panic — pathfinding
    /// probes neighbours at the map edge constantly and a bounds check at every
    /// call site would be noise that eventually gets forgotten somewhere.
    #[inline]
    pub fn is_walkable(&self, t: Tile) -> bool {
        self.in_bounds(t) && self.walkable.get(self.index(t)) && self.occupancy_at(t).is_none()
    }

    /// Terrain walkability, ignoring buildings. Used when a building is being
    /// placed or salvaged and the question is what the ground is like.
    #[inline]
    pub fn is_terrain_walkable(&self, t: Tile) -> bool {
        self.in_bounds(t) && self.walkable.get(self.index(t))
    }

    #[inline]
    pub fn is_buildable(&self, t: Tile) -> bool {
        self.in_bounds(t) && self.buildable.get(self.index(t)) && self.occupancy_at(t).is_none()
    }

    #[inline]
    pub fn elevation(&self, t: Tile) -> u8 {
        if self.in_bounds(t) {
            self.elevation[self.index(t) as usize]
        } else {
            0
        }
    }

    #[inline]
    pub fn occupancy_at(&self, t: Tile) -> OptionalHandle {
        if self.in_bounds(t) {
            self.occupancy[self.index(t) as usize]
        } else {
            OptionalHandle::NONE
        }
    }

    pub fn set_walkable(&mut self, t: Tile, v: bool) {
        if self.in_bounds(t) {
            let i = self.index(t);
            self.walkable.set(i, v);
        }
    }

    pub fn set_buildable(&mut self, t: Tile, v: bool) {
        if self.in_bounds(t) {
            let i = self.index(t);
            self.buildable.set(i, v);
        }
    }

    pub fn set_elevation(&mut self, t: Tile, level: u8) {
        if self.in_bounds(t) {
            let i = self.index(t) as usize;
            // §5.3 defines three tiers. Clamping rather than asserting means a
            // malformed map file degrades instead of crashing the match.
            self.elevation[i] = level.min(2);
        }
    }

    pub fn set_occupancy(&mut self, t: Tile, h: OptionalHandle) {
        if self.in_bounds(t) {
            let i = self.index(t) as usize;
            self.occupancy[i] = h;
        }
    }

    pub fn hash_into(&self, h: &mut SimHasher) {
        h.write_u32(self.width as u32);
        h.write_u32(self.height as u32);
        self.walkable.hash_into(h);
        self.buildable.hash_into(h);
        for v in &self.elevation {
            h.write_u8(*v);
        }
        for v in &self.occupancy {
            let (i, g) = v.raw();
            h.write_u32(i);
            h.write_u32(g);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_grid_is_entirely_open() {
        let g = MacroGrid::new(8, 4);
        assert_eq!(g.tile_count(), 32);
        for y in 0..4 {
            for x in 0..8 {
                let t = Tile::new(x, y);
                assert!(g.is_walkable(t));
                assert!(g.is_buildable(t));
                assert_eq!(g.elevation(t), 0);
            }
        }
    }

    #[test]
    fn index_and_from_index_round_trip() {
        let g = MacroGrid::new(13, 7);
        for y in 0..7 {
            for x in 0..13 {
                let t = Tile::new(x, y);
                assert_eq!(g.from_index(g.index(t)), t);
            }
        }
    }

    #[test]
    fn tile_at_floors_and_matches_tile_centre() {
        // The conversion that must be the only one. If these disagree, a unit
        // paths into a wall it believes it is standing beside.
        let g = MacroGrid::new(16, 16);
        for (x, y, ex, ey) in [(0i32, 0i32, 0u16, 0u16), (3, 5, 3, 5), (15, 15, 15, 15)] {
            let p = FixedVec2::from_ints(x, y);
            assert_eq!(g.tile_at(p), Tile::new(ex, ey));
        }
        // Anywhere inside a tile maps to that tile.
        let inside = FixedVec2::new(Fixed::from_ratio(7, 2), Fixed::from_ratio(19, 4));
        assert_eq!(g.tile_at(inside), Tile::new(3, 4));
        // And a tile's centre maps back to itself.
        let t = Tile::new(9, 2);
        assert_eq!(g.tile_at(g.tile_centre(t)), t);
    }

    #[test]
    fn positions_outside_the_map_clamp_rather_than_wrap() {
        let g = MacroGrid::new(16, 16);
        assert_eq!(g.tile_at(FixedVec2::from_ints(-5, -5)), Tile::new(0, 0));
        assert_eq!(g.tile_at(FixedVec2::from_ints(999, 999)), Tile::new(15, 15));
    }

    #[test]
    fn out_of_bounds_tiles_are_not_walkable_and_do_not_panic() {
        // Pathfinding probes past the edge constantly.
        let g = MacroGrid::new(4, 4);
        assert!(!g.is_walkable(Tile::new(4, 0)));
        assert!(!g.is_walkable(Tile::new(0, 4)));
        assert!(!g.is_walkable(Tile::new(u16::MAX, u16::MAX)));
        assert_eq!(g.elevation(Tile::new(99, 99)), 0);
    }

    #[test]
    fn blocking_a_tile_makes_it_unwalkable() {
        let mut g = MacroGrid::new(4, 4);
        let t = Tile::new(2, 2);
        g.set_walkable(t, false);
        assert!(!g.is_walkable(t));
        assert!(!g.is_terrain_walkable(t));
        assert!(g.is_walkable(Tile::new(2, 1)), "neighbour was affected");
    }

    #[test]
    fn a_building_blocks_a_tile_without_changing_the_terrain() {
        // Salvage (§2.2) must restore walkability, so the two must be separate.
        let mut g = MacroGrid::new(4, 4);
        let t = Tile::new(1, 1);
        let h = OptionalHandle::some(crate::ids::EntityHandle::new(5, 1));
        g.set_occupancy(t, h);
        assert!(!g.is_walkable(t), "building did not block");
        assert!(g.is_terrain_walkable(t), "terrain should be unchanged");
        assert!(!g.is_buildable(t));

        g.set_occupancy(t, OptionalHandle::NONE);
        assert!(g.is_walkable(t), "salvage did not restore the tile");
    }

    #[test]
    fn elevation_clamps_to_three_tiers() {
        let mut g = MacroGrid::new(4, 4);
        let t = Tile::new(0, 0);
        g.set_elevation(t, 200);
        assert_eq!(
            g.elevation(t),
            2,
            "a malformed map should degrade, not crash"
        );
    }

    #[test]
    fn the_hash_notices_a_single_tile_change() {
        let a = MacroGrid::new(8, 8);
        let mut b = MacroGrid::new(8, 8);
        let hash = |g: &MacroGrid| {
            let mut h = SimHasher::new();
            g.hash_into(&mut h);
            h.finish()
        };
        assert_eq!(hash(&a), hash(&b));
        b.set_walkable(Tile::new(3, 3), false);
        assert_ne!(hash(&a), hash(&b));
    }

    #[test]
    fn grids_of_different_shape_hash_differently() {
        let hash = |g: &MacroGrid| {
            let mut h = SimHasher::new();
            g.hash_into(&mut h);
            h.finish()
        };
        assert_ne!(hash(&MacroGrid::new(4, 16)), hash(&MacroGrid::new(16, 4)));
    }

    #[test]
    #[should_panic(expected = "must be positive")]
    fn a_zero_sized_grid_is_refused() {
        MacroGrid::new(0, 8);
    }
}
