//! sim_content — YAML on disk becomes registries in memory.
//!
//! TECH_SPEC §1 puts this crate outside the simulation proper: it runs once at
//! match start (and on hot reload in dev), never inside `sim_tick`. That is
//! why it is allowed a YAML parser and the `float_bridge` feature when
//! `sim_core` is not.
//!
//! # Authored content is untrusted input
//! MARKET_POSITION differentiator #4 makes balance community-forkable, so
//! these loaders will eventually be fed files written by strangers. Every
//! failure must be a returned error with a message naming the offending id —
//! never a panic, and never a silent default that turns a typo into a subtly
//! different game.
//!
//! # A Cargo feature-unification hazard worth knowing about
//! This crate enables `trilateral_fixed/float_bridge`. Cargo features are
//! **additive and unified across the whole graph**, so the moment any crate in
//! the build turns that feature on, the same `trilateral_fixed` rlib linked
//! into `sim_core` also has `from_f64`/`to_f64` compiled in. The quarantine in
//! TECH_SPEC §1 is therefore a *convention*, not something the type system
//! enforces.
//!
//! What actually enforces it is `scripts/ban_floats.sh`: any call to
//! `Fixed::from_f64` inside `sim_core`/`sim_systems` contains the token `f64`
//! and is rejected. Worth remembering the next time someone assumes a
//! feature flag is a wall.

#![forbid(unsafe_code)]

pub mod engine;
pub mod tech;

pub use engine::capacities_from_yaml;
pub use tech::{ContentError, Effect, EffectValue, Tech, TechKind, TechRegistry};
