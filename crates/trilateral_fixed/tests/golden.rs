//! The golden hash — Phase 1's exit gate, and the first real test of the
//! determinism thesis on actual hardware.
//!
//! Everything else in this crate asserts that one operation gives one right
//! answer. This asserts that **ten thousand operations composed together give
//! bit-identical results on x86-64 Linux, ARM64 Linux and x86-64 Windows**.
//! CI runs it on all three; if the hash below is wrong anywhere, the merge is
//! blocked.
//!
//! # What a failure here means
//! Not "adjust the constant". A moved hash means the arithmetic changed:
//! a rounding rule, a table entry, an iteration count, or a platform doing
//! something we did not predict. Find out which before touching this file.
//! Updating the constant to make the test pass is the single most destructive
//! thing anyone can do to this project, because every stored replay and every
//! future desync investigation is calibrated against it.
//!
//! Legitimately changing it requires an ADR in the Ledger saying what changed
//! and why, and it invalidates all existing replays.

use trilateral_fixed::{Fixed, FixedAngle, FixedVec2};

/// splitmix64 — chosen because it is four lines, has no state beyond a u64,
/// and is trivially identical everywhere. This drives the *test*, not the
/// simulation; `SimRng` (Phase 2) is a separate concern.
fn next_u64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// FNV-1a over explicit little-endian bytes.
///
/// `to_le_bytes`, not `to_ne_bytes`: byte order must be a property of the
/// algorithm, not of the machine. Every platform CI currently runs on is
/// little-endian, so this costs nothing today and removes a whole category of
/// future surprise.
struct Hasher(u64);

impl Hasher {
    fn new() -> Hasher {
        Hasher(0xcbf2_9ce4_8422_2325)
    }
    fn fold_i64(&mut self, v: i64) {
        for b in v.to_le_bytes() {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01B3);
        }
    }
    fn fold(&mut self, v: Fixed) {
        self.fold_i64(v.to_bits());
    }
    fn fold_vec(&mut self, v: FixedVec2) {
        self.fold(v.x);
        self.fold(v.y);
    }
    fn fold_angle(&mut self, a: FixedAngle) {
        self.fold_i64(a.to_bits() as i64);
    }
}

/// Operands stay inside +/-10_000 so that products (up to 1e8) sit far below
/// the +/-2.147e9 ceiling. Debug builds panic on overflow, which is exactly
/// what we want everywhere *except* here, where the point is to exercise the
/// arithmetic rather than the overflow behaviour.
const BOUND: Fixed = Fixed::from_bits(10_000i64 << 32);

fn bounded(raw: u64) -> Fixed {
    // Take 45 bits so values span roughly +/-4096 with a full fraction.
    let v = ((raw & 0x1FFF_FFFF_FFFF) as i64) - 0x1000_0000_0000;
    Fixed::from_bits(v).clamp(-BOUND, BOUND)
}

/// Divisors are pushed to magnitude >= 1 so quotients stay in range. Without
/// this a 1-ULP divisor produces a 2^32x amplification and overflows.
fn safe_divisor(f: Fixed) -> Fixed {
    if f.abs() < Fixed::ONE { Fixed::ONE } else { f }
}

fn run_fuzz(ops: usize) -> u64 {
    let mut rng: u64 = 42; // the seed named in TECH_SPEC's arena spec
    let mut h = Hasher::new();
    let mut acc = Fixed::ONE;

    for _ in 0..ops {
        let r = next_u64(&mut rng);
        let operand = bounded(next_u64(&mut rng));
        let angle = FixedAngle::from_bits(next_u64(&mut rng) as u32);

        acc = match r % 16 {
            0 => acc + operand,
            1 => acc - operand,
            2 => acc.mul(operand),
            3 => acc.div(safe_divisor(operand)),
            4 => acc.abs().sqrt(),
            5 => acc.floor(),
            6 => acc.ceil(),
            7 => acc.round(),
            8 => acc.fract(),
            9 => angle.sin(),
            10 => angle.cos(),
            11 => {
                let a = FixedAngle::atan2(acc, safe_divisor(operand));
                h.fold_angle(a);
                Fixed::from_bits(a.to_bits() as i64).mul(Fixed::EPSILON)
            }
            12 => {
                let v = FixedVec2::new(acc, operand);
                h.fold_i64(v.length_sq_wide() as i64);
                v.length()
            }
            13 => {
                let v = FixedVec2::new(acc, operand).normalize();
                h.fold_vec(v);
                v.x
            }
            14 => {
                let v = FixedVec2::new(acc, operand).rotate(angle);
                h.fold_vec(v);
                v.y
            }
            _ => acc.lerp(operand, Fixed::HALF),
        };

        // Keep the accumulator in range without ever short-circuiting the
        // operation above — clamping after the fact still exercises the real
        // arithmetic path.
        acc = acc.clamp(-BOUND, BOUND);
        h.fold(acc);
    }
    h.0
}

/// The committed golden hash.
///
/// Produced by this exact code on 2026-08-04. Do not edit without reading the
/// module docs above.
const GOLDEN_10K: u64 = 0x3373_a9ec_5352_e0b7;

#[test]
fn ten_thousand_operations_hash_to_the_committed_value() {
    let actual = run_fuzz(10_000);
    assert_eq!(
        actual, GOLDEN_10K,
        "\n\nGOLDEN HASH MISMATCH\n\
         expected {GOLDEN_10K:#018x}\n\
         actual   {actual:#018x}\n\n\
         The arithmetic changed. Do NOT update the constant to match.\n\
         Find what moved: a rounding rule, a trig table entry, an iteration\n\
         count, or a platform difference. See the module docs.\n"
    );
}

#[test]
fn the_fuzz_is_reproducible_within_a_single_run() {
    // A weaker check that fails fast and locally if something is reading
    // uninitialised or ambient state, before blaming the cross-platform leg.
    assert_eq!(run_fuzz(1_000), run_fuzz(1_000));
    assert_eq!(run_fuzz(10_000), run_fuzz(10_000));
}

#[test]
fn prefixes_differ_from_the_full_run() {
    // Guards against a hash that has saturated or stopped absorbing input —
    // the failure mode where a "golden" test passes because it hashes nothing.
    let a = run_fuzz(100);
    let b = run_fuzz(1_000);
    let c = run_fuzz(10_000);
    assert_ne!(a, b);
    assert_ne!(b, c);
    assert_ne!(a, c);
}
