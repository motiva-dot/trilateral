//! `FixedVec2` — a 2D vector of `Fixed`, per TECH_SPEC §2.
//!
//! Every operation that can overflow a single `Fixed` offers an i128 variant,
//! because the classic RTS overflow is a squared distance, not a coordinate.

use core::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

use serde::{Deserialize, Serialize};

use crate::fixed::{FRAC_BITS, Fixed};

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FixedVec2 {
    pub x: Fixed,
    pub y: Fixed,
}

impl FixedVec2 {
    pub const ZERO: FixedVec2 = FixedVec2::new(Fixed::ZERO, Fixed::ZERO);
    pub const X: FixedVec2 = FixedVec2::new(Fixed::ONE, Fixed::ZERO);
    pub const Y: FixedVec2 = FixedVec2::new(Fixed::ZERO, Fixed::ONE);

    #[inline]
    pub const fn new(x: Fixed, y: Fixed) -> FixedVec2 {
        FixedVec2 { x, y }
    }

    /// Convenience for whole-number literals in tests and map data.
    #[inline]
    pub const fn from_ints(x: i32, y: i32) -> FixedVec2 {
        FixedVec2::new(Fixed::from_int(x), Fixed::from_int(y))
    }

    #[inline]
    pub const fn dot(self, rhs: FixedVec2) -> Fixed {
        Fixed::from_bits(self.x.mul(rhs.x).to_bits() + self.y.mul(rhs.y).to_bits())
    }

    /// The z component of the 3D cross product — the 2D "which side" test.
    ///
    /// Positive means `rhs` is counter-clockwise from `self`. Used for facing
    /// arcs and for velocity-obstacle side selection in steering.
    #[inline]
    pub const fn cross_z(self, rhs: FixedVec2) -> Fixed {
        Fixed::from_bits(self.x.mul(rhs.y).to_bits() - self.y.mul(rhs.x).to_bits())
    }

    /// Squared length as a `Fixed`.
    ///
    /// Overflows for magnitudes beyond ~46_340 (since 46_341^2 > 2^31). That is
    /// far outside any legal map coordinate; use [`length_sq_wide`] if the
    /// inputs are unbounded.
    ///
    /// [`length_sq_wide`]: FixedVec2::length_sq_wide
    #[inline]
    pub const fn length_sq(self) -> Fixed {
        self.dot(self)
    }

    /// Squared length kept in i128, in raw Q32.32 bits.
    ///
    /// Cannot overflow for any representable input. Compare two of these
    /// directly rather than converting back — that is the whole point.
    #[inline]
    pub const fn length_sq_wide(self) -> i128 {
        let x = self.x.to_bits() as i128;
        let y = self.y.to_bits() as i128;
        ((x * x) >> FRAC_BITS) + ((y * y) >> FRAC_BITS)
    }

    /// Euclidean length, exact to the last representable bit.
    #[inline]
    pub const fn length(self) -> Fixed {
        // Route through the wide form so long vectors do not overflow before
        // the square root brings the magnitude back down.
        let sq = self.length_sq_wide();
        debug_assert!(sq >= 0, "length_sq_wide went negative");
        Fixed::from_bits(isqrt_i128_q32(sq))
    }

    /// Manhattan (L1) distance from the origin. Cheap, and enough for
    /// broad-phase rejection before a real length is needed.
    #[inline]
    pub const fn manhattan(self) -> Fixed {
        Fixed::from_bits(self.x.abs().to_bits() + self.y.abs().to_bits())
    }

    #[inline]
    pub const fn distance_sq_wide(self, rhs: FixedVec2) -> i128 {
        self.sub(rhs).length_sq_wide()
    }

    #[inline]
    pub const fn distance(self, rhs: FixedVec2) -> Fixed {
        self.sub(rhs).length()
    }

    /// Unit vector in the same direction.
    ///
    /// Returns [`FixedVec2::ZERO`] for a zero input — never a panic and never a
    /// NaN-equivalent. Callers that care about the difference should test
    /// `is_zero()` first; silently propagating zero is the behaviour steering
    /// and pathfinding both want.
    #[inline]
    pub const fn normalize(self) -> FixedVec2 {
        let len = self.length();
        if len.is_zero() {
            return FixedVec2::ZERO;
        }
        FixedVec2::new(self.x.div(len), self.y.div(len))
    }

    /// Shorten to `max_len` if longer; leave alone if not.
    #[inline]
    pub const fn clamp_length(self, max_len: Fixed) -> FixedVec2 {
        debug_assert!(!max_len.is_negative(), "clamp_length: negative max");
        let len = self.length();
        if len.to_bits() <= max_len.to_bits() || len.is_zero() {
            self
        } else {
            let n = FixedVec2::new(self.x.div(len), self.y.div(len));
            n.scale(max_len)
        }
    }

    /// Multiply both components by a scalar.
    #[inline]
    pub const fn scale(self, s: Fixed) -> FixedVec2 {
        FixedVec2::new(self.x.mul(s), self.y.mul(s))
    }

    /// Rotate 90 degrees counter-clockwise. Exact — no trig involved.
    #[inline]
    pub const fn perp(self) -> FixedVec2 {
        FixedVec2::new(Fixed::from_bits(-self.y.to_bits()), self.x)
    }

    #[inline]
    pub const fn is_zero(self) -> bool {
        self.x.is_zero() && self.y.is_zero()
    }

    #[inline]
    pub const fn add(self, rhs: FixedVec2) -> FixedVec2 {
        FixedVec2::new(
            Fixed::from_bits(self.x.to_bits() + rhs.x.to_bits()),
            Fixed::from_bits(self.y.to_bits() + rhs.y.to_bits()),
        )
    }

    #[inline]
    pub const fn sub(self, rhs: FixedVec2) -> FixedVec2 {
        FixedVec2::new(
            Fixed::from_bits(self.x.to_bits() - rhs.x.to_bits()),
            Fixed::from_bits(self.y.to_bits() - rhs.y.to_bits()),
        )
    }

    #[inline]
    pub const fn neg(self) -> FixedVec2 {
        FixedVec2::new(
            Fixed::from_bits(-self.x.to_bits()),
            Fixed::from_bits(-self.y.to_bits()),
        )
    }

    #[inline]
    pub const fn lerp(self, to: FixedVec2, t: Fixed) -> FixedVec2 {
        FixedVec2::new(self.x.lerp(to.x, t), self.y.lerp(to.y, t))
    }
}

/// `sqrt` of a Q32.32 value already held in i128, returning Q32.32 bits.
///
/// Same bit-by-bit method as `Fixed::sqrt`, but the input has already been
/// widened so no magnitude is lost on the way in.
#[inline]
const fn isqrt_i128_q32(q32: i128) -> i64 {
    let n = (q32 as u128) << FRAC_BITS;
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
    root as i64
}

impl Add for FixedVec2 {
    type Output = FixedVec2;
    #[inline]
    fn add(self, rhs: FixedVec2) -> FixedVec2 {
        FixedVec2::add(self, rhs)
    }
}

impl Sub for FixedVec2 {
    type Output = FixedVec2;
    #[inline]
    fn sub(self, rhs: FixedVec2) -> FixedVec2 {
        FixedVec2::sub(self, rhs)
    }
}

impl Neg for FixedVec2 {
    type Output = FixedVec2;
    #[inline]
    fn neg(self) -> FixedVec2 {
        FixedVec2::neg(self)
    }
}

impl Mul<Fixed> for FixedVec2 {
    type Output = FixedVec2;
    #[inline]
    fn mul(self, s: Fixed) -> FixedVec2 {
        self.scale(s)
    }
}

impl Div<Fixed> for FixedVec2 {
    type Output = FixedVec2;
    #[inline]
    fn div(self, s: Fixed) -> FixedVec2 {
        FixedVec2::new(self.x / s, self.y / s)
    }
}

impl AddAssign for FixedVec2 {
    #[inline]
    fn add_assign(&mut self, rhs: FixedVec2) {
        *self = *self + rhs;
    }
}

impl SubAssign for FixedVec2 {
    #[inline]
    fn sub_assign(&mut self, rhs: FixedVec2) {
        *self = *self - rhs;
    }
}

impl core::fmt::Debug for FixedVec2 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_sub_are_componentwise_and_exact() {
        let a = FixedVec2::from_ints(3, -4);
        let b = FixedVec2::from_ints(-1, 7);
        assert_eq!(a + b, FixedVec2::from_ints(2, 3));
        assert_eq!(a - b, FixedVec2::from_ints(4, -11));
        assert_eq!(a + b - b, a);
    }

    #[test]
    fn dot_of_perpendicular_axes_is_zero() {
        assert_eq!(FixedVec2::X.dot(FixedVec2::Y), Fixed::ZERO);
        assert_eq!(FixedVec2::X.dot(FixedVec2::X), Fixed::ONE);
    }

    #[test]
    fn dot_matches_hand_computation() {
        let a = FixedVec2::from_ints(3, 4);
        let b = FixedVec2::from_ints(5, 6);
        // 3*5 + 4*6 = 39
        assert_eq!(a.dot(b), Fixed::from_int(39));
    }

    #[test]
    fn cross_z_sign_indicates_side() {
        // Y is counter-clockwise from X.
        assert_eq!(FixedVec2::X.cross_z(FixedVec2::Y), Fixed::ONE);
        assert_eq!(FixedVec2::Y.cross_z(FixedVec2::X), Fixed::NEG_ONE);
        assert_eq!(FixedVec2::X.cross_z(FixedVec2::X), Fixed::ZERO);
    }

    #[test]
    fn length_of_a_three_four_five_triangle_is_exact() {
        let v = FixedVec2::from_ints(3, 4);
        assert_eq!(v.length_sq(), Fixed::from_int(25));
        assert_eq!(v.length(), Fixed::from_int(5));
    }

    #[test]
    fn length_of_axis_vectors_is_one() {
        assert_eq!(FixedVec2::X.length(), Fixed::ONE);
        assert_eq!(FixedVec2::Y.length(), Fixed::ONE);
        assert_eq!(FixedVec2::ZERO.length(), Fixed::ZERO);
    }

    #[test]
    fn length_sq_wide_survives_magnitudes_that_overflow_the_narrow_form() {
        // 100_000^2 * 2 = 2e10, well past Fixed's ~2.1e9 integer ceiling.
        let v = FixedVec2::from_ints(100_000, 100_000);
        let wide = v.length_sq_wide();
        // In raw Q32.32 bits: 2e10 * 2^32.
        assert_eq!(wide, 20_000_000_000i128 * (1i128 << 32));
        // And the length still comes back exactly: sqrt(2e10) = 141421.356...
        assert_eq!(v.length().to_int_trunc(), 141_421);
    }

    #[test]
    fn manhattan_is_the_sum_of_absolute_components() {
        assert_eq!(FixedVec2::from_ints(-3, 4).manhattan(), Fixed::from_int(7));
    }

    #[test]
    fn distance_is_symmetric() {
        let a = FixedVec2::from_ints(10, 10);
        let b = FixedVec2::from_ints(13, 14);
        assert_eq!(a.distance(b), Fixed::from_int(5));
        assert_eq!(b.distance(a), a.distance(b));
    }

    #[test]
    fn normalize_of_zero_is_zero_not_a_panic() {
        assert_eq!(FixedVec2::ZERO.normalize(), FixedVec2::ZERO);
    }

    #[test]
    fn normalize_of_an_axis_is_exact() {
        let v = FixedVec2::from_ints(0, 17);
        assert_eq!(v.normalize(), FixedVec2::Y);
    }

    #[test]
    fn normalize_produces_unit_length_within_one_ulp() {
        for (x, y) in [(3, 4), (5, 12), (8, 15), (7, 24), (1, 1), (-9, 40)] {
            let n = FixedVec2::from_ints(x, y).normalize();
            let len = n.length();
            let err = (len - Fixed::ONE).abs();
            // Two rounding steps (divide, then sqrt) — bound at a few ULP.
            assert!(
                err <= Fixed::from_bits(8),
                "({x},{y}) normalized to length {len}, error {err}"
            );
        }
    }

    #[test]
    fn perp_rotates_ninety_degrees_counter_clockwise() {
        assert_eq!(FixedVec2::X.perp(), FixedVec2::Y);
        assert_eq!(FixedVec2::Y.perp(), -FixedVec2::X);
        // Four applications return to start, exactly.
        let v = FixedVec2::from_ints(7, -3);
        assert_eq!(v.perp().perp().perp().perp(), v);
        // And perp is always orthogonal.
        assert_eq!(v.dot(v.perp()), Fixed::ZERO);
    }

    #[test]
    fn clamp_length_leaves_short_vectors_untouched() {
        let v = FixedVec2::from_ints(3, 4); // length 5
        assert_eq!(v.clamp_length(Fixed::from_int(10)), v);
        assert_eq!(v.clamp_length(Fixed::from_int(5)), v);
    }

    #[test]
    fn clamp_length_shortens_long_vectors() {
        let v = FixedVec2::from_ints(30, 40); // length 50
        let c = v.clamp_length(Fixed::from_int(5));
        let err = (c.length() - Fixed::from_int(5)).abs();
        assert!(err <= Fixed::from_bits(8), "clamped to {}", c.length());
        // Direction is preserved: still parallel, so cross product ~ 0.
        assert!(v.cross_z(c).abs() <= Fixed::from_bits(64));
    }

    #[test]
    fn clamp_length_of_zero_is_zero() {
        assert_eq!(
            FixedVec2::ZERO.clamp_length(Fixed::from_int(5)),
            FixedVec2::ZERO
        );
    }

    #[test]
    fn scale_by_one_is_identity() {
        let v = FixedVec2::from_ints(-11, 6);
        assert_eq!(v * Fixed::ONE, v);
        assert_eq!(v * Fixed::ZERO, FixedVec2::ZERO);
    }

    #[test]
    fn lerp_endpoints_are_exact() {
        let a = FixedVec2::from_ints(0, 0);
        let b = FixedVec2::from_ints(10, 20);
        assert_eq!(a.lerp(b, Fixed::ZERO), a);
        assert_eq!(a.lerp(b, Fixed::ONE), b);
        assert_eq!(a.lerp(b, Fixed::HALF), FixedVec2::from_ints(5, 10));
    }

    #[test]
    fn debug_formatting_uses_no_floats() {
        assert_eq!(
            format!("{:?}", FixedVec2::from_ints(1, -2)),
            "(1.000000000, -2.000000000)"
        );
    }
}
