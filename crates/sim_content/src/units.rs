//! `units.yaml` → `sim_core::UnitRegistry`. CLAUDE.md §1.8, GAME_DESIGN §2.
//!
//! # Parse-time types here, runtime types in `sim_core`
//! The `Raw*` structs below are the YAML spelling: `f64` fields, serde
//! attributes, optional everything. They convert once, at load, into the
//! runtime `UnitStats` that `sim_systems` compiles against. See
//! `sim_core::registry` for why the split exists.
//!
//! # Where the float bridge is allowed to be
//! Authored values like `move_speed: 0.1362` are decimals because that is what
//! a human tuning a game writes. They cross into `Fixed` exactly once, here.
//! After that the simulation never sees a float.
//!
//! # Registry index is `ArchetypeId` is gameplay
//! An entity's only link to its stats is its `ArchetypeId`, which is its index
//! in the registry. Index assignment must therefore be a pure function of the
//! file: sorted by faction key, then authored order within a faction. Two
//! clients that assigned indices differently would disagree about what every
//! unit on the map *is*.

use std::collections::BTreeMap;

use serde::Deserialize;
use sim_core::registry::{
    AttackStats, ResourceNode, Role, Shape, UnitCost, UnitRegistry, UnitStats,
};
use trilateral_fixed::Fixed;

use crate::tech::ContentError;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawAttack {
    pub damage: i32,
    pub range: f64,
    pub cooldown_ticks: u32,
    pub frontswing_ticks: u32,
    pub backswing_ticks: u32,
    /// Fraction of a full turn the target must be within before frontswing may
    /// begin (TECH_SPEC §5.3).
    pub arc: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawUnit {
    pub id: String,
    pub role: Role,
    pub shape: Shape,
    pub cost: UnitCost,
    pub supply_x2: u16,
    #[serde(default)]
    pub provides_supply_x2: u16,
    pub hp: i32,
    pub armor: i32,
    pub collider_radius: f64,
    pub mass: u16,
    pub move_speed: f64,
    pub turn_rate: f64,
    pub sight_range: f64,
    pub selection_weight: u16,
    #[serde(default)]
    pub attack: Option<RawAttack>,
    #[serde(default)]
    pub resource: Option<ResourceNode>,
    #[serde(default)]
    pub tags: Vec<String>,
}

pub type RawUnitFile = BTreeMap<String, Vec<RawUnit>>;

/// Parse, validate, and convert to the runtime registry.
pub fn units_from_yaml(src: &str) -> Result<UnitRegistry, ContentError> {
    let raw: RawUnitFile =
        serde_saphyr::from_str(src).map_err(|e| ContentError::Parse(e.to_string()))?;
    units_from_raw(raw)
}

pub fn units_from_raw(raw: RawUnitFile) -> Result<UnitRegistry, ContentError> {
    let mut units: Vec<UnitStats> = Vec::new();
    let mut seen: BTreeMap<&str, ()> = BTreeMap::new();

    // BTreeMap iteration is sorted by faction, so index assignment is a pure
    // function of the file contents rather than of insertion order.
    for (faction, entries) in &raw {
        for u in entries {
            if seen.insert(u.id.as_str(), ()).is_some() {
                return Err(ContentError::DuplicateId(u.id.clone()));
            }
            validate(u)?;
            units.push(UnitStats {
                id: u.id.clone(),
                faction: faction.clone(),
                role: u.role,
                shape: u.shape,
                cost: u.cost,
                supply_x2: u.supply_x2,
                provides_supply_x2: u.provides_supply_x2,
                hp: u.hp,
                armor: u.armor,
                collider_radius: Fixed::from_f64(u.collider_radius),
                mass: u.mass,
                move_speed: Fixed::from_f64(u.move_speed),
                turn_rate: Fixed::from_f64(u.turn_rate),
                sight_range: Fixed::from_f64(u.sight_range),
                selection_weight: u.selection_weight,
                attack: u.attack.map(|a| AttackStats {
                    damage: a.damage,
                    range: Fixed::from_f64(a.range),
                    cooldown_ticks: a.cooldown_ticks,
                    frontswing_ticks: a.frontswing_ticks,
                    backswing_ticks: a.backswing_ticks,
                    arc: Fixed::from_f64(a.arc),
                }),
                resource: u.resource,
                tags: u.tags.clone(),
            });
        }
    }
    if units.is_empty() {
        return Err(ContentError::Empty);
    }
    Ok(UnitRegistry::new(units))
}

/// Reject values that are nonsense before they reach the simulation.
///
/// Every one of these has a failure mode worse than an error message: zero HP
/// spawns a corpse, a negative radius inverts collision, a zero cooldown fires
/// every tick forever, and a swing longer than the cooldown means the attack
/// FSM can never return to ready.
fn validate(u: &RawUnit) -> Result<(), ContentError> {
    let bad = |reason: &str| ContentError::BadEffect {
        tech: u.id.clone(),
        reason: reason.to_string(),
    };
    if u.hp <= 0 {
        return Err(bad("hp must be positive"));
    }
    if u.collider_radius < 0.0 {
        return Err(bad("collider_radius must not be negative"));
    }
    if u.move_speed < 0.0 {
        return Err(bad("move_speed must not be negative"));
    }
    if let Some(a) = &u.attack {
        if a.cooldown_ticks == 0 {
            return Err(bad(
                "attack.cooldown_ticks must be non-zero, or the unit attacks every tick",
            ));
        }
        if a.range < 0.0 {
            return Err(bad("attack.range must not be negative"));
        }
        if a.frontswing_ticks + a.backswing_ticks > a.cooldown_ticks {
            return Err(bad(
                "frontswing + backswing exceeds cooldown; the attack FSM could never complete",
            ));
        }
    }
    if let Some(r) = &u.resource
        && r.harvest_slots == 0
    {
        return Err(bad("resource.harvest_slots must be non-zero"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::registry::ResourceKind;

    const REAL_UNITS: &str = include_str!("../../../assets/data/units.yaml");

    #[test]
    fn the_real_units_file_loads() {
        let reg = units_from_yaml(REAL_UNITS)
            .unwrap_or_else(|e| panic!("assets/data/units.yaml failed: {e}"));
        assert!(reg.len() >= 6, "only {} units loaded", reg.len());
    }

    #[test]
    fn brood_war_derived_stats_survive_the_conversion() {
        let reg = units_from_yaml(REAL_UNITS).unwrap();

        // BW Drone: 40 HP, 5 damage, cooldown 22 frames, 50 minerals.
        // 22 frames * (30/23.81) = 27.7 -> 28 ticks.
        let drone = reg.get("mur_drone").unwrap();
        assert_eq!(drone.hp, 40);
        assert_eq!(drone.cost.ore, 50);
        let a = drone.attack.unwrap();
        assert_eq!(a.damage, 5);
        assert_eq!(a.cooldown_ticks, 28);

        // BW Zergling: 35 HP, 5 damage, cooldown 8 frames -> 10 ticks,
        // 0.5 supply -> supply_x2 = 1 (audit item 29).
        let mite = reg.get("mur_mite").unwrap();
        assert_eq!(mite.hp, 35);
        assert_eq!(mite.supply_x2, 1, "half supply must be stored doubled");
        assert_eq!(mite.attack.unwrap().cooldown_ticks, 10);

        // BW Hydralisk: 80 HP, 10 damage, range 4, cooldown 15 -> 19 ticks.
        let lasher = reg.get("mur_lasher").unwrap();
        assert_eq!(lasher.hp, 80);
        assert_eq!(lasher.cost.flux, 25);
        let la = lasher.attack.unwrap();
        assert_eq!(la.damage, 10);
        assert_eq!(la.range, Fixed::from_int(4));
        assert_eq!(la.cooldown_ticks, 19);
    }

    #[test]
    fn supply_is_stored_doubled_everywhere() {
        // Audit item 29: `supply: 0.5` cannot exist in an integer simulation.
        let reg = units_from_yaml(REAL_UNITS).unwrap();
        assert_eq!(reg.get("mur_mite").unwrap().supply_x2, 1); // 0.5 displayed
        assert_eq!(reg.get("mur_drone").unwrap().supply_x2, 2); // 1.0 displayed
        // BW Overlord provides 8 supply.
        assert_eq!(reg.get("mur_supply_sac").unwrap().provides_supply_x2, 16);
    }

    #[test]
    fn decimals_reach_fixed_point_exactly_as_authored() {
        let reg = units_from_yaml(REAL_UNITS).unwrap();
        // 0.1362 tiles/tick, rounded to nearest Q32.32. Written in hex
        // deliberately: the decimal form invites recomputing it by hand, and
        // hand arithmetic is how a wrong "expected" value gets committed.
        let mite = reg.get("mur_mite").unwrap();
        assert_eq!(mite.move_speed, Fixed::from_bits(0x22de_00d2));
        assert_eq!(mite.move_speed.to_string(), "0.136200000");
        // And the ordering that matters for feel: the ranged unit is slower.
        assert!(reg.get("mur_lasher").unwrap().move_speed < mite.move_speed);
    }

    #[test]
    fn resource_nodes_are_neutral_and_carry_harvest_data() {
        let reg = units_from_yaml(REAL_UNITS).unwrap();
        let ore = reg.get("ore_node").unwrap();
        assert_eq!(ore.faction, "neutral");
        let r = ore.resource.unwrap();
        assert_eq!(r.kind, ResourceKind::Ore);
        assert_eq!(r.amount, 1500);
        assert_eq!(r.harvest_slots, 2); // GAME_DESIGN §1.1
        let flux = reg.get("flux_fissure").unwrap().resource.unwrap();
        assert_eq!(flux.kind, ResourceKind::Flux);
        assert_eq!(flux.harvest_slots, 3); // GAME_DESIGN §1.2
    }

    #[test]
    fn archetype_indices_are_stable_across_loads() {
        // The index IS the ArchetypeId, and the ArchetypeId is an entity's only
        // link to its stats. Two clients disagreeing here disagree about what
        // every unit on the map is.
        let a = units_from_yaml(REAL_UNITS).unwrap();
        let b = units_from_yaml(REAL_UNITS).unwrap();
        let ids_a: Vec<&str> = a.iter().map(|u| u.id.as_str()).collect();
        let ids_b: Vec<&str> = b.iter().map(|u| u.id.as_str()).collect();
        assert_eq!(ids_a, ids_b);
        assert_eq!(a.index_of("mur_mite"), b.index_of("mur_mite"));
    }

    #[test]
    fn every_combat_unit_can_actually_complete_an_attack() {
        let reg = units_from_yaml(REAL_UNITS).unwrap();
        for u in reg.iter() {
            if let Some(a) = u.attack {
                assert!(
                    a.frontswing_ticks + a.backswing_ticks <= a.cooldown_ticks,
                    "{} cannot complete its attack cycle",
                    u.id
                );
            }
        }
    }

    #[test]
    fn the_spatial_hash_cell_size_source_is_sane() {
        // TECH_SPEC §4 sizes spatial cells from the largest collider.
        let reg = units_from_yaml(REAL_UNITS).unwrap();
        let max = reg.max_collider_radius();
        assert!(max > Fixed::ZERO);
        assert!(
            max <= Fixed::from_int(2),
            "collider radius unexpectedly large"
        );
    }

    // ---- malformed content ------------------------------------------------

    fn minimal(extra: &str) -> String {
        format!(
            "murmur:
  - id: u
    role: melee
    shape: circle
    cost: {{ ore: 1, flux: 0, ticks: 1 }}
    supply_x2: 2
    hp: 10
    armor: 0
    collider_radius: 0.5
    mass: 1
    move_speed: 0.1
    turn_rate: 0.1
    sight_range: 5.0
    selection_weight: 1
{extra}"
        )
    }

    #[test]
    fn a_unit_with_no_hp_is_rejected() {
        let src = minimal("").replace("hp: 10", "hp: 0");
        assert!(units_from_yaml(&src).is_err());
    }

    #[test]
    fn a_zero_cooldown_attack_is_rejected() {
        let src = minimal(
            "    attack: { damage: 1, range: 1.0, cooldown_ticks: 0, frontswing_ticks: 0, backswing_ticks: 0, arc: 0.25 }\n",
        );
        let err = units_from_yaml(&src).unwrap_err().to_string();
        assert!(err.contains("cooldown_ticks"), "{err}");
    }

    #[test]
    fn an_attack_that_cannot_finish_its_swing_is_rejected() {
        let src = minimal(
            "    attack: { damage: 1, range: 1.0, cooldown_ticks: 5, frontswing_ticks: 4, backswing_ticks: 4, arc: 0.25 }\n",
        );
        let err = units_from_yaml(&src).unwrap_err().to_string();
        assert!(err.contains("exceeds cooldown"), "{err}");
    }

    #[test]
    fn a_negative_radius_is_rejected() {
        let src = minimal("").replace("collider_radius: 0.5", "collider_radius: -1.0");
        assert!(units_from_yaml(&src).is_err());
    }

    #[test]
    fn garbage_errors_and_does_not_panic() {
        for src in ["", "{}", "murmur: []", "- - -", "\u{0}", "murmur: [1,2,3]"] {
            assert!(units_from_yaml(src).is_err(), "expected error: {src:?}");
        }
    }
}
