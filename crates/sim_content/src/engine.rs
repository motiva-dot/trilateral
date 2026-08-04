//! `engine.yaml` → [`Capacities`].
//!
//! Small, but it is the file that decides how big the simulation is, so its
//! failure modes matter more than its size. A missing or malformed
//! `engine.yaml` must stop the match with a message naming the field — never
//! fall back to a default, because two clients silently choosing different
//! defaults is a desync at the first spawn.

use sim_core::Capacities;

use crate::tech::ContentError;

/// Parse and validate. Both steps, always — a `Capacities` that parsed but did
/// not validate is exactly the kind of thing that fails at tick 0 with a
/// confusing panic instead of at load with a clear message.
pub fn capacities_from_yaml(src: &str) -> Result<Capacities, ContentError> {
    let caps: Capacities =
        serde_saphyr::from_str(src).map_err(|e| ContentError::Parse(e.to_string()))?;
    caps.validate()
        .map_err(|e| ContentError::Parse(e.to_string()))?;
    Ok(caps)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL_ENGINE_YAML: &str = include_str!("../../../assets/data/engine.yaml");

    #[test]
    fn the_real_engine_yaml_loads() {
        let c = capacities_from_yaml(REAL_ENGINE_YAML)
            .unwrap_or_else(|e| panic!("assets/data/engine.yaml failed: {e}"));
        assert_eq!(c.max_entities, 2048);
        assert_eq!(c.max_projectiles, 4096);
        assert_eq!(c.max_commands_per_tick, 256);
        assert_eq!(c.max_players, 8);
        assert_eq!(c.cmd_queue_slots, 16);
        assert_eq!(c.modifier_slots, 8);
    }

    #[test]
    fn the_real_capacities_exceed_the_prototype_entity_target() {
        // PRD §4 targets 1,200 entities at prototype scale. Running out of
        // slots mid-match drops spawns silently from the player's point of
        // view, so the buffer is deliberately above the target.
        let c = capacities_from_yaml(REAL_ENGINE_YAML).unwrap();
        assert!(c.max_entities >= 1_200, "max_entities below PRD target");
    }

    #[test]
    fn a_missing_field_is_an_error_not_a_default() {
        // The whole point: no silent defaults. Two clients defaulting
        // differently is a desync at the first spawn.
        let src = "max_entities: 64\n";
        assert!(capacities_from_yaml(src).is_err());
    }

    #[test]
    fn an_unknown_field_is_rejected() {
        let src = "
max_entities: 64
max_projectiles: 64
max_commands_per_tick: 16
max_players: 2
cmd_queue_slots: 4
modifier_slots: 2
max_entites: 99
";
        // Note the typo above — `max_entites`. Silently ignoring it would give
        // a simulation sized 64 while the author believed it was 99.
        assert!(capacities_from_yaml(src).is_err());
    }

    #[test]
    fn an_invalid_value_is_caught_at_load_with_a_useful_message() {
        let src = "
max_entities: 0
max_projectiles: 64
max_commands_per_tick: 16
max_players: 2
cmd_queue_slots: 4
modifier_slots: 2
";
        let err = capacities_from_yaml(src).unwrap_err().to_string();
        assert!(err.contains("max_entities"), "{err}");
    }

    #[test]
    fn garbage_errors_and_does_not_panic() {
        for src in ["", "{}", "[]", "\u{0}", "max_entities: banana"] {
            assert!(
                capacities_from_yaml(src).is_err(),
                "expected error: {src:?}"
            );
        }
    }
}
