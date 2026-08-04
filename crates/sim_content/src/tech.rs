//! `tech_tree.yaml` → `TechRegistry`. GAME_DESIGN §3, CLAUDE.md §5.5.
//!
//! Authored YAML is untrusted input. MARKET_POSITION differentiator #4 makes
//! balance community-forkable, so these loaders will one day be fed files
//! written by strangers. Every failure mode here must be a returned error with
//! a useful message, never a panic and never a silent default.

use std::collections::BTreeMap;

use serde::Deserialize;
use trilateral_fixed::Fixed;

/// Which tier gate a tech sits behind.
pub type Tier = u8;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TechKind {
    UnitUnlock,
    Upgrade,
    AbilityUnlock,
    Mechanic,
}

/// Cost as authored. Ore and Flux are whole numbers; time is whole ticks
/// (30 ticks = 1 second), never seconds — CLAUDE.md §1.1.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cost {
    #[serde(default)]
    pub ore: u32,
    #[serde(default)]
    pub flux: u32,
    pub ticks: u32,
}

/// A single modifier spec, as authored.
///
/// The variants are mutually exclusive but YAML cannot express that, so the
/// fields are all optional and [`RawEffect::resolve`] enforces "exactly one".
/// Accepting two would silently drop one of them.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawEffect {
    pub stat: String,
    pub applies_to: String,
    #[serde(default)]
    pub add: Option<f64>,
    #[serde(default)]
    pub mult: Option<f64>,
    #[serde(default)]
    pub mult_on_bloom: Option<f64>,
    #[serde(default)]
    pub set: Option<bool>,
}

/// A modifier spec after conversion to fixed point.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum EffectValue {
    /// `clamp(base + sum(add))`
    Add(Fixed),
    /// `* mult`
    Mult(Fixed),
    /// `* mult`, but only while the unit stands on Bloom (Murmur).
    MultOnBloom(Fixed),
    /// Boolean capability flip, e.g. `warp_in_enabled`.
    Set(bool),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Effect {
    pub stat: String,
    pub applies_to: String,
    pub value: EffectValue,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantsAbility {
    pub ability: String,
    pub to: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Track {
    pub next: String,
}

/// One tech as authored in YAML.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawTech {
    pub id: String,
    pub tier: Tier,
    pub kind: TechKind,
    pub cost: Cost,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub effects: Vec<RawEffect>,
    #[serde(default)]
    pub unlocks_building: Option<String>,
    #[serde(default)]
    pub unlocks_unit: Option<String>,
    #[serde(default)]
    pub grants_ability: Option<GrantsAbility>,
    #[serde(default)]
    pub track: Option<Track>,
}

#[derive(Clone, Debug)]
pub struct Tech {
    pub id: String,
    pub race: String,
    pub tier: Tier,
    pub kind: TechKind,
    pub cost: Cost,
    pub requires: Vec<String>,
    pub effects: Vec<Effect>,
    pub unlocks_building: Option<String>,
    pub unlocks_unit: Option<String>,
    pub grants_ability: Option<GrantsAbility>,
    pub track: Option<Track>,
}

/// The whole file: race name → its techs.
///
/// `BTreeMap`, not `HashMap` — §1.2 bans hash containers from sim crates, and
/// while `sim_content` is a loader rather than the sim itself, registry
/// iteration order feeds registry *indices*, which are gameplay. A `HashMap`
/// here would make `ArchetypeId` assignment depend on hasher seeding.
pub type RawTechTree = BTreeMap<String, Vec<RawTech>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContentError {
    Parse(String),
    /// The file parsed but yielded no techs at all.
    ///
    /// Found via a self-referential YAML alias (`&a *a`), which parses to a
    /// null document and deserialises to an empty map — no panic, but an
    /// empty registry silently means "this race has no tech tree", which no
    /// legitimate content file means. Loading nothing is a failure, not a
    /// successful load of nothing.
    Empty,
    /// Two techs share an id; the second would silently shadow the first.
    DuplicateId(String),
    /// `requires` or `track.next` names a tech that does not exist.
    UnknownReference {
        tech: String,
        missing: String,
    },
    /// An effect specified none, or more than one, of add/mult/set.
    BadEffect {
        tech: String,
        reason: String,
    },
}

impl core::fmt::Display for ContentError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ContentError::Parse(m) => write!(f, "YAML parse error: {m}"),
            ContentError::Empty => write!(f, "tech tree contains no techs"),
            ContentError::DuplicateId(id) => write!(f, "duplicate tech id `{id}`"),
            ContentError::UnknownReference { tech, missing } => {
                write!(f, "tech `{tech}` references unknown id `{missing}`")
            }
            ContentError::BadEffect { tech, reason } => {
                write!(f, "tech `{tech}` has a bad effect: {reason}")
            }
        }
    }
}

impl std::error::Error for ContentError {}

impl RawEffect {
    /// Enforce "exactly one of add / mult / mult_on_bloom / set".
    fn resolve(&self, tech: &str) -> Result<Effect, ContentError> {
        let mut found: Vec<EffectValue> = Vec::new();
        if let Some(v) = self.add {
            found.push(EffectValue::Add(Fixed::from_f64(v)));
        }
        if let Some(v) = self.mult {
            found.push(EffectValue::Mult(Fixed::from_f64(v)));
        }
        if let Some(v) = self.mult_on_bloom {
            found.push(EffectValue::MultOnBloom(Fixed::from_f64(v)));
        }
        if let Some(v) = self.set {
            found.push(EffectValue::Set(v));
        }
        match found.len() {
            1 => Ok(Effect {
                stat: self.stat.clone(),
                applies_to: self.applies_to.clone(),
                value: found.pop().expect("length checked"),
            }),
            0 => Err(ContentError::BadEffect {
                tech: tech.to_string(),
                reason: format!("effect on `{}` specifies no value", self.stat),
            }),
            n => Err(ContentError::BadEffect {
                tech: tech.to_string(),
                reason: format!(
                    "effect on `{}` specifies {n} values; exactly one of \
                     add/mult/mult_on_bloom/set is allowed",
                    self.stat
                ),
            }),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct TechRegistry {
    /// Ordered by (race, authored order). Index is the future `TechId`.
    techs: Vec<Tech>,
    by_id: BTreeMap<String, usize>,
}

impl TechRegistry {
    /// Parse and validate a `tech_tree.yaml`.
    pub fn from_yaml(src: &str) -> Result<TechRegistry, ContentError> {
        let raw: RawTechTree =
            serde_saphyr::from_str(src).map_err(|e| ContentError::Parse(e.to_string()))?;
        TechRegistry::from_raw(raw)
    }

    pub fn from_raw(raw: RawTechTree) -> Result<TechRegistry, ContentError> {
        let mut techs = Vec::new();
        let mut by_id: BTreeMap<String, usize> = BTreeMap::new();

        // BTreeMap iteration is sorted by race name, so index assignment is a
        // pure function of the file contents — not of insertion order and not
        // of a hasher.
        for (race, entries) in &raw {
            for e in entries {
                if by_id.contains_key(&e.id) {
                    return Err(ContentError::DuplicateId(e.id.clone()));
                }
                let effects = e
                    .effects
                    .iter()
                    .map(|r| r.resolve(&e.id))
                    .collect::<Result<Vec<_>, _>>()?;
                by_id.insert(e.id.clone(), techs.len());
                techs.push(Tech {
                    id: e.id.clone(),
                    race: race.clone(),
                    tier: e.tier,
                    kind: e.kind,
                    cost: e.cost,
                    requires: e.requires.clone(),
                    effects,
                    unlocks_building: e.unlocks_building.clone(),
                    unlocks_unit: e.unlocks_unit.clone(),
                    grants_ability: e.grants_ability.clone(),
                    track: e.track.clone(),
                });
            }
        }

        if techs.is_empty() {
            return Err(ContentError::Empty);
        }
        let reg = TechRegistry { techs, by_id };
        reg.validate_references()?;
        Ok(reg)
    }

    /// `requires` may name buildings as well as techs, so only `track.next` is
    /// checked strictly here. Building ids are validated once
    /// `BuildingRegistry` exists — flagged rather than silently skipped.
    fn validate_references(&self) -> Result<(), ContentError> {
        for t in &self.techs {
            if let Some(track) = &t.track
                && !self.by_id.contains_key(&track.next)
            {
                return Err(ContentError::UnknownReference {
                    tech: t.id.clone(),
                    missing: track.next.clone(),
                });
            }
        }
        Ok(())
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.techs.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.techs.is_empty()
    }

    #[inline]
    pub fn get(&self, id: &str) -> Option<&Tech> {
        self.by_id.get(id).map(|&i| &self.techs[i])
    }

    #[inline]
    pub fn by_index(&self, i: usize) -> Option<&Tech> {
        self.techs.get(i)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Tech> {
        self.techs.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real authored file, compiled in so the test cannot drift from it.
    const REAL_TECH_TREE: &str = include_str!("../../../assets/data/tech_tree.yaml");

    #[test]
    fn the_real_tech_tree_parses() {
        let reg = TechRegistry::from_yaml(REAL_TECH_TREE)
            .unwrap_or_else(|e| panic!("assets/data/tech_tree.yaml failed to load: {e}"));
        assert!(reg.len() >= 20, "only {} techs loaded", reg.len());
    }

    #[test]
    fn known_techs_load_with_the_authored_numbers() {
        let reg = TechRegistry::from_yaml(REAL_TECH_TREE).unwrap();
        let t = reg.get("mur_weapons_1").expect("mur_weapons_1 missing");
        assert_eq!(t.race, "murmur");
        assert_eq!(t.tier, 2);
        assert_eq!(t.kind, TechKind::Upgrade);
        assert_eq!(t.cost.ore, 100);
        assert_eq!(t.cost.flux, 100);
        assert_eq!(t.cost.ticks, 2400);
        assert_eq!(t.effects.len(), 1);
        assert_eq!(t.effects[0].value, EffectValue::Add(Fixed::from_int(1)));
        assert_eq!(t.track.as_ref().unwrap().next, "mur_weapons_2");
    }

    #[test]
    fn placeholder_weapon_ladders_are_flat_hundreds() {
        // Bastion and Concord weapon tracks are PLACEHOLDER values pending a
        // balance pass: a deliberately generic 100/200/300 in both resources.
        // Pinned so they cannot drift unnoticed before that pass happens.
        let reg = TechRegistry::from_yaml(REAL_TECH_TREE).unwrap();
        for race in ["bas", "con"] {
            for (level, expected) in [(1, 100), (2, 200), (3, 300)] {
                let id = format!("{race}_weapons_{level}");
                let t = reg.get(&id).unwrap_or_else(|| panic!("{id} missing"));
                assert_eq!(t.cost.ore, expected, "{id} ore");
                assert_eq!(t.cost.flux, expected, "{id} flux");
            }
        }
    }

    #[test]
    fn every_track_chain_terminates() {
        // A track cycle would make the upgrade path loop forever once the UI
        // walks it. Reference validity is checked at load; this checks shape.
        let reg = TechRegistry::from_yaml(REAL_TECH_TREE).unwrap();
        for start in reg.iter() {
            let mut seen = vec![start.id.as_str()];
            let mut cur = start;
            while let Some(track) = &cur.track {
                let next = reg.get(&track.next).expect("validated at load");
                assert!(
                    !seen.contains(&next.id.as_str()),
                    "track cycle reaching {} from {}",
                    next.id,
                    start.id
                );
                seen.push(next.id.as_str());
                cur = next;
            }
        }
    }

    #[test]
    fn all_three_races_are_present() {
        let reg = TechRegistry::from_yaml(REAL_TECH_TREE).unwrap();
        for race in ["murmur", "bastion", "concord"] {
            assert!(
                reg.iter().any(|t| t.race == race),
                "no techs found for {race}"
            );
        }
    }

    #[test]
    fn fractional_multipliers_survive_the_float_bridge_exactly() {
        let reg = TechRegistry::from_yaml(REAL_TECH_TREE).unwrap();
        let t = reg.get("mur_mite_frenzy").unwrap();
        // 0.85 authored -> nearest Q32.32. 0.85 * 2^32 = 3650722201.6 -> 3650722202.
        assert_eq!(
            t.effects[0].value,
            EffectValue::Mult(Fixed::from_bits(3_650_722_202))
        );
    }

    #[test]
    fn a_boolean_set_effect_loads() {
        let reg = TechRegistry::from_yaml(REAL_TECH_TREE).unwrap();
        let t = reg.get("con_warp_resonance").unwrap();
        assert_eq!(t.effects[0].value, EffectValue::Set(true));
    }

    #[test]
    fn registry_indices_are_stable_across_loads() {
        // Index becomes TechId, which becomes gameplay. Two loads of the same
        // bytes must agree, or two clients disagree about what tech 7 is.
        let a = TechRegistry::from_yaml(REAL_TECH_TREE).unwrap();
        let b = TechRegistry::from_yaml(REAL_TECH_TREE).unwrap();
        let ids_a: Vec<&str> = a.iter().map(|t| t.id.as_str()).collect();
        let ids_b: Vec<&str> = b.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids_a, ids_b);
    }

    // ---- malformed input must error, never panic -------------------------

    #[test]
    fn duplicate_ids_are_rejected() {
        let src = "
murmur:
  - { id: dup, tier: 1, kind: upgrade, cost: { ore: 1, flux: 0, ticks: 1 } }
  - { id: dup, tier: 1, kind: upgrade, cost: { ore: 1, flux: 0, ticks: 1 } }
";
        assert_eq!(
            TechRegistry::from_yaml(src).unwrap_err(),
            ContentError::DuplicateId("dup".into())
        );
    }

    #[test]
    fn an_effect_with_two_values_is_rejected() {
        let src = "
murmur:
  - id: t
    tier: 1
    kind: upgrade
    cost: { ore: 1, flux: 0, ticks: 1 }
    effects:
      - { stat: s, applies_to: a, add: 1.0, mult: 2.0 }
";
        let err = TechRegistry::from_yaml(src).unwrap_err();
        match err {
            ContentError::BadEffect { tech, reason } => {
                assert_eq!(tech, "t");
                assert!(reason.contains("2 values"), "unhelpful message: {reason}");
            }
            other => panic!("wrong error: {other}"),
        }
    }

    #[test]
    fn an_effect_with_no_value_is_rejected() {
        let src = "
murmur:
  - id: t
    tier: 1
    kind: upgrade
    cost: { ore: 1, flux: 0, ticks: 1 }
    effects:
      - { stat: s, applies_to: a }
";
        assert!(matches!(
            TechRegistry::from_yaml(src),
            Err(ContentError::BadEffect { .. })
        ));
    }

    #[test]
    fn a_dangling_track_reference_is_rejected() {
        let src = "
murmur:
  - id: t
    tier: 1
    kind: upgrade
    cost: { ore: 1, flux: 0, ticks: 1 }
    track: { next: nope }
";
        assert_eq!(
            TechRegistry::from_yaml(src).unwrap_err(),
            ContentError::UnknownReference {
                tech: "t".into(),
                missing: "nope".into()
            }
        );
    }

    #[test]
    fn an_unknown_field_is_rejected_rather_than_ignored() {
        // deny_unknown_fields: a community balance mod with a typo should be
        // told about it, not silently have the line dropped.
        let src = "
murmur:
  - id: t
    tier: 1
    kind: upgrade
    cost: { ore: 1, flux: 0, ticks: 1 }
    typo_field: 3
";
        assert!(matches!(
            TechRegistry::from_yaml(src),
            Err(ContentError::Parse(_))
        ));
    }

    #[test]
    fn garbage_input_errors_and_does_not_panic() {
        for src in [
            "\u{0}\u{1}\u{2}",
            "murmur: [",
            "- - - -",
            "murmur:\n  - id: 3.5",
            ": :",
            // A self-referential alias. Parses to a null document without
            // panicking — the reason `ContentError::Empty` exists.
            "&a *a",
            "",
            "{}",
        ] {
            let r = TechRegistry::from_yaml(src);
            assert!(r.is_err(), "expected an error for {src:?}");
        }
    }

    #[test]
    fn a_file_that_yields_no_techs_is_an_error() {
        assert_eq!(
            TechRegistry::from_yaml("{}").unwrap_err(),
            ContentError::Empty
        );
        assert_eq!(
            TechRegistry::from_yaml("murmur: []").unwrap_err(),
            ContentError::Empty
        );
    }
}
