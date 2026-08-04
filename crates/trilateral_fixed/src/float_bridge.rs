//! The float bridge — the only place in this crate where a float may appear,
//! and it is feature-gated off by default.
//!
//! FLOAT_EXCEPTION_FILE: the sanctioned, feature-gated float bridge (CLAUDE.md §1.1)
//!
//! CLAUDE.md §1.1: enabled only by the YAML loader (parse time) and by the
//! presentation crate (render interpolation). Nothing reachable from
//! `sim_tick` may call any of this. The crate graph in TECH_SPEC §1 and the
//! `cargo deny` allowlist are what keep that true — this file being exempt
//! from the grep linter is precisely why those two matter.
//!
//! # Is this deterministic?
//! Yes, and the reason is worth stating because it is not obvious. Content
//! hashes must agree across clients, so parse-time conversion has to give the
//! same bits on every platform. It does: Rust's decimal-to-f64 parser is
//! correctly rounded, multiplication by 2^32 is exact in IEEE-754 (it only
//! adjusts the exponent, never the mantissa), and `round()` is a specified
//! operation. No libm call, no fused-multiply-add, no compiler latitude.
//!
//! That argument holds for *conversion*. It would NOT hold for arithmetic —
//! which is why this module converts and does nothing else. Do not add a
//! helper here that computes anything.
//!
//! # Better still, later
//! A pure-integer `from_decimal_str` would remove even this reasoning by
//! parsing "1.25" straight to bits without touching a float. Worth an ADR at
//! Phase 2 when the YAML loader lands, at which point this bridge may shrink
//! to presentation use only.

use crate::{Fixed, FixedVec2, ONE_BITS};

impl Fixed {
    /// Convert from a float. Parse time and presentation only.
    ///
    /// Rounds to nearest, ties away from zero.
    pub fn from_f64(v: f64) -> Fixed {
        Fixed::from_bits((v * (ONE_BITS as f64)).round() as i64)
    }

    /// Convert to a float. Presentation only — never feed the result back
    /// into simulation state.
    pub fn to_f64(self) -> f64 {
        (self.to_bits() as f64) / (ONE_BITS as f64)
    }
}

impl FixedVec2 {
    pub fn from_f64s(x: f64, y: f64) -> FixedVec2 {
        FixedVec2::new(Fixed::from_f64(x), Fixed::from_f64(y))
    }

    pub fn to_f64s(self) -> (f64, f64) {
        (self.x.to_f64(), self.y.to_f64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_numbers_round_trip_exactly() {
        for v in [-1000i32, -1, 0, 1, 1000] {
            assert_eq!(Fixed::from_f64(v as f64), Fixed::from_int(v));
        }
    }

    #[test]
    fn dyadic_fractions_are_exact() {
        assert_eq!(Fixed::from_f64(0.5), Fixed::HALF);
        assert_eq!(Fixed::from_f64(0.25), Fixed::from_ratio(1, 4));
        assert_eq!(Fixed::from_f64(-0.5), -Fixed::HALF);
    }

    #[test]
    fn conversion_is_stable_under_a_round_trip() {
        for bits in [1i64, 12_345_678, -987_654_321, ONE_BITS, -ONE_BITS * 7] {
            let a = Fixed::from_bits(bits);
            assert_eq!(a, Fixed::from_f64(a.to_f64()), "round trip moved {a:?}");
        }
    }

    #[test]
    fn vec_conversion_round_trips() {
        let v = FixedVec2::from_ints(3, -4);
        let (x, y) = v.to_f64s();
        assert_eq!(FixedVec2::from_f64s(x, y), v);
    }
}
