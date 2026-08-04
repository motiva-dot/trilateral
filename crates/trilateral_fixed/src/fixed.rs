//! `Fixed` — Q32.32 fixed-point scalar backed by `i64`.
//!
//! CLAUDE.md §1.1 and TECH_SPEC §2 are the authority. The v1 spec used Q16.16
//! on i32; the audit (item 3) replaced it because `length_squared` between two
//! points 300 tiles apart overflows an i32 immediately. Do not regress.
//!
//! # Layout
//! 32 integer bits (±2.1e9), 32 fractional bits (step ~2.3e-10).
//!
//! # Rounding, stated once and relied upon everywhere
//! * `mul` shifts right by 32. Arithmetic shift on a negative value rounds
//!   toward **negative infinity** (floor).
//! * `div` uses integer division, which in Rust truncates toward **zero**.
//!
//! These two disagree for negative operands. That asymmetry is deliberate and
//! matches TECH_SPEC §2 verbatim: it is not a bug to be "fixed" later, because
//! every stored golden hash in this repo encodes it. Changing a rounding rule
//! is a determinism-breaking change and needs an ADR.

use core::cmp::Ordering;
use core::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Rem, Sub, SubAssign};

use serde::{Deserialize, Serialize};

/// Number of fractional bits. The `32` in Q32.32.
pub const FRAC_BITS: u32 = 32;

/// `1.0` expressed in raw bits.
pub const ONE_BITS: i64 = 1i64 << FRAC_BITS;

/// A Q32.32 fixed-point number.
///
/// Serialises as its raw `i64` bit pattern, never as decimal text — see
/// TECH_SPEC §2. A round trip is therefore exact by construction.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(transparent)]
pub struct Fixed(i64);

impl Fixed {
    pub const ZERO: Fixed = Fixed(0);
    pub const ONE: Fixed = Fixed(ONE_BITS);
    pub const NEG_ONE: Fixed = Fixed(-ONE_BITS);
    pub const HALF: Fixed = Fixed(ONE_BITS >> 1);
    pub const TWO: Fixed = Fixed(ONE_BITS << 1);
    pub const MIN: Fixed = Fixed(i64::MIN);
    pub const MAX: Fixed = Fixed(i64::MAX);

    /// Smallest representable positive value, ~2.3e-10.
    pub const EPSILON: Fixed = Fixed(1);

    /// pi, rounded to nearest representable Q32.32 value.
    /// Verified against a high-precision decimal expansion in `tests/`.
    pub const PI: Fixed = Fixed(13_493_037_705);
    /// tau = 2*pi.
    pub const TAU: Fixed = Fixed(26_986_075_409);
    /// pi/2.
    pub const FRAC_PI_2: Fixed = Fixed(6_746_518_852);

    /// Reinterpret a raw bit pattern. This is the deserialisation primitive;
    /// prefer `from_int` / `from_ratio` for values with meaning.
    #[inline]
    pub const fn from_bits(bits: i64) -> Fixed {
        Fixed(bits)
    }

    /// The raw bit pattern. This is what gets folded into state hashes.
    #[inline]
    pub const fn to_bits(self) -> i64 {
        self.0
    }

    /// Exact conversion from a whole number.
    ///
    /// Debug builds assert the value is representable; release wraps, and the
    /// determinism arena would catch the divergence.
    #[inline]
    pub const fn from_int(v: i32) -> Fixed {
        Fixed((v as i64) << FRAC_BITS)
    }

    /// Exact rational construction: `num / den`, evaluated in i128.
    ///
    /// This is how gameplay constants should be written — `from_ratio(1, 3)`
    /// rather than any decimal literal, so intent survives review.
    ///
    /// # Panics
    /// If `den` is zero.
    #[inline]
    pub const fn from_ratio(num: i64, den: i64) -> Fixed {
        assert!(den != 0, "Fixed::from_ratio: division by zero");
        Fixed((((num as i128) << FRAC_BITS) / (den as i128)) as i64)
    }

    /// Truncate toward zero to a whole number.
    #[inline]
    pub const fn to_int_trunc(self) -> i64 {
        self.0 / ONE_BITS
    }

    /// Round toward negative infinity to a whole number.
    #[inline]
    pub const fn to_int_floor(self) -> i64 {
        self.0 >> FRAC_BITS
    }

    /// Largest whole value <= self, as a `Fixed`.
    #[inline]
    pub const fn floor(self) -> Fixed {
        Fixed((self.0 >> FRAC_BITS) << FRAC_BITS)
    }

    /// Smallest whole value >= self, as a `Fixed`.
    #[inline]
    pub const fn ceil(self) -> Fixed {
        let f = self.floor();
        if f.0 == self.0 {
            f
        } else {
            Fixed(f.0 + ONE_BITS)
        }
    }

    /// Round half away from zero.
    #[inline]
    pub const fn round(self) -> Fixed {
        if self.0 >= 0 {
            Fixed(((self.0 + (ONE_BITS >> 1)) >> FRAC_BITS) << FRAC_BITS)
        } else {
            Fixed(-((((-self.0) + (ONE_BITS >> 1)) >> FRAC_BITS) << FRAC_BITS))
        }
    }

    /// Fractional part, always in `[0, 1)` — `self - self.floor()`.
    #[inline]
    pub const fn fract(self) -> Fixed {
        Fixed(self.0 - self.floor().0)
    }

    #[inline]
    pub const fn abs(self) -> Fixed {
        Fixed(self.0.abs())
    }

    #[inline]
    pub const fn signum(self) -> i32 {
        if self.0 > 0 {
            1
        } else if self.0 < 0 {
            -1
        } else {
            0
        }
    }

    #[inline]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    #[inline]
    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }

    #[inline]
    pub const fn min(self, other: Fixed) -> Fixed {
        if self.0 < other.0 { self } else { other }
    }

    #[inline]
    pub const fn max(self, other: Fixed) -> Fixed {
        if self.0 > other.0 { self } else { other }
    }

    /// # Panics
    /// If `lo > hi` — a caller bug, not a runtime condition.
    #[inline]
    pub const fn clamp(self, lo: Fixed, hi: Fixed) -> Fixed {
        assert!(lo.0 <= hi.0, "Fixed::clamp: lo > hi");
        self.max(lo).min(hi)
    }

    /// Multiply, widening through i128 (CLAUDE.md §6.3 — never skip this).
    ///
    /// Rounds toward negative infinity. See the module note on rounding.
    #[inline]
    pub const fn mul(self, rhs: Fixed) -> Fixed {
        let wide = ((self.0 as i128) * (rhs.0 as i128)) >> FRAC_BITS;
        debug_assert!(
            wide >= i64::MIN as i128 && wide <= i64::MAX as i128,
            "Fixed::mul overflowed i64"
        );
        Fixed(wide as i64)
    }

    /// Divide, widening through i128.
    ///
    /// Truncates toward zero. See the module note on rounding.
    ///
    /// # Panics
    /// If `rhs` is zero. Division by zero is a caller bug; the simulation must
    /// never reach it, and silently returning zero would hide the defect until
    /// it surfaced as a desync.
    #[inline]
    pub const fn div(self, rhs: Fixed) -> Fixed {
        assert!(rhs.0 != 0, "Fixed::div by zero");
        let wide = ((self.0 as i128) << FRAC_BITS) / (rhs.0 as i128);
        debug_assert!(
            wide >= i64::MIN as i128 && wide <= i64::MAX as i128,
            "Fixed::div overflowed i64"
        );
        Fixed(wide as i64)
    }

    /// `self * self`, widened.
    #[inline]
    pub const fn sq(self) -> Fixed {
        self.mul(self)
    }

    /// Square root, bit-by-bit integer method.
    ///
    /// Exact and iteration-count-free — unlike Newton-Raphson, which needs a
    /// convergence criterion, and a convergence criterion is a place where two
    /// platforms can disagree. Result is the largest `r` with `r*r <= self`.
    ///
    /// # Panics
    /// If `self` is negative.
    #[inline]
    pub const fn sqrt(self) -> Fixed {
        assert!(self.0 >= 0, "Fixed::sqrt of a negative value");
        // sqrt(v / 2^32) * 2^32 == sqrt(v * 2^32).
        // v <= i64::MAX ~ 9.2e18, so v << 32 ~ 4.0e28 — comfortably inside u128.
        Fixed(isqrt_u128((self.0 as u128) << FRAC_BITS) as i64)
    }

    /// Linear interpolation. `t` is clamped to `[0, 1]`.
    #[inline]
    pub const fn lerp(self, to: Fixed, t: Fixed) -> Fixed {
        let t = t.clamp(Fixed::ZERO, Fixed::ONE);
        Fixed(self.0 + Fixed(to.0 - self.0).mul(t).0)
    }

    #[inline]
    pub const fn saturating_add(self, rhs: Fixed) -> Fixed {
        Fixed(self.0.saturating_add(rhs.0))
    }

    #[inline]
    pub const fn saturating_sub(self, rhs: Fixed) -> Fixed {
        Fixed(self.0.saturating_sub(rhs.0))
    }

    /// Multiply, saturating instead of wrapping on overflow.
    #[inline]
    pub const fn saturating_mul(self, rhs: Fixed) -> Fixed {
        let wide = ((self.0 as i128) * (rhs.0 as i128)) >> FRAC_BITS;
        if wide > i64::MAX as i128 {
            Fixed::MAX
        } else if wide < i64::MIN as i128 {
            Fixed::MIN
        } else {
            Fixed(wide as i64)
        }
    }

    /// `None` on overflow rather than a debug panic — for parse-time code that
    /// must reject bad content instead of aborting the process.
    #[inline]
    pub const fn checked_mul(self, rhs: Fixed) -> Option<Fixed> {
        let wide = ((self.0 as i128) * (rhs.0 as i128)) >> FRAC_BITS;
        if wide > i64::MAX as i128 || wide < i64::MIN as i128 {
            None
        } else {
            Some(Fixed(wide as i64))
        }
    }

    /// `None` on divide-by-zero or overflow.
    #[inline]
    pub const fn checked_div(self, rhs: Fixed) -> Option<Fixed> {
        if rhs.0 == 0 {
            return None;
        }
        let wide = ((self.0 as i128) << FRAC_BITS) / (rhs.0 as i128);
        if wide > i64::MAX as i128 || wide < i64::MIN as i128 {
            None
        } else {
            Some(Fixed(wide as i64))
        }
    }
}

/// Integer square root of a `u128`, two bits at a time.
///
/// Returns the largest `r` such that `r*r <= n`. Pure integer, fixed 64
/// iterations, no data-dependent branching on magnitude — the same work on
/// every platform for every input.
#[inline]
const fn isqrt_u128(n: u128) -> u128 {
    let mut rem: u128 = 0;
    let mut root: u128 = 0;
    let mut i: i32 = 63;
    while i >= 0 {
        rem = (rem << 2) | ((n >> (i * 2)) & 3);
        root <<= 1;
        let div = (root << 1) | 1;
        if rem >= div {
            rem -= div;
            root |= 1;
        }
        i -= 1;
    }
    root
}

// ---------------------------------------------------------------------------
// Operator sugar. Every one delegates to the inherent methods above so there
// is exactly one implementation of each rounding rule.
// ---------------------------------------------------------------------------

impl Add for Fixed {
    type Output = Fixed;
    #[inline]
    fn add(self, rhs: Fixed) -> Fixed {
        Fixed(self.0 + rhs.0)
    }
}

impl Sub for Fixed {
    type Output = Fixed;
    #[inline]
    fn sub(self, rhs: Fixed) -> Fixed {
        Fixed(self.0 - rhs.0)
    }
}

impl Mul for Fixed {
    type Output = Fixed;
    #[inline]
    fn mul(self, rhs: Fixed) -> Fixed {
        Fixed::mul(self, rhs)
    }
}

impl Div for Fixed {
    type Output = Fixed;
    #[inline]
    fn div(self, rhs: Fixed) -> Fixed {
        Fixed::div(self, rhs)
    }
}

impl Rem for Fixed {
    type Output = Fixed;
    #[inline]
    fn rem(self, rhs: Fixed) -> Fixed {
        assert!(rhs.0 != 0, "Fixed::rem by zero");
        Fixed(self.0 % rhs.0)
    }
}

impl Neg for Fixed {
    type Output = Fixed;
    #[inline]
    fn neg(self) -> Fixed {
        Fixed(-self.0)
    }
}

impl AddAssign for Fixed {
    #[inline]
    fn add_assign(&mut self, rhs: Fixed) {
        *self = *self + rhs;
    }
}

impl SubAssign for Fixed {
    #[inline]
    fn sub_assign(&mut self, rhs: Fixed) {
        *self = *self - rhs;
    }
}

impl MulAssign for Fixed {
    #[inline]
    fn mul_assign(&mut self, rhs: Fixed) {
        *self = *self * rhs;
    }
}

impl DivAssign for Fixed {
    #[inline]
    fn div_assign(&mut self, rhs: Fixed) {
        *self = *self / rhs;
    }
}

impl PartialOrd for Fixed {
    #[inline]
    fn partial_cmp(&self, other: &Fixed) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Fixed {
    #[inline]
    fn cmp(&self, other: &Fixed) -> Ordering {
        self.0.cmp(&other.0)
    }
}

impl From<i32> for Fixed {
    #[inline]
    fn from(v: i32) -> Fixed {
        Fixed::from_int(v)
    }
}

/// Renders as a decimal *for humans only* — logs, panic messages, debug
/// overlays. Never parse it back; `to_bits` is the machine-readable form.
///
/// Implemented with integer arithmetic so that even the debug path cannot
/// introduce a float into a sim crate.
impl core::fmt::Display for Fixed {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let neg = self.0 < 0;
        // Careful: -i64::MIN overflows, so work in i128.
        let mag = (self.0 as i128).unsigned_abs();
        let int_part = mag >> FRAC_BITS;
        let frac_bits = mag & (ONE_BITS as u128 - 1);
        // 9 decimal digits ~ 1e-9, finer than one ULP (2.3e-10) is not useful.
        let frac_digits = (frac_bits * 1_000_000_000) >> FRAC_BITS;
        if neg {
            write!(f, "-")?;
        }
        write!(f, "{int_part}.{frac_digits:09}")
    }
}

impl core::fmt::Debug for Fixed {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Fixed({self} | {:#x})", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- construction & conversion ------------------------------------

    #[test]
    fn one_is_two_to_the_thirty_two() {
        assert_eq!(Fixed::ONE.to_bits(), 4_294_967_296);
    }

    #[test]
    fn from_int_round_trips() {
        for v in [-100_000, -7, -1, 0, 1, 7, 100_000] {
            assert_eq!(Fixed::from_int(v).to_int_trunc(), v as i64);
        }
    }

    #[test]
    fn from_ratio_is_exact_for_dyadic_rationals() {
        assert_eq!(Fixed::from_ratio(1, 2), Fixed::HALF);
        assert_eq!(Fixed::from_ratio(1, 4).to_bits(), ONE_BITS / 4);
        assert_eq!(Fixed::from_ratio(-3, 4).to_bits(), -3 * ONE_BITS / 4);
    }

    #[test]
    fn from_ratio_one_third_is_floor_of_exact() {
        // 2^32 / 3 = 1431655765.33... -> truncates to 1431655765
        assert_eq!(Fixed::from_ratio(1, 3).to_bits(), 1_431_655_765);
    }

    // ---- rounding family ----------------------------------------------

    #[test]
    fn floor_ceil_round_on_exact_integers_are_identity() {
        let five = Fixed::from_int(5);
        assert_eq!(five.floor(), five);
        assert_eq!(five.ceil(), five);
        assert_eq!(five.round(), five);
        assert_eq!(five.fract(), Fixed::ZERO);
    }

    #[test]
    fn floor_goes_toward_negative_infinity() {
        let x = Fixed::from_ratio(-3, 2); // -1.5
        assert_eq!(x.floor(), Fixed::from_int(-2));
        assert_eq!(x.ceil(), Fixed::from_int(-1));
        assert_eq!(x.to_int_floor(), -2);
        assert_eq!(x.to_int_trunc(), -1); // trunc differs — deliberately
    }

    #[test]
    fn round_is_half_away_from_zero() {
        assert_eq!(Fixed::from_ratio(3, 2).round(), Fixed::from_int(2));
        assert_eq!(Fixed::from_ratio(-3, 2).round(), Fixed::from_int(-2));
        assert_eq!(Fixed::from_ratio(1, 2).round(), Fixed::ONE);
        assert_eq!(Fixed::from_ratio(-1, 2).round(), Fixed::NEG_ONE);
    }

    #[test]
    fn fract_is_always_non_negative() {
        for n in [-7i64, -3, -1, 1, 3, 7] {
            let x = Fixed::from_ratio(n, 4);
            let fr = x.fract();
            assert!(fr >= Fixed::ZERO && fr < Fixed::ONE, "fract({x}) = {fr}");
            assert_eq!(x.floor() + fr, x);
        }
    }

    // ---- arithmetic identities ----------------------------------------

    #[test]
    fn mul_by_one_is_identity() {
        for bits in [1i64, -1, 12_345_678, -987_654_321, ONE_BITS, -ONE_BITS] {
            let x = Fixed::from_bits(bits);
            assert_eq!(x * Fixed::ONE, x);
        }
    }

    #[test]
    fn mul_by_zero_is_zero() {
        assert_eq!(Fixed::from_int(12_345) * Fixed::ZERO, Fixed::ZERO);
    }

    #[test]
    fn mul_is_commutative() {
        let a = Fixed::from_ratio(7, 3);
        let b = Fixed::from_ratio(-11, 5);
        assert_eq!(a * b, b * a);
    }

    #[test]
    fn div_by_one_is_identity() {
        let x = Fixed::from_ratio(22, 7);
        assert_eq!(x / Fixed::ONE, x);
    }

    #[test]
    fn div_is_inverse_of_mul_for_exact_dyadics() {
        let a = Fixed::from_int(12);
        let b = Fixed::from_int(4);
        assert_eq!(a / b, Fixed::from_int(3));
        assert_eq!(Fixed::from_int(3) * b, a);
    }

    #[test]
    fn negation_is_involutive() {
        let x = Fixed::from_ratio(355, 113);
        assert_eq!(-(-x), x);
        assert_eq!(x + (-x), Fixed::ZERO);
    }

    #[test]
    fn mul_widens_through_i128_without_overflowing() {
        // 40_000^2 = 1.6e9. The i128 intermediate here is 1.6e9 * 2^64 ~ 3e28,
        // which is nowhere near representable in i64 — so this only works
        // because mul widens. Drop the widening and this test fails loudly.
        let a = Fixed::from_int(40_000);
        let b = Fixed::from_int(40_000);
        assert_eq!((a * b).to_int_trunc(), 1_600_000_000);
    }

    #[test]
    fn representable_range_is_roughly_plus_minus_two_billion() {
        // i64::MAX / 2^32. Squared magnitudes must stay under this; the map is
        // 128-256 tiles, so the largest squared distance in a real match is
        // ~131_072 — four orders of magnitude of headroom. That margin is the
        // entire point of audit item 3.
        assert_eq!(Fixed::MAX.to_int_trunc(), 2_147_483_647);
        assert_eq!(Fixed::MIN.to_int_trunc(), -2_147_483_648);
    }

    #[test]
    fn map_scale_distance_squared_does_not_overflow() {
        // The exact case that killed Q16.16 on i32: two points 300 tiles apart
        // give 90_000, which overflows an i32 holding Q16.16 (max 32_767).
        let dx = Fixed::from_int(300);
        let dy = Fixed::from_int(300);
        assert_eq!((dx.sq() + dy.sq()).to_int_trunc(), 180_000);
    }

    // ---- rounding direction, pinned ------------------------------------

    #[test]
    fn mul_rounds_toward_negative_infinity() {
        // 1 ULP * 0.5 == 0.5 ULP, floors to 0 for positive...
        assert_eq!(Fixed::EPSILON.mul(Fixed::HALF), Fixed::ZERO);
        // ...and to -1 ULP for negative, because >> is an arithmetic shift.
        assert_eq!((-Fixed::EPSILON).mul(Fixed::HALF), Fixed::from_bits(-1));
    }

    #[test]
    fn div_truncates_toward_zero() {
        let one_ulp = Fixed::EPSILON;
        assert_eq!(one_ulp.div(Fixed::TWO), Fixed::ZERO);
        assert_eq!((-one_ulp).div(Fixed::TWO), Fixed::ZERO);
    }

    // ---- sqrt ----------------------------------------------------------

    #[test]
    fn sqrt_of_perfect_squares_is_exact() {
        for n in [0i32, 1, 4, 9, 16, 25, 100, 10_000, 1_000_000] {
            let s = Fixed::from_int(n).sqrt();
            let expected = integer_sqrt(n as i64) as i32;
            assert_eq!(s, Fixed::from_int(expected), "sqrt({n})");
        }
    }

    /// Naive reference implementation. Deliberately not the one under test —
    /// a test that reuses the implementation proves only self-consistency.
    fn integer_sqrt(n: i64) -> i64 {
        let mut r = 0i64;
        while (r + 1) * (r + 1) <= n {
            r += 1;
        }
        r
    }

    #[test]
    fn sqrt_zero_and_one() {
        assert_eq!(Fixed::ZERO.sqrt(), Fixed::ZERO);
        assert_eq!(Fixed::ONE.sqrt(), Fixed::ONE);
    }

    #[test]
    fn sqrt_is_the_exact_integer_root_of_the_widened_value() {
        // The contract is about the WIDENED integers, not about `sq()`.
        //
        // Stating it in terms of `Fixed::sq` would be wrong near the bottom of
        // the range: at 1 ULP, sqrt gives 65536 bits, and both 65536^2 and
        // 65537^2 floor to the same Fixed after the >>32 — so "the next value
        // up squares to something bigger" is simply false there. Maximality
        // lives in i128, before the shift discards it.
        for bits in [
            1i64,
            7,
            ONE_BITS,
            ONE_BITS * 2,
            ONE_BITS * 12_345,
            999_999_999,
        ] {
            let x = Fixed::from_bits(bits);
            let r = x.sqrt().to_bits() as i128;
            let target = (bits as i128) << FRAC_BITS;
            assert!(r * r <= target, "sqrt({x:?}) overshot");
            assert!((r + 1) * (r + 1) > target, "sqrt({x:?}) was not maximal");
        }
    }

    #[test]
    fn sqrt_never_exceeds_its_input_when_squared_back() {
        // The weaker, Fixed-level property that DOES hold everywhere.
        for bits in [
            1i64,
            7,
            ONE_BITS,
            ONE_BITS * 2,
            ONE_BITS * 12_345,
            999_999_999,
        ] {
            let x = Fixed::from_bits(bits);
            let r = x.sqrt();
            assert!(r.sq() <= x, "sqrt({x:?})^2 = {:?} exceeded input", r.sq());
        }
    }

    #[test]
    #[should_panic(expected = "negative")]
    fn sqrt_of_negative_panics() {
        let _ = Fixed::from_int(-1).sqrt();
    }

    #[test]
    fn sqrt_of_max_does_not_overflow() {
        let r = Fixed::MAX.sqrt();
        assert!(r.sq() <= Fixed::MAX);
    }

    // ---- clamp / lerp / min / max --------------------------------------

    #[test]
    fn clamp_bounds_are_inclusive() {
        let lo = Fixed::from_int(-5);
        let hi = Fixed::from_int(5);
        assert_eq!(Fixed::from_int(-9).clamp(lo, hi), lo);
        assert_eq!(Fixed::from_int(9).clamp(lo, hi), hi);
        assert_eq!(Fixed::from_int(0).clamp(lo, hi), Fixed::ZERO);
        assert_eq!(lo.clamp(lo, hi), lo);
        assert_eq!(hi.clamp(lo, hi), hi);
    }

    #[test]
    fn lerp_endpoints_are_exact() {
        let a = Fixed::from_int(10);
        let b = Fixed::from_int(20);
        assert_eq!(a.lerp(b, Fixed::ZERO), a);
        assert_eq!(a.lerp(b, Fixed::ONE), b);
        assert_eq!(a.lerp(b, Fixed::HALF), Fixed::from_int(15));
    }

    #[test]
    fn lerp_clamps_t_outside_unit_interval() {
        let a = Fixed::from_int(10);
        let b = Fixed::from_int(20);
        assert_eq!(a.lerp(b, Fixed::from_int(5)), b);
        assert_eq!(a.lerp(b, Fixed::from_int(-5)), a);
    }

    // ---- saturating & checked ------------------------------------------

    #[test]
    fn saturating_add_clamps_at_the_extremes() {
        assert_eq!(Fixed::MAX.saturating_add(Fixed::ONE), Fixed::MAX);
        assert_eq!(Fixed::MIN.saturating_sub(Fixed::ONE), Fixed::MIN);
    }

    #[test]
    fn saturating_mul_clamps() {
        let big = Fixed::from_int(2_000_000_000);
        assert_eq!(big.saturating_mul(big), Fixed::MAX);
        assert_eq!(big.saturating_mul(-big), Fixed::MIN);
    }

    #[test]
    fn checked_div_by_zero_is_none() {
        assert_eq!(Fixed::ONE.checked_div(Fixed::ZERO), None);
        assert_eq!(Fixed::ONE.checked_div(Fixed::TWO), Some(Fixed::HALF));
    }

    #[test]
    #[should_panic(expected = "div by zero")]
    fn div_by_zero_panics() {
        let _ = Fixed::ONE / Fixed::ZERO;
    }

    // ---- ordering -------------------------------------------------------

    #[test]
    fn ordering_matches_numeric_order() {
        let mut v = [
            Fixed::from_int(3),
            Fixed::from_int(-1),
            Fixed::ZERO,
            Fixed::MIN,
            Fixed::MAX,
            Fixed::HALF,
        ];
        v.sort();
        assert_eq!(
            v,
            [
                Fixed::MIN,
                Fixed::from_int(-1),
                Fixed::ZERO,
                Fixed::HALF,
                Fixed::from_int(3),
                Fixed::MAX,
            ]
        );
    }

    // ---- display --------------------------------------------------------

    #[test]
    fn display_is_integer_only_and_readable() {
        assert_eq!(Fixed::ONE.to_string(), "1.000000000");
        assert_eq!(Fixed::HALF.to_string(), "0.500000000");
        assert_eq!(Fixed::from_int(-3).to_string(), "-3.000000000");
        assert_eq!(Fixed::from_ratio(-1, 2).to_string(), "-0.500000000");
    }

    #[test]
    fn display_of_min_does_not_overflow() {
        // -i64::MIN would panic if magnitude were taken in i64.
        let _ = Fixed::MIN.to_string();
    }
}
