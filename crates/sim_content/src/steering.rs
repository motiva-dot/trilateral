//! `steering.yaml` → [`SteeringParams`]. TECH_SPEC §5.2.
//!
//! Small, and the most consequential file in the content directory for how the
//! game *feels*. PRD §7: clumping and shoving are tunable on purpose, because
//! some friction creates positional skill.

use sim_core::SteeringParams;
use trilateral_fixed::Fixed;

use crate::tech::ContentError;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSteering {
    separation_response: f64,
    settle_stuck_ticks: u8,
    settle_progress_fraction: f64,
    max_neighbours: u16,
    max_paths_per_tick: u16,
}

pub fn steering_from_yaml(src: &str) -> Result<SteeringParams, ContentError> {
    let raw: RawSteering =
        serde_saphyr::from_str(src).map_err(|e| ContentError::Parse(e.to_string()))?;

    let bad = |reason: &str| ContentError::BadEffect {
        tech: "steering.yaml".into(),
        reason: reason.to_string(),
    };
    // A zero or negative response means units never separate; above 1.0 they
    // over-correct and oscillate. Both look like a physics bug rather than a
    // tuning choice, so neither is allowed to reach the simulation.
    if !(raw.separation_response > 0.0 && raw.separation_response <= 1.0) {
        return Err(bad("separation_response must be in (0, 1]"));
    }
    if raw.settle_stuck_ticks == 0 {
        return Err(bad(
            "settle_stuck_ticks must be non-zero, or units settle instantly",
        ));
    }
    if !(raw.settle_progress_fraction >= 0.0 && raw.settle_progress_fraction < 1.0) {
        return Err(bad("settle_progress_fraction must be in [0, 1)"));
    }
    if raw.max_neighbours == 0 {
        return Err(bad(
            "max_neighbours must be non-zero, or nothing ever collides",
        ));
    }
    Ok(SteeringParams {
        separation_response: Fixed::from_f64(raw.separation_response),
        settle_stuck_ticks: raw.settle_stuck_ticks,
        settle_progress_fraction: Fixed::from_f64(raw.settle_progress_fraction),
        max_neighbours: raw.max_neighbours,
        max_paths_per_tick: raw.max_paths_per_tick,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL: &str = include_str!("../../../assets/data/steering.yaml");

    #[test]
    fn the_real_steering_file_loads() {
        let p = steering_from_yaml(REAL).unwrap_or_else(|e| panic!("steering.yaml: {e}"));
        assert_eq!(p.settle_stuck_ticks, 12);
        assert_eq!(p.max_neighbours, 16);
        assert_eq!(p.separation_response, Fixed::from_f64(0.45));
    }

    #[test]
    fn the_settle_delay_is_a_sane_fraction_of_a_second() {
        // 12 ticks at 30Hz = 0.4s. Long enough that a brief squeeze does not
        // cancel a real order; short enough that a jam does not look broken.
        let p = steering_from_yaml(REAL).unwrap();
        assert!((6..=45).contains(&(p.settle_stuck_ticks as u32)));
    }

    #[test]
    fn out_of_range_values_are_rejected() {
        for (field, value) in [
            ("separation_response", "0.0"),
            ("separation_response", "1.5"),
            ("settle_stuck_ticks", "0"),
            ("settle_progress_fraction", "1.0"),
            ("max_neighbours", "0"),
        ] {
            let src =
                "separation_response: 0.45\nsettle_stuck_ticks: 12\nsettle_progress_fraction: 0.25\nmax_neighbours: 16"
            .lines()
            .map(|l| {
                if l.starts_with(field) {
                    format!("{field}: {value}")
                } else {
                    l.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
            assert!(
                steering_from_yaml(&src).is_err(),
                "{field}: {value} should be rejected"
            );
        }
    }

    #[test]
    fn garbage_errors_and_does_not_panic() {
        for src in ["", "{}", "[]", "\u{0}", "separation_response: banana"] {
            assert!(steering_from_yaml(src).is_err(), "expected error: {src:?}");
        }
    }
}
