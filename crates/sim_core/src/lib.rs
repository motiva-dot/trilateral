//! sim_core — the simulation state store.
//!
//! Governance: `/CLAUDE.md` is the supreme authority; §3 defines this crate's
//! shape and §4 its vocabulary. TECH_SPEC §3 gives the mechanism.
//!
//! # The four laws this crate has to obey
//! * **No floats** (§1.1) — all math via `trilateral_fixed`.
//! * **No entropy** (§1.2) — no `HashMap`, no `thread_rng`, no `SystemTime`.
//!   Randomness only through [`SimRng`], seeded explicitly.
//! * **Only three dependencies** (§1.3) — `trilateral_fixed`, `serde`,
//!   `smallvec`. Enforced by `cargo deny`, not by good intentions.
//! * **No steady-state allocation** (§1.4) — everything is sized once at match
//!   start from `engine.yaml` and never grows.
//!
//! # Why a hand-rolled SoA store instead of an ECS
//! Audit items 5 and 6. Snapshotting and hashing the *whole* state are the two
//! operations this engine performs constantly, and an archetype ECS makes both
//! awkward because memory layout depends on component insertion history. Here
//! layout is declared once and never varies, so `snapshot()` is a copy and
//! `hash()` is a fold over contiguous buffers in a fixed order.

#![forbid(unsafe_code)]

pub mod bitset;
pub mod capacities;
pub mod clock;
pub mod command;
pub mod components;
pub mod entity;
pub mod grid;
pub mod hash;
pub mod ids;
pub mod registry;
pub mod rng;
pub mod state;

pub use bitset::BitSet;
pub use capacities::{Capacities, CapacityError};
pub use clock::{SimClock, Tick};
pub use command::{AbilityId, AbilityTarget, Command, CommandLog, IssuedCommand, Reject};
pub use components::{Components, UnitState};
pub use entity::EntityAllocator;
pub use grid::{MacroGrid, Tile};
pub use hash::SimHasher;
pub use ids::{ArchetypeId, EntityHandle, EntityIndex, OptionalHandle, PlayerId, TechId};
pub use registry::{Registries, SteeringParams, UnitRegistry, UnitStats};
pub use rng::SimRng;
pub use state::{SimState, Spawn};
