//! `races.yaml` → [`RaceParams`]. GAME_DESIGN §2.
//!
//! Small file, large consequences: this is where "races differ in how you
//! macro" stops being a design statement and becomes numbers.

use sim_core::{BroodParams, RaceParams};

use crate::tech::ContentError;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMurmur {
    brood_point_period_ticks: u32,
    brood_stockpile_cap_per_base: u16,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRaces {
    murmur: RawMurmur,
}

pub fn races_from_yaml(src: &str) -> Result<RaceParams, ContentError> {
    let raw: RawRaces =
        serde_saphyr::from_str(src).map_err(|e| ContentError::Parse(e.to_string()))?;
    let bad = |reason: &str| ContentError::BadEffect {
        tech: "races.yaml".into(),
        reason: reason.to_string(),
    };
    if raw.murmur.brood_point_period_ticks == 0 {
        return Err(bad(
            "brood_point_period_ticks must be non-zero, or points accrue infinitely per tick",
        ));
    }
    if raw.murmur.brood_stockpile_cap_per_base == 0 {
        return Err(bad(
            "brood_stockpile_cap_per_base must be non-zero, or Murmur can never produce",
        ));
    }
    Ok(RaceParams {
        brood: BroodParams {
            period_ticks: raw.murmur.brood_point_period_ticks,
            stockpile_cap_per_base: raw.murmur.brood_stockpile_cap_per_base,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL: &str = include_str!("../../../assets/data/races.yaml");

    #[test]
    fn the_real_races_file_loads() {
        let p = races_from_yaml(REAL).unwrap_or_else(|e| panic!("races.yaml: {e}"));
        assert_eq!(p.brood.period_ticks, 90, "3 seconds at 30Hz");
        assert_eq!(p.brood.stockpile_cap_per_base, 3);
    }

    #[test]
    fn zero_values_are_rejected() {
        for src in [
            "murmur:\n  brood_point_period_ticks: 0\n  brood_stockpile_cap_per_base: 3",
            "murmur:\n  brood_point_period_ticks: 90\n  brood_stockpile_cap_per_base: 0",
        ] {
            assert!(races_from_yaml(src).is_err(), "accepted: {src}");
        }
    }

    #[test]
    fn garbage_errors_and_does_not_panic() {
        for src in ["", "{}", "[]", "\u{0}", "murmur: banana"] {
            assert!(races_from_yaml(src).is_err(), "expected error: {src:?}");
        }
    }
}
