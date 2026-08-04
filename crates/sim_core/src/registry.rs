//! Runtime unit statistics, and the `Registries` handle systems read them
//! through. CLAUDE.md §1.8.
//!
//! # Why these types live in `sim_core` and not `sim_content`
//! §1.3 lets `sim_systems` depend on `sim_core` and nothing else, but systems
//! obviously need to know a unit's speed. So the *runtime* shape of a unit
//! lives here, while the *parse-time* shape — the serde structs, the YAML
//! spelling, the `f64` fields authors actually write — stays in
//! `sim_content`, which converts.
//!
//! That split is worth more than the small duplication it costs. The YAML
//! schema can gain a field, change a name, or accept a friendlier spelling
//! without touching anything the simulation compiles against, and no serde
//! attribute can accidentally become load-bearing on tick-rate code.
//!
//! # Nothing here is part of `SimState`
//! Registries are immutable for a match and identical on every client by
//! content hash, so they are not snapshotted and not folded into the state
//! hash. What *is* hashed is each entity's `ArchetypeId` — the index into
//! this table. Two clients with different content would therefore agree on
//! the hash while disagreeing about the game, which is exactly why the
//! lockstep handshake compares a content hash before the match starts
//! (TECH_SPEC §6).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use trilateral_fixed::Fixed;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Worker,
    Melee,
    Ranged,
    Supply,
    Resource,
    /// Townhall. Workers deposit here, and it is what a player loses when a
    /// base dies — GAME_DESIGN §2.1 makes that cost Murmur its production
    /// capacity, not just its buildings.
    Base,
}

/// Visual and collision shape. PRD §3 ties these together deliberately: the
/// silhouette a player reads is the collider the simulation uses.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    Circle,
    Aabb,
    Triangle,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Ore,
    Flux,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceNode {
    pub kind: ResourceKind,
    pub amount: u32,
    pub harvest_slots: u8,
    pub per_trip: u32,
    pub trip_ticks: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnitCost {
    pub ore: u32,
    pub flux: u32,
    pub ticks: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AttackStats {
    pub damage: i32,
    pub range: Fixed,
    pub cooldown_ticks: u32,
    pub frontswing_ticks: u32,
    pub backswing_ticks: u32,
    /// Fraction of a full turn the target must be within before frontswing may
    /// begin (TECH_SPEC §5.3).
    pub arc: Fixed,
}

#[derive(Clone, Debug)]
pub struct UnitStats {
    pub id: String,
    pub faction: String,
    pub role: Role,
    pub shape: Shape,
    pub cost: UnitCost,
    pub supply_x2: u16,
    pub provides_supply_x2: u16,
    pub hp: i32,
    pub armor: i32,
    /// Tiles.
    pub collider_radius: Fixed,
    /// Integer shoving priority — heavier displaces lighter (TECH_SPEC §5.2).
    pub mass: u16,
    /// Tiles per tick.
    pub move_speed: Fixed,
    /// Fraction of a full turn per tick.
    pub turn_rate: Fixed,
    pub sight_range: Fixed,
    pub selection_weight: u16,
    pub attack: Option<AttackStats>,
    pub resource: Option<ResourceNode>,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct UnitRegistry {
    units: Vec<UnitStats>,
    by_id: BTreeMap<String, usize>,
}

impl UnitRegistry {
    /// Build from an ordered list. Order is `ArchetypeId` order and must
    /// already be deterministic — the loader in `sim_content` guarantees that
    /// and rejects duplicate ids before calling here.
    pub fn new(units: Vec<UnitStats>) -> UnitRegistry {
        let by_id = units
            .iter()
            .enumerate()
            .map(|(i, u)| (u.id.clone(), i))
            .collect();
        UnitRegistry { units, by_id }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.units.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }

    #[inline]
    pub fn get(&self, id: &str) -> Option<&UnitStats> {
        self.by_id.get(id).map(|&i| &self.units[i])
    }

    #[inline]
    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.by_id.get(id).copied()
    }

    /// The lookup systems perform, by `ArchetypeId`.
    #[inline]
    pub fn by_archetype(&self, a: crate::ids::ArchetypeId) -> Option<&UnitStats> {
        self.units.get(a.0 as usize)
    }

    pub fn iter(&self) -> impl Iterator<Item = &UnitStats> {
        self.units.iter()
    }

    /// Largest collider radius in the registry.
    ///
    /// The spatial hash sizes its cells from this (TECH_SPEC §4: cell = 2x max
    /// collider radius), so it must consider every archetype, not just the
    /// ones currently spawned — a cell size that changed with the contents of
    /// the map would change query results and therefore gameplay.
    pub fn max_collider_radius(&self) -> Fixed {
        self.units
            .iter()
            .map(|u| u.collider_radius)
            .fold(Fixed::ZERO, Fixed::max)
    }
}

/// Everything loaded from content, handed to systems as one immutable bundle.
///
/// Grows as later phases add `BuildingRegistry`, `TechRegistry` and the
/// ability table.
#[derive(Clone, Debug, Default)]
pub struct Registries {
    pub units: UnitRegistry,
    pub steering: SteeringParams,
    pub race: crate::race::RaceParams,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ArchetypeId;

    fn unit(id: &str, radius: i64) -> UnitStats {
        UnitStats {
            id: id.to_string(),
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
            collider_radius: Fixed::from_ratio(radius, 100),
            mass: 1,
            move_speed: Fixed::from_ratio(1, 10),
            turn_rate: Fixed::from_ratio(1, 10),
            sight_range: Fixed::from_int(5),
            selection_weight: 1,
            attack: None,
            resource: None,
            tags: vec![],
        }
    }

    #[test]
    fn lookup_by_id_and_by_archetype_agree() {
        let reg = UnitRegistry::new(vec![unit("a", 30), unit("b", 50)]);
        assert_eq!(reg.index_of("b"), Some(1));
        assert_eq!(reg.by_archetype(ArchetypeId(1)).unwrap().id, "b");
        assert_eq!(reg.get("a").unwrap().id, "a");
    }

    #[test]
    fn an_out_of_range_archetype_is_none_not_a_panic() {
        // A corrupt or hostile replay can contain any ArchetypeId.
        let reg = UnitRegistry::new(vec![unit("a", 30)]);
        assert!(reg.by_archetype(ArchetypeId(99)).is_none());
        assert!(reg.by_archetype(ArchetypeId::NONE).is_none());
    }

    #[test]
    fn max_collider_radius_spans_the_whole_registry() {
        let reg = UnitRegistry::new(vec![unit("a", 30), unit("big", 90), unit("c", 50)]);
        assert_eq!(reg.max_collider_radius(), Fixed::from_ratio(90, 100));
    }

    #[test]
    fn an_empty_registry_has_zero_max_radius_rather_than_panicking() {
        assert_eq!(UnitRegistry::default().max_collider_radius(), Fixed::ZERO);
    }
}

/// Local-avoidance tuning, from `assets/data/steering.yaml`. TECH_SPEC §5.2.
///
/// Feel, not capacity. PRD §7 makes clumping and shoving deliberately tunable
/// because "some friction creates positional skill" — these are the knobs that
/// friction lives in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SteeringParams {
    /// Fraction of an overlap resolved per tick.
    pub separation_response: Fixed,
    /// Ticks of no progress before a mover settles.
    pub settle_stuck_ticks: u8,
    /// Fraction of full speed below which a tick counts as no progress.
    pub settle_progress_fraction: Fixed,
    /// Hard bound on neighbours resolved per unit per tick.
    pub max_neighbours: u16,
    /// Hard bound on A* searches started per tick, across all units.
    pub max_paths_per_tick: u16,
    /// Pushes below this are dropped to zero, which is what guarantees a
    /// crowd reaches exact stillness rather than cycling forever.
    pub min_push: Fixed,
    /// Relaxation passes per tick. One pass cannot resolve a chain of
    /// overlaps, which is what reads as "mushy".
    pub separation_iterations: u8,
}

impl Default for SteeringParams {
    /// Only for tests and tools. Real values come from YAML (§1.8); a default
    /// that silently differed from the file would be a desync between a client
    /// that loaded content and one that did not.
    fn default() -> Self {
        SteeringParams {
            separation_response: Fixed::from_ratio(85, 100),
            settle_stuck_ticks: 45,
            settle_progress_fraction: Fixed::from_ratio(6, 100),
            max_neighbours: 16,
            max_paths_per_tick: 16,
            min_push: Fixed::from_ratio(1, 4096),
            separation_iterations: 3,
        }
    }
}
