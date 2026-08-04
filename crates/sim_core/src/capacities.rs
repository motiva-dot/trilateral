//! `Capacities` — the buffer sizes read from `assets/data/engine.yaml`.
//!
//! CLAUDE.md §1.4: every simulation buffer is pre-sized at match start from
//! YAML-declared maxima and never grows. ADR-005 made these runtime values
//! rather than Rust constants, because a `const MAX_ENTITIES` is a gameplay
//! number hardcoded in Rust and §1.8 forbids those.
//!
//! These are engine limits, not balance. They belong in `engine.yaml` rather
//! than `units.yaml` precisely because changing one is a performance decision,
//! not a design one.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capacities {
    /// Component array length. Every entity, projectile-owner and building
    /// occupies one slot.
    pub max_entities: u32,
    pub max_projectiles: u32,
    /// Per player, per tick. The anti-flood bound at the command ingest
    /// boundary, which is also the anti-cheat boundary for networked play.
    pub max_commands_per_tick: u32,
    pub max_players: u8,
    /// Fixed-capacity per-entity command ring.
    pub cmd_queue_slots: u8,
    /// Fixed-capacity per-entity modifier stack.
    pub modifier_slots: u8,
    /// Entries reserved up front in the `CommandLog`.
    ///
    /// The log is append-only and grows with match length, so it is the one
    /// buffer §1.4 cannot fully pre-size. Reserving covers a typical match;
    /// beyond it the vector doubles, which is rare and amortised but not free.
    /// See the note on `CommandLog` about the allocation gate.
    pub command_log_reserve: u32,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CapacityError {
    Zero(&'static str),
    TooLarge {
        field: &'static str,
        value: u32,
        limit: u32,
    },
}

impl core::fmt::Display for CapacityError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CapacityError::Zero(field) => write!(f, "engine.yaml: `{field}` must be non-zero"),
            CapacityError::TooLarge {
                field,
                value,
                limit,
            } => write!(f, "engine.yaml: `{field}` is {value}, limit is {limit}"),
        }
    }
}

impl std::error::Error for CapacityError {}

impl Capacities {
    /// Upper bound on `max_entities`.
    ///
    /// The allocator reserves two u32 sentinels, and every index must also fit
    /// an `EntityIndex`. Well above any plausible value — TECH_SPEC §3 puts
    /// the prototype at 2048 and the ship target at 4000 entities.
    pub const MAX_ENTITIES_LIMIT: u32 = u32::MAX - 2;

    pub fn validate(&self) -> Result<(), CapacityError> {
        if self.max_entities == 0 {
            return Err(CapacityError::Zero("max_entities"));
        }
        if self.max_entities > Self::MAX_ENTITIES_LIMIT {
            return Err(CapacityError::TooLarge {
                field: "max_entities",
                value: self.max_entities,
                limit: Self::MAX_ENTITIES_LIMIT,
            });
        }
        if self.max_players == 0 {
            return Err(CapacityError::Zero("max_players"));
        }
        if self.cmd_queue_slots == 0 {
            return Err(CapacityError::Zero("cmd_queue_slots"));
        }
        if self.max_commands_per_tick == 0 {
            return Err(CapacityError::Zero("max_commands_per_tick"));
        }
        Ok(())
    }

    pub fn hash_into(&self, h: &mut crate::hash::SimHasher) {
        h.write_u32(self.max_entities);
        h.write_u32(self.max_projectiles);
        h.write_u32(self.max_commands_per_tick);
        h.write_u8(self.max_players);
        h.write_u8(self.cmd_queue_slots);
        h.write_u8(self.modifier_slots);
        h.write_u32(self.command_log_reserve);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test fixture. Not a default — there is deliberately no
    /// `Capacities::default()`, because a default would let a missing
    /// `engine.yaml` silently produce a differently-sized simulation.
    pub(crate) fn fixture(max_entities: u32) -> Capacities {
        Capacities {
            max_entities,
            max_projectiles: 64,
            max_commands_per_tick: 256,
            max_players: 8,
            cmd_queue_slots: 16,
            modifier_slots: 8,
            command_log_reserve: 1024,
        }
    }

    #[test]
    fn a_valid_set_validates() {
        assert_eq!(fixture(2048).validate(), Ok(()));
    }

    #[test]
    fn zero_entities_is_rejected() {
        assert_eq!(
            fixture(0).validate(),
            Err(CapacityError::Zero("max_entities"))
        );
    }

    #[test]
    fn absurd_entity_counts_are_rejected() {
        let c = fixture(u32::MAX);
        assert!(matches!(c.validate(), Err(CapacityError::TooLarge { .. })));
    }

    #[test]
    fn zero_players_is_rejected() {
        let mut c = fixture(16);
        c.max_players = 0;
        assert_eq!(c.validate(), Err(CapacityError::Zero("max_players")));
    }

    #[test]
    fn error_messages_name_the_yaml_field() {
        // These messages are what a modder sees. They must say where to look.
        let msg = fixture(0).validate().unwrap_err().to_string();
        assert!(msg.contains("engine.yaml"), "{msg}");
        assert!(msg.contains("max_entities"), "{msg}");
    }
}
