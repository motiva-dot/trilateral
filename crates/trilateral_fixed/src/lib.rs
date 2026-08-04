//! trilateral_fixed — the deterministic arithmetic foundation.
//!
//! Governance: `/CLAUDE.md` is the supreme authority. See TECH_SPEC §2 for the
//! mechanism and `docs/00_AUDIT.md` item 3 for why Q16.16-on-i32 was rejected.
//!
//! Everything in this crate is integer arithmetic. There are no floats behind
//! any code path reachable from the simulation, and `scripts/ban_floats.sh`
//! plus the cross-architecture CI arena both enforce that. The single
//! exception is the `float_bridge` feature, which exists so the YAML loader
//! can parse authored decimals at match start and so the presentation crate
//! can interpolate for rendering — neither of which runs inside `sim_tick`.
//!
//! # Why this crate can be trusted
//! Determinism here is not a property of the tests passing; it is a property
//! of every operation being integer-exact with a stated rounding rule. The
//! tests pin those rules, and `tests/golden.rs` pins the composition of them
//! to a hash committed in the repo and compared across three architectures in
//! CI. If that hash moves, the arithmetic changed — whether or not anyone
//! meant it to.

#![forbid(unsafe_code)]

mod fixed;
mod vec2;

pub use fixed::{FRAC_BITS, Fixed, ONE_BITS};
pub use vec2::FixedVec2;

#[cfg(feature = "float_bridge")]
mod float_bridge;
