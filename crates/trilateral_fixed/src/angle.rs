//! `FixedAngle` — a u32 where one full turn is exactly 2^32.
//!
//! # Why a u32 and not a `Fixed` in radians
//! Because wrapping is then free and exact. Adding two angles cannot drift,
//! there is no modulo step to get wrong, and — the reason this pays for itself
//! in combat code — the signed shortest difference between two angles is
//! `a.wrapping_sub(b) as i32`, with no branches and no special case at the
//! seam. An angle in radians stored as `Fixed` would need a range reduction
//! every time it crossed 2*pi, and range reduction is where trig
//! implementations traditionally start disagreeing with each other.
//!
//! Resolution is 2^-32 of a turn, about 0.3 micro-degrees. TECH_SPEC §5.3
//! wants turn rates fine enough that facing micro reads as analog at 30Hz;
//! this is roughly ten million times finer than that needs.
//!
//! # Method
//! `sin`/`cos` read the committed 4097-entry quarter-wave table in
//! `tables.rs` and interpolate linearly between samples, using quadrant
//! symmetry for the other three quarters. `atan2` is a 32-iteration
//! vectoring-mode CORDIC. Both are pure integer, with a fixed iteration count
//! and no data-dependent early exit.

use crate::fixed::Fixed;
use crate::tables::{CORDIC_ATAN, SIN_QUARTER};
use crate::vec2::FixedVec2;

use core::ops::{Add, AddAssign, Neg, Sub, SubAssign};

use serde::{Deserialize, Serialize};

/// Bits of angle that select the quadrant.
const QUADRANT_SHIFT: u32 = 30;
/// Mask for the within-quadrant phase.
const PHASE_MASK: u32 = (1 << QUADRANT_SHIFT) - 1;
/// One quarter turn, as a raw phase value.
const QUARTER: u32 = 1 << QUADRANT_SHIFT;
/// Phase bits consumed by the table index (4096 == 2^12 entries).
const INDEX_SHIFT: u32 = QUADRANT_SHIFT - 12;
/// Mask for the interpolation fraction between two table samples.
const FRAC_MASK: u32 = (1 << INDEX_SHIFT) - 1;

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(transparent)]
pub struct FixedAngle(u32);

impl FixedAngle {
    pub const ZERO: FixedAngle = FixedAngle(0);
    pub const QUARTER_TURN: FixedAngle = FixedAngle(1 << 30);
    pub const HALF_TURN: FixedAngle = FixedAngle(1 << 31);
    pub const THREE_QUARTER_TURN: FixedAngle = FixedAngle(3 << 30);

    #[inline]
    pub const fn from_bits(bits: u32) -> FixedAngle {
        FixedAngle(bits)
    }

    #[inline]
    pub const fn to_bits(self) -> u32 {
        self.0
    }

    /// Exact for any divisor of 360 that divides 2^32 evenly — which includes
    /// 90, 45, 30 and 15 degrees. Other values round.
    #[inline]
    pub const fn from_degrees(deg: i32) -> FixedAngle {
        FixedAngle((((deg as i64) * (1i64 << 32)) / 360) as u32)
    }

    /// `num/den` of a full turn. `from_turn_ratio(1, 8)` is 45 degrees, exact.
    #[inline]
    pub const fn from_turn_ratio(num: i64, den: i64) -> FixedAngle {
        assert!(den != 0, "FixedAngle::from_turn_ratio: division by zero");
        FixedAngle(((num * (1i64 << 32)) / den) as u32)
    }

    /// Approximate degrees, for debug output only. Truncates.
    #[inline]
    pub const fn to_degrees_trunc(self) -> i32 {
        ((self.0 as i64) * 360 / (1i64 << 32)) as i32
    }

    /// Signed shortest angular difference from `self` to `target`, in raw
    /// angle units, always in `[-half turn, +half turn)`.
    ///
    /// Positive means counter-clockwise. This single line is the reason the
    /// type is a wrapping u32: the seam at 0/360 needs no handling at all.
    #[inline]
    pub const fn shortest_diff(self, target: FixedAngle) -> i32 {
        target.0.wrapping_sub(self.0) as i32
    }

    /// Rotate toward `target` by at most `max_step` raw units.
    ///
    /// This is the turn-rate primitive for the combat FSM (TECH_SPEC §5.3):
    /// a unit must be within `attack_arc` of its target's bearing before it
    /// may enter frontswing, and this is how it gets there.
    #[inline]
    pub const fn turn_toward(self, target: FixedAngle, max_step: u32) -> FixedAngle {
        let d = self.shortest_diff(target);
        if d.unsigned_abs() <= max_step {
            target
        } else if d > 0 {
            FixedAngle(self.0.wrapping_add(max_step))
        } else {
            FixedAngle(self.0.wrapping_sub(max_step))
        }
    }

    /// Sine, as a `Fixed` in `[-1, 1]`.
    #[inline]
    pub const fn sin(self) -> Fixed {
        let quadrant = self.0 >> QUADRANT_SHIFT;
        let phase = self.0 & PHASE_MASK;
        // sin(pi/2 + x) = sin(pi/2 - x), so the odd quadrants read the table
        // backwards. QUARTER - 0 == QUARTER is why the table has 4097 entries.
        let bits = match quadrant {
            0 => quarter_sin(phase),
            1 => quarter_sin(QUARTER - phase),
            2 => -quarter_sin(phase),
            _ => -quarter_sin(QUARTER - phase),
        };
        Fixed::from_bits(bits)
    }

    /// Cosine, as a `Fixed` in `[-1, 1]`.
    #[inline]
    pub const fn cos(self) -> Fixed {
        // cos(t) == sin(t + pi/2), and the addition wraps for free.
        FixedAngle(self.0.wrapping_add(QUARTER)).sin()
    }

    /// The unit vector pointing along this bearing.
    #[inline]
    pub const fn to_unit_vec(self) -> FixedVec2 {
        FixedVec2::new(self.cos(), self.sin())
    }

    /// Bearing of the vector `(x, y)`, by 32-iteration CORDIC.
    ///
    /// `atan2(0, 0)` is defined as zero rather than a panic: degenerate
    /// direction vectors are routine in steering (a unit standing exactly on
    /// its target), and every caller would otherwise need the same guard.
    pub fn atan2(y: Fixed, x: Fixed) -> FixedAngle {
        let mut xi = x.to_bits();
        let mut yi = y.to_bits();
        if xi == 0 && yi == 0 {
            return FixedAngle::ZERO;
        }

        // CORDIC grows the vector by the gain ~1.647 and each iteration adds
        // a shifted copy, so cap the magnitude first. Scaling by a power of
        // two is exact and does not change the angle at all. The shift amount
        // is derived from the inputs, so it is identical on every platform.
        let magnitude = xi.unsigned_abs().max(yi.unsigned_abs());
        let used_bits = 64 - magnitude.leading_zeros();
        const HEADROOM_BITS: u32 = 40;
        if used_bits > HEADROOM_BITS {
            let sh = used_bits - HEADROOM_BITS;
            xi >>= sh;
            yi >>= sh;
        }

        // Vectoring mode converges only within +/-99.7 degrees, so fold the
        // left half-plane over first and account for it in the accumulator.
        // The accumulator is mod 2^32, so "account for it" is just an add.
        let mut z: i64 = 0;
        if xi < 0 {
            xi = -xi;
            yi = -yi;
            z = 1i64 << 31;
        }

        let mut i = 0;
        while i < 32 {
            let dx = yi >> i;
            let dy = xi >> i;
            if yi > 0 {
                xi += dx;
                yi -= dy;
                z += CORDIC_ATAN[i];
            } else {
                xi -= dx;
                yi += dy;
                z -= CORDIC_ATAN[i];
            }
            i += 1;
        }
        FixedAngle(z as u32)
    }

    /// Bearing of a vector. Convenience wrapper over [`FixedAngle::atan2`].
    #[inline]
    pub fn of_vec(v: FixedVec2) -> FixedAngle {
        FixedAngle::atan2(v.y, v.x)
    }
}

/// sin over the first quadrant. `phase` runs `0..=QUARTER`, inclusive at the
/// top so that `QUARTER - 0` is representable.
#[inline]
const fn quarter_sin(phase: u32) -> i64 {
    let index = (phase >> INDEX_SHIFT) as usize;
    if index >= 4096 {
        return SIN_QUARTER[4096];
    }
    let frac = (phase & FRAC_MASK) as i64;
    let a = SIN_QUARTER[index];
    let b = SIN_QUARTER[index + 1];
    a + (((b - a) * frac) >> INDEX_SHIFT)
}

impl FixedVec2 {
    /// Rotate counter-clockwise by `a`.
    #[inline]
    pub fn rotate(self, a: FixedAngle) -> FixedVec2 {
        let (s, c) = (a.sin(), a.cos());
        FixedVec2::new(self.x.mul(c) - self.y.mul(s), self.x.mul(s) + self.y.mul(c))
    }
}

impl Add for FixedAngle {
    type Output = FixedAngle;
    #[inline]
    fn add(self, rhs: FixedAngle) -> FixedAngle {
        FixedAngle(self.0.wrapping_add(rhs.0))
    }
}

impl Sub for FixedAngle {
    type Output = FixedAngle;
    #[inline]
    fn sub(self, rhs: FixedAngle) -> FixedAngle {
        FixedAngle(self.0.wrapping_sub(rhs.0))
    }
}

impl Neg for FixedAngle {
    type Output = FixedAngle;
    #[inline]
    fn neg(self) -> FixedAngle {
        FixedAngle(self.0.wrapping_neg())
    }
}

impl AddAssign for FixedAngle {
    #[inline]
    fn add_assign(&mut self, rhs: FixedAngle) {
        *self = *self + rhs;
    }
}

impl SubAssign for FixedAngle {
    #[inline]
    fn sub_assign(&mut self, rhs: FixedAngle) {
        *self = *self - rhs;
    }
}

impl core::fmt::Debug for FixedAngle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}deg({:#010x})", self.to_degrees_trunc(), self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Linear interpolation between 4096 samples over pi/2 gives a step of
    /// h = (pi/2)/4096 = 3.835e-4 rad. The worst-case interpolation error for
    /// a function with |f''| <= 1 is h^2/8 = 1.838e-8, which is 78.9 units of
    /// Q32.32. Pinned at 96 — comfortably above the derivation, tight enough
    /// that a table regression cannot hide beneath it.
    const SIN_TOLERANCE: i64 = 96;

    fn assert_close(actual: Fixed, expected: Fixed, tol: i64, what: &str) {
        let diff = (actual.to_bits() - expected.to_bits()).abs();
        assert!(
            diff <= tol,
            "{what}: got {actual:?}, expected {expected:?}, diff {diff} > {tol}"
        );
    }

    // ---- cardinal directions must be EXACT, not approximate -------------

    #[test]
    fn sin_at_the_cardinals_is_exact() {
        assert_eq!(FixedAngle::ZERO.sin(), Fixed::ZERO);
        assert_eq!(FixedAngle::QUARTER_TURN.sin(), Fixed::ONE);
        assert_eq!(FixedAngle::HALF_TURN.sin(), Fixed::ZERO);
        assert_eq!(FixedAngle::THREE_QUARTER_TURN.sin(), Fixed::NEG_ONE);
    }

    #[test]
    fn cos_at_the_cardinals_is_exact() {
        assert_eq!(FixedAngle::ZERO.cos(), Fixed::ONE);
        assert_eq!(FixedAngle::QUARTER_TURN.cos(), Fixed::ZERO);
        assert_eq!(FixedAngle::HALF_TURN.cos(), Fixed::NEG_ONE);
        assert_eq!(FixedAngle::THREE_QUARTER_TURN.cos(), Fixed::ZERO);
    }

    #[test]
    fn a_full_turn_wraps_to_zero_exactly() {
        let a = FixedAngle::QUARTER_TURN;
        assert_eq!(a + a + a + a, FixedAngle::ZERO);
        assert_eq!(FixedAngle::from_degrees(360), FixedAngle::ZERO);
    }

    // ---- identities ------------------------------------------------------

    #[test]
    fn pythagorean_identity_holds_across_a_full_sweep() {
        let mut i = 0u64;
        while i < 512 {
            let a = FixedAngle::from_bits(((i * (1u64 << 32)) / 512) as u32);
            let s = a.sin();
            let c = a.cos();
            assert_close(
                s.sq() + c.sq(),
                Fixed::ONE,
                SIN_TOLERANCE * 4,
                "sin^2+cos^2",
            );
            i += 1;
        }
    }

    #[test]
    fn sin_is_odd_and_cos_is_even() {
        for deg in [1i32, 17, 45, 90, 133, 200, 271, 359] {
            let a = FixedAngle::from_degrees(deg);
            assert_close(
                (-a).sin(),
                Fixed::from_bits(-a.sin().to_bits()),
                2,
                "sin(-x) == -sin(x)",
            );
            assert_close((-a).cos(), a.cos(), 2, "cos(-x) == cos(x)");
        }
    }

    #[test]
    fn sin_of_thirty_degrees_is_one_half() {
        // 30 degrees divides 2^32 exactly, so this is a real check of the
        // table rather than of the interpolation.
        assert_close(
            FixedAngle::from_degrees(30).sin(),
            Fixed::HALF,
            SIN_TOLERANCE,
            "sin(30)",
        );
    }

    #[test]
    fn sin_of_forty_five_equals_cos_of_forty_five() {
        let a = FixedAngle::from_turn_ratio(1, 8);
        assert_close(a.sin(), a.cos(), 2, "sin(45) == cos(45)");
        // and both equal sqrt(2)/2
        let root_half = Fixed::HALF.sqrt();
        assert_close(a.sin(), root_half, SIN_TOLERANCE, "sin(45) == sqrt(1/2)");
    }

    #[test]
    fn sin_never_leaves_the_unit_interval() {
        let mut i = 0u64;
        while i < 4096 {
            let a = FixedAngle::from_bits(((i * (1u64 << 32)) / 4096) as u32);
            assert!(a.sin().abs() <= Fixed::ONE, "sin out of range at {a:?}");
            assert!(a.cos().abs() <= Fixed::ONE, "cos out of range at {a:?}");
            i += 1;
        }
    }

    // ---- shortest_diff / turn_toward, the combat primitives ---------------

    #[test]
    fn from_degrees_is_exact_only_for_power_of_two_divisors_of_the_turn() {
        // 2^32 / 360 is not an integer, so most degree values round. This is
        // not a defect, but it IS a trap: differences of rounded angles are
        // not the rounded difference. Prefer `from_turn_ratio` with a power of
        // two whenever a test or a gameplay constant needs to be exact.
        assert_eq!(FixedAngle::from_degrees(90), FixedAngle::QUARTER_TURN);
        assert_eq!(FixedAngle::from_degrees(180), FixedAngle::HALF_TURN);
        assert_eq!(
            FixedAngle::from_degrees(45),
            FixedAngle::from_turn_ratio(1, 8)
        );

        // Whereas: 350 deg and 10 deg each truncate downward, and the
        // difference lands one raw unit off the truncation of 20 deg.
        let a = FixedAngle::from_degrees(350);
        let b = FixedAngle::from_degrees(10);
        let d = a.shortest_diff(b) as i64;
        let twenty = FixedAngle::from_degrees(20).to_bits() as i64;
        assert_eq!(d - twenty, 1, "rounding drift changed; check from_degrees");
    }

    #[test]
    fn shortest_diff_takes_the_short_way_round_the_seam() {
        // Eighths of a turn are exact, so this can assert exact equality.
        let a = FixedAngle::from_turn_ratio(7, 8); // 315 deg
        let b = FixedAngle::from_turn_ratio(1, 8); // 45 deg
        // 315 -> 45 is +90 degrees the short way, not -270.
        let d = a.shortest_diff(b);
        assert!(d > 0, "expected counter-clockwise, got {d}");
        assert_eq!(
            FixedAngle::from_bits(d as u32),
            FixedAngle::QUARTER_TURN,
            "crossing the 0/360 seam did not take the short way"
        );
        // and the reverse is exactly symmetric
        assert_eq!(b.shortest_diff(a), -d);
    }

    #[test]
    fn shortest_diff_at_exactly_half_a_turn_is_unambiguous() {
        // The one genuinely ambiguous case: 180 degrees apart. i32 wrapping
        // resolves it to -half rather than +half, deterministically. Combat
        // code relies on SOME answer here, not on a particular one — but it
        // must be the same answer on every machine.
        let a = FixedAngle::ZERO;
        let b = FixedAngle::HALF_TURN;
        assert_eq!(a.shortest_diff(b), i32::MIN);
        assert_eq!(b.shortest_diff(a), i32::MIN);
    }

    #[test]
    fn shortest_diff_of_equal_angles_is_zero() {
        let a = FixedAngle::from_degrees(123);
        assert_eq!(a.shortest_diff(a), 0);
    }

    #[test]
    fn turn_toward_snaps_when_within_one_step() {
        let a = FixedAngle::from_degrees(0);
        let b = FixedAngle::from_degrees(5);
        let big_step = FixedAngle::from_degrees(10).to_bits();
        assert_eq!(a.turn_toward(b, big_step), b);
    }

    #[test]
    fn turn_toward_takes_the_short_way_and_never_overshoots() {
        let step = FixedAngle::from_degrees(3).to_bits();
        let target = FixedAngle::from_degrees(10);
        let mut a = FixedAngle::from_degrees(350);
        // Should march 350 -> 353 -> 356 -> 359 -> 2 ... -> 10 and stop.
        for _ in 0..10 {
            a = a.turn_toward(target, step);
        }
        assert_eq!(a, target, "did not converge");
        // Once arrived it stays put.
        assert_eq!(a.turn_toward(target, step), target);
    }

    // ---- atan2 ------------------------------------------------------------

    /// CORDIC with a 32-entry table resolves to roughly the last non-zero
    /// entry, a couple of raw units. Pinned generously at 64 units — about
    /// 5.4 millionths of a degree.
    const ATAN_TOLERANCE: i64 = 64;

    fn assert_angle_close(a: FixedAngle, b: FixedAngle, what: &str) {
        let d = a.shortest_diff(b) as i64;
        assert!(
            d.abs() <= ATAN_TOLERANCE,
            "{what}: {a:?} vs {b:?}, diff {d} units"
        );
    }

    #[test]
    fn atan2_of_the_axes_is_exact_or_nearly_so() {
        let one = Fixed::ONE;
        let zero = Fixed::ZERO;
        assert_angle_close(FixedAngle::atan2(zero, one), FixedAngle::ZERO, "+x");
        assert_angle_close(FixedAngle::atan2(one, zero), FixedAngle::QUARTER_TURN, "+y");
        assert_angle_close(FixedAngle::atan2(zero, -one), FixedAngle::HALF_TURN, "-x");
        assert_angle_close(
            FixedAngle::atan2(-one, zero),
            FixedAngle::THREE_QUARTER_TURN,
            "-y",
        );
    }

    #[test]
    fn atan2_of_the_diagonals() {
        let one = Fixed::ONE;
        assert_angle_close(
            FixedAngle::atan2(one, one),
            FixedAngle::from_turn_ratio(1, 8),
            "45",
        );
        assert_angle_close(
            FixedAngle::atan2(one, -one),
            FixedAngle::from_turn_ratio(3, 8),
            "135",
        );
        assert_angle_close(
            FixedAngle::atan2(-one, -one),
            FixedAngle::from_turn_ratio(5, 8),
            "225",
        );
        assert_angle_close(
            FixedAngle::atan2(-one, one),
            FixedAngle::from_turn_ratio(7, 8),
            "315",
        );
    }

    #[test]
    fn atan2_of_zero_is_zero_not_a_panic() {
        assert_eq!(
            FixedAngle::atan2(Fixed::ZERO, Fixed::ZERO),
            FixedAngle::ZERO
        );
    }

    #[test]
    fn atan2_inverts_the_trig_tables_across_a_full_sweep() {
        // The round trip that matters: bearing -> unit vector -> bearing.
        // Tolerance covers table interpolation on the way out as well as
        // CORDIC on the way back.
        let mut i = 0u64;
        while i < 360 {
            let a = FixedAngle::from_degrees(i as i32);
            let v = a.to_unit_vec();
            let back = FixedAngle::of_vec(v);
            let d = (a.shortest_diff(back) as i64).abs();
            assert!(d <= 512, "round trip at {i} deg drifted {d} units");
            i += 1;
        }
    }

    #[test]
    fn atan2_is_scale_invariant() {
        // Only the ratio matters, so a vector and its multiple agree.
        let a = FixedAngle::atan2(Fixed::from_int(3), Fixed::from_int(4));
        let b = FixedAngle::atan2(Fixed::from_int(3000), Fixed::from_int(4000));
        assert_angle_close(a, b, "scale invariance");
    }

    #[test]
    fn atan2_handles_magnitudes_that_need_prescaling() {
        // Large enough to trip the headroom shift; the angle must not move.
        let small = FixedAngle::atan2(Fixed::from_int(1), Fixed::from_int(1));
        let huge = FixedAngle::atan2(Fixed::from_int(2_000_000), Fixed::from_int(2_000_000));
        assert_angle_close(small, huge, "prescaled 45 degrees");
    }

    // ---- rotation ---------------------------------------------------------

    #[test]
    fn rotating_by_ninety_degrees_matches_perp() {
        for (x, y) in [(1, 0), (0, 1), (3, 4), (-7, 2)] {
            let v = FixedVec2::from_ints(x, y);
            let r = v.rotate(FixedAngle::QUARTER_TURN);
            let p = v.perp();
            assert!(
                (r.x - p.x).abs() <= Fixed::from_bits(4)
                    && (r.y - p.y).abs() <= Fixed::from_bits(4),
                "rotate(90) of {v:?} gave {r:?}, perp gave {p:?}"
            );
        }
    }

    #[test]
    fn rotation_preserves_length() {
        let v = FixedVec2::from_ints(3, 4); // length exactly 5
        for deg in [0i32, 17, 90, 180, 271, 359] {
            let r = v.rotate(FixedAngle::from_degrees(deg));
            let err = (r.length() - Fixed::from_int(5)).abs();
            assert!(
                err <= Fixed::from_bits(2048),
                "length drift at {deg}: {err:?}"
            );
        }
    }

    #[test]
    fn rotating_by_a_full_turn_is_identity() {
        let v = FixedVec2::from_ints(5, -9);
        assert_eq!(v.rotate(FixedAngle::ZERO), v);
    }
}
