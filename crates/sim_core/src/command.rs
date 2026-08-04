//! `Command` and the ingest boundary. CLAUDE.md §4, §1.7.
//!
//! # Two laws meet here
//! * **§1.7, the Replay Mandate.** Every variant is serde-serialisable, and
//!   anything that cannot be reproduced from `(seed, command log)` alone is
//!   rejected in review. If a player action does not appear here, it does not
//!   appear in replays, cannot be reconnected through, and cannot be trained
//!   against by a bot. There is no second channel.
//! * **§4, validation at ingest.** An invalid command is dropped and logged,
//!   never a panic. This is also the anti-cheat boundary for networked play:
//!   a peer can send anything, and "anything" must be survivable.
//!
//! # One command, one entity
//! A player selecting forty units and right-clicking produces forty
//! `IssuedCommand`s, not one command with forty subjects. It costs log space,
//! and buys a model where every command has exactly one subject to validate
//! against — no partial application, no "succeeded for 38 of 40" state to
//! reconcile between clients. The client-side expansion is presentation's job.

use serde::{Deserialize, Serialize};
use trilateral_fixed::FixedVec2;

use crate::clock::Tick;
use crate::ids::{ArchetypeId, EntityHandle, PlayerId, TechId};
use crate::state::SimState;

/// Index into the ability registry loaded from YAML.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize,
)]
#[repr(transparent)]
pub struct AbilityId(pub u16);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum AbilityTarget {
    None,
    Position(FixedVec2),
    Entity(EntityHandle),
}

/// Exactly the list in CLAUDE.md §4. Do not add a variant without adding it
/// there too — §4 is the ubiquitous language, and a command the spec does not
/// name is a command reviewers will not look for.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Command {
    Move {
        target: FixedVec2,
    },
    AttackMove {
        target: FixedVec2,
    },
    AttackTarget {
        target: EntityHandle,
    },
    Stop,
    HoldPosition,
    Patrol {
        waypoint: FixedVec2,
    },
    Harvest {
        node: EntityHandle,
    },
    ReturnCargo,
    Build {
        building: ArchetypeId,
        pos: FixedVec2,
    },
    Train {
        unit: ArchetypeId,
    },
    Research {
        tech: TechId,
    },
    Cancel {
        queue_slot: u8,
    },
    UseAbility {
        ability: AbilityId,
        target: AbilityTarget,
    },
    SetRally {
        pos: FixedVec2,
    },
    Surrender,
}

/// A command stamped for execution.
///
/// `tick` is the *execution* tick, not the issue tick — §5.1 schedules
/// commands at `issue_tick + input_delay` so every client runs them
/// simultaneously.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct IssuedCommand {
    pub tick: Tick,
    pub player: PlayerId,
    pub subject: EntityHandle,
    pub command: Command,
}

/// Why a command was dropped.
///
/// Kept as data rather than a string so the desync tracer and the anti-cheat
/// telemetry can count them by kind.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Reject {
    /// Subject does not exist, or the handle is stale.
    DeadSubject,
    /// Subject exists but belongs to someone else. On a network this is either
    /// a bug or an attack; either way it is dropped, not obeyed.
    NotOwned,
    /// A targeted command naming a dead or stale entity.
    DeadTarget,
    /// A player id outside `capacities.max_players`.
    NoSuchPlayer,
    /// `Cancel` naming a slot beyond `capacities.cmd_queue_slots`.
    BadQueueSlot,
    /// Commanding a neutral entity — resource nodes take no orders.
    NeutralSubject,
}

/// Validate a command against current state.
///
/// # What is NOT checked yet, and where it lands
/// * cost and prerequisites — needs the registries (Phase 6/7)
/// * fog legality for targeted commands — needs `FogMasks` (Phase 6)
/// * placement legality for `Build` — needs `MacroGrid` (Phase 6)
///
/// These are named rather than silently absent because §4 lists them as part
/// of the ingest contract, and a reader should be able to tell the difference
/// between "checked" and "not written yet".
pub fn validate(state: &SimState, cmd: &IssuedCommand) -> Result<(), Reject> {
    if cmd.player.0 as u32 >= state.capacities.max_players as u32 {
        return Err(Reject::NoSuchPlayer);
    }
    if !state.is_alive(cmd.subject) {
        return Err(Reject::DeadSubject);
    }
    let owner = state.c.owner[cmd.subject.index as usize];
    if owner == PlayerId::NEUTRAL {
        return Err(Reject::NeutralSubject);
    }
    if owner != cmd.player {
        return Err(Reject::NotOwned);
    }
    match cmd.command {
        Command::AttackTarget { target } | Command::Harvest { node: target }
            if !state.is_alive(target) =>
        {
            Err(Reject::DeadTarget)
        }
        Command::UseAbility {
            target: AbilityTarget::Entity(target),
            ..
        } if !state.is_alive(target) => Err(Reject::DeadTarget),
        Command::Cancel { queue_slot } if queue_slot >= state.capacities.cmd_queue_slots => {
            Err(Reject::BadQueueSlot)
        }
        _ => Ok(()),
    }
}

/// The append-only record that §1.7 is about.
///
/// # Allocation
/// §1.4 bans steady-state allocation, and an append-only log obviously grows.
/// It is pre-reserved at construction from `engine.yaml`'s
/// `command_log_reserve`; beyond that it doubles, which is rare and amortised
/// but not free. When the allocation gate revives, either the reserve covers
/// the benchmark match or the log is excluded from the count deliberately —
/// recorded as an open item rather than discovered by a red CI job.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct CommandLog {
    entries: Vec<IssuedCommand>,
    rejected: u32,
}

impl CommandLog {
    pub fn with_reserve(reserve: u32) -> CommandLog {
        CommandLog {
            entries: Vec::with_capacity(reserve as usize),
            rejected: 0,
        }
    }

    /// Record an accepted command. Order of append is execution order.
    pub fn push(&mut self, cmd: IssuedCommand) {
        self.entries.push(cmd);
    }

    /// Record that a command was dropped.
    ///
    /// The count is part of hashed state on purpose: two clients disagreeing
    /// about how many commands were *invalid* have diverged just as surely as
    /// two disagreeing about unit positions, and this makes it visible at the
    /// next checkpoint rather than never.
    pub fn record_rejection(&mut self) {
        self.rejected += 1;
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[inline]
    pub fn rejected_count(&self) -> u32 {
        self.rejected
    }

    pub fn iter(&self) -> impl Iterator<Item = &IssuedCommand> {
        self.entries.iter()
    }

    /// Commands scheduled for exactly this tick, in append order.
    pub fn for_tick(&self, tick: Tick) -> impl Iterator<Item = &IssuedCommand> {
        self.entries.iter().filter(move |c| c.tick == tick)
    }

    pub fn hash_into(&self, h: &mut crate::hash::SimHasher) {
        h.write_u64(self.entries.len() as u64);
        h.write_u32(self.rejected);
        for e in &self.entries {
            h.write_u64(e.tick.0);
            h.write_u8(e.player.0);
            h.write_u32(e.subject.index);
            h.write_u32(e.subject.generation);
            e.command.hash_into(h);
        }
    }
}

impl Command {
    /// Discriminant, pinned. These bytes land in the state hash, so reordering
    /// the enum is a determinism break and a replay invalidation.
    pub const fn tag(self) -> u8 {
        match self {
            Command::Move { .. } => 0,
            Command::AttackMove { .. } => 1,
            Command::AttackTarget { .. } => 2,
            Command::Stop => 3,
            Command::HoldPosition => 4,
            Command::Patrol { .. } => 5,
            Command::Harvest { .. } => 6,
            Command::ReturnCargo => 7,
            Command::Build { .. } => 8,
            Command::Train { .. } => 9,
            Command::Research { .. } => 10,
            Command::Cancel { .. } => 11,
            Command::UseAbility { .. } => 12,
            Command::SetRally { .. } => 13,
            Command::Surrender => 14,
        }
    }

    fn hash_into(self, h: &mut crate::hash::SimHasher) {
        h.write_u8(self.tag());
        match self {
            Command::Move { target }
            | Command::AttackMove { target }
            | Command::Patrol { waypoint: target }
            | Command::SetRally { pos: target } => {
                h.write_i64(target.x.to_bits());
                h.write_i64(target.y.to_bits());
            }
            Command::AttackTarget { target } | Command::Harvest { node: target } => {
                h.write_u32(target.index);
                h.write_u32(target.generation);
            }
            Command::Build { building, pos } => {
                h.write_u32(building.0 as u32);
                h.write_i64(pos.x.to_bits());
                h.write_i64(pos.y.to_bits());
            }
            Command::Train { unit } => h.write_u32(unit.0 as u32),
            Command::Research { tech } => h.write_u32(tech.0 as u32),
            Command::Cancel { queue_slot } => h.write_u8(queue_slot),
            Command::UseAbility { ability, target } => {
                h.write_u32(ability.0 as u32);
                match target {
                    AbilityTarget::None => h.write_u8(0),
                    AbilityTarget::Position(p) => {
                        h.write_u8(1);
                        h.write_i64(p.x.to_bits());
                        h.write_i64(p.y.to_bits());
                    }
                    AbilityTarget::Entity(e) => {
                        h.write_u8(2);
                        h.write_u32(e.index);
                        h.write_u32(e.generation);
                    }
                }
            }
            Command::Stop | Command::HoldPosition | Command::ReturnCargo | Command::Surrender => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capacities::Capacities;
    use crate::ids::ArchetypeId;
    use crate::state::Spawn;

    fn caps() -> Capacities {
        Capacities {
            max_entities: 32,
            max_projectiles: 8,
            max_commands_per_tick: 16,
            max_players: 4,
            cmd_queue_slots: 16,
            modifier_slots: 8,
            command_log_reserve: 1024,
        }
    }

    fn state_with_two_players() -> (SimState, EntityHandle, EntityHandle, EntityHandle) {
        let mut s = SimState::new(caps(), 1, 64);
        let mine = s
            .spawn(Spawn {
                archetype: ArchetypeId(1),
                owner: PlayerId(0),
                pos: FixedVec2::ZERO,
                hp: 100,
                resource: 0,
            })
            .unwrap();
        let theirs = s
            .spawn(Spawn {
                archetype: ArchetypeId(1),
                owner: PlayerId(1),
                pos: FixedVec2::from_ints(10, 10),
                hp: 100,
                resource: 0,
            })
            .unwrap();
        let neutral = s
            .spawn(Spawn {
                archetype: ArchetypeId(2),
                owner: PlayerId::NEUTRAL,
                pos: FixedVec2::from_ints(5, 5),
                hp: 1500,
                resource: 0,
            })
            .unwrap();
        (s, mine, theirs, neutral)
    }

    fn cmd(subject: EntityHandle, player: u8, command: Command) -> IssuedCommand {
        IssuedCommand {
            tick: Tick(10),
            player: PlayerId(player),
            subject,
            command,
        }
    }

    // ---- validation ------------------------------------------------------

    #[test]
    fn a_valid_move_is_accepted() {
        let (s, mine, _, _) = state_with_two_players();
        let c = cmd(
            mine,
            0,
            Command::Move {
                target: FixedVec2::from_ints(3, 3),
            },
        );
        assert_eq!(validate(&s, &c), Ok(()));
    }

    #[test]
    fn commanding_someone_elses_unit_is_rejected() {
        // The anti-cheat case. A peer can send this; it must be dropped.
        let (s, _, theirs, _) = state_with_two_players();
        let c = cmd(theirs, 0, Command::Stop);
        assert_eq!(validate(&s, &c), Err(Reject::NotOwned));
    }

    #[test]
    fn commanding_a_neutral_entity_is_rejected() {
        let (s, _, _, neutral) = state_with_two_players();
        let c = cmd(neutral, 0, Command::Stop);
        assert_eq!(validate(&s, &c), Err(Reject::NeutralSubject));
    }

    #[test]
    fn a_stale_subject_handle_is_rejected() {
        let (mut s, mine, _, _) = state_with_two_players();
        s.despawn(mine);
        let c = cmd(mine, 0, Command::Stop);
        assert_eq!(validate(&s, &c), Err(Reject::DeadSubject));
    }

    #[test]
    fn a_fabricated_subject_handle_is_rejected() {
        let (s, _, _, _) = state_with_two_players();
        let fake = EntityHandle::new(999, 1);
        assert_eq!(
            validate(&s, &cmd(fake, 0, Command::Stop)),
            Err(Reject::DeadSubject)
        );
    }

    #[test]
    fn a_player_id_beyond_capacity_is_rejected() {
        let (s, mine, _, _) = state_with_two_players();
        assert_eq!(
            validate(&s, &cmd(mine, 99, Command::Stop)),
            Err(Reject::NoSuchPlayer)
        );
    }

    #[test]
    fn attacking_a_dead_target_is_rejected() {
        let (mut s, mine, theirs, _) = state_with_two_players();
        s.despawn(theirs);
        let c = cmd(mine, 0, Command::AttackTarget { target: theirs });
        assert_eq!(validate(&s, &c), Err(Reject::DeadTarget));
    }

    #[test]
    fn harvesting_a_dead_node_is_rejected() {
        let (mut s, mine, _, neutral) = state_with_two_players();
        s.despawn(neutral);
        let c = cmd(mine, 0, Command::Harvest { node: neutral });
        assert_eq!(validate(&s, &c), Err(Reject::DeadTarget));
    }

    #[test]
    fn harvesting_a_live_neutral_node_is_fine() {
        // A neutral entity cannot be commanded, but it is a legal TARGET.
        let (s, mine, _, neutral) = state_with_two_players();
        let c = cmd(mine, 0, Command::Harvest { node: neutral });
        assert_eq!(validate(&s, &c), Ok(()));
    }

    #[test]
    fn an_out_of_range_cancel_slot_is_rejected() {
        let (s, mine, _, _) = state_with_two_players();
        let c = cmd(mine, 0, Command::Cancel { queue_slot: 16 });
        assert_eq!(validate(&s, &c), Err(Reject::BadQueueSlot));
        let ok = cmd(mine, 0, Command::Cancel { queue_slot: 15 });
        assert_eq!(validate(&s, &ok), Ok(()));
    }

    #[test]
    fn an_ability_aimed_at_a_dead_entity_is_rejected() {
        let (mut s, mine, theirs, _) = state_with_two_players();
        s.despawn(theirs);
        let c = cmd(
            mine,
            0,
            Command::UseAbility {
                ability: AbilityId(3),
                target: AbilityTarget::Entity(theirs),
            },
        );
        assert_eq!(validate(&s, &c), Err(Reject::DeadTarget));
    }

    #[test]
    fn validation_never_panics_on_hostile_input() {
        // Every variant, with maximally hostile arguments, against an empty
        // state. A networked peer can send exactly this.
        let s = SimState::new(caps(), 1, 64);
        let bad = EntityHandle::new(u32::MAX, u32::MAX);
        let variants = [
            Command::Move {
                target: FixedVec2::ZERO,
            },
            Command::AttackMove {
                target: FixedVec2::ZERO,
            },
            Command::AttackTarget { target: bad },
            Command::Stop,
            Command::HoldPosition,
            Command::Patrol {
                waypoint: FixedVec2::ZERO,
            },
            Command::Harvest { node: bad },
            Command::ReturnCargo,
            Command::Build {
                building: ArchetypeId(u16::MAX),
                pos: FixedVec2::ZERO,
            },
            Command::Train {
                unit: ArchetypeId(u16::MAX),
            },
            Command::Research {
                tech: TechId(u16::MAX),
            },
            Command::Cancel {
                queue_slot: u8::MAX,
            },
            Command::UseAbility {
                ability: AbilityId(u16::MAX),
                target: AbilityTarget::Entity(bad),
            },
            Command::SetRally {
                pos: FixedVec2::ZERO,
            },
            Command::Surrender,
        ];
        for v in variants {
            for player in [0u8, 3, u8::MAX] {
                let c = cmd(bad, player, v);
                let _ = validate(&s, &c); // must not panic
            }
        }
    }

    // ---- log --------------------------------------------------------------

    #[test]
    fn the_log_preserves_append_order() {
        let mut log = CommandLog::with_reserve(8);
        for i in 0..5u64 {
            log.push(IssuedCommand {
                tick: Tick(i),
                player: PlayerId(0),
                subject: EntityHandle::new(i as u32, 1),
                command: Command::Stop,
            });
        }
        let ticks: Vec<u64> = log.iter().map(|c| c.tick.0).collect();
        assert_eq!(ticks, [0, 1, 2, 3, 4]);
        assert_eq!(log.len(), 5);
    }

    #[test]
    fn for_tick_selects_only_that_tick() {
        let mut log = CommandLog::with_reserve(8);
        for i in 0..6u64 {
            log.push(IssuedCommand {
                tick: Tick(i % 2),
                player: PlayerId(0),
                subject: EntityHandle::new(i as u32, 1),
                command: Command::Stop,
            });
        }
        assert_eq!(log.for_tick(Tick(0)).count(), 3);
        assert_eq!(log.for_tick(Tick(1)).count(), 3);
        assert_eq!(log.for_tick(Tick(9)).count(), 0);
    }

    #[test]
    fn rejections_are_counted_into_the_hash() {
        // Two clients disagreeing about how many commands were INVALID have
        // diverged just as surely as ones disagreeing about positions.
        let mut a = CommandLog::with_reserve(4);
        let b = CommandLog::with_reserve(4);
        a.record_rejection();
        let mut ha = crate::hash::SimHasher::new();
        let mut hb = crate::hash::SimHasher::new();
        a.hash_into(&mut ha);
        b.hash_into(&mut hb);
        assert_ne!(ha.finish(), hb.finish());
        assert_eq!(a.rejected_count(), 1);
    }

    #[test]
    fn command_tags_are_pinned() {
        // These bytes are in the state hash. Reordering the enum is a
        // determinism break and invalidates every stored replay.
        assert_eq!(
            Command::Move {
                target: FixedVec2::ZERO
            }
            .tag(),
            0
        );
        assert_eq!(Command::Stop.tag(), 3);
        assert_eq!(Command::Surrender.tag(), 14);
    }

    #[test]
    fn different_commands_hash_differently() {
        let mut seen = Vec::new();
        for c in [
            Command::Stop,
            Command::HoldPosition,
            Command::ReturnCargo,
            Command::Surrender,
            Command::Move {
                target: FixedVec2::ZERO,
            },
            Command::AttackMove {
                target: FixedVec2::ZERO,
            },
            Command::Train {
                unit: ArchetypeId(1),
            },
            Command::Train {
                unit: ArchetypeId(2),
            },
        ] {
            let mut h = crate::hash::SimHasher::new();
            c.hash_into(&mut h);
            let v = h.finish();
            assert!(!seen.contains(&v), "hash collision for {c:?}");
            seen.push(v);
        }
    }

    #[test]
    fn move_and_attack_move_to_the_same_point_hash_differently() {
        // They share a payload shape; only the tag separates them.
        let p = FixedVec2::from_ints(4, 4);
        let mut h1 = crate::hash::SimHasher::new();
        let mut h2 = crate::hash::SimHasher::new();
        Command::Move { target: p }.hash_into(&mut h1);
        Command::AttackMove { target: p }.hash_into(&mut h2);
        assert_ne!(h1.finish(), h2.finish());
    }
}
