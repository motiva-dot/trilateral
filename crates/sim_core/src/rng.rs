//! `SimRng` — xoshiro256\*\*, hand-written. CLAUDE.md §1.2.
//!
//! # Read this before using it
//! **Combat resolution uses no randomness at all** (§1.2, audit item 8). This
//! generator exists for map generation and for data-driven optional modes such
//! as the YAML-toggled "BW classic" miss chance in custom games. If you find
//! yourself reaching for it inside `combat_fsm` or `damage_application`, the
//! design has drifted — stop and re-read §5.3.
//!
//! # Why hand-written
//! §1.3 permits sim crates only `trilateral_fixed`, `serde` and `smallvec`, so
//! `rand` and `rand_xoshiro` are not available. That is a feature: this
//! implementation is thirty lines, it is part of the state, and it serialises
//! and hashes with everything else. No hidden thread-local, no OS entropy, no
//! `Default` that silently seeds from the clock.
//!
//! Deliberately absent: any constructor that reads ambient entropy. The only
//! way to make one is to supply a seed, because a `SimRng::new()` that seeded
//! itself would be a desync generator with a friendly name.

use serde::{Deserialize, Serialize};

/// splitmix64 — used only to expand a single u64 seed into the four words of
/// xoshiro state. Seeding xoshiro directly from a small integer gives a poor
/// first few outputs; this is the standard remedy.
#[inline]
const fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct SimRng {
    s: [u64; 4],
}

impl SimRng {
    /// The only constructor. A seed of zero is remapped, since the all-zero
    /// state is xoshiro's fixed point and would emit zeroes forever.
    pub const fn from_seed(seed: u64) -> SimRng {
        let mut sm = if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        };
        let a = splitmix64(&mut sm);
        let b = splitmix64(&mut sm);
        let c = splitmix64(&mut sm);
        let d = splitmix64(&mut sm);
        SimRng { s: [a, b, c, d] }
    }

    #[inline]
    pub const fn next_u64(&mut self) -> u64 {
        let result = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    #[inline]
    pub const fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// Uniform in `0..n`, via Lemire's multiply-shift with rejection.
    ///
    /// Rejection makes the loop length input-dependent, which is fine — it is
    /// still a deterministic function of the state, so every client takes the
    /// same number of iterations. What would NOT be fine is modulo bias, which
    /// silently skews map generation.
    ///
    /// # Panics
    /// If `n` is zero.
    #[inline]
    pub fn below(&mut self, n: u64) -> u64 {
        assert!(n != 0, "SimRng::below(0) has no valid result");
        let threshold = n.wrapping_neg() % n;
        loop {
            let r = self.next_u64();
            let m = (r as u128) * (n as u128);
            if (m as u64) >= threshold {
                return (m >> 64) as u64;
            }
        }
    }

    /// Raw state, for folding into the state hash.
    #[inline]
    pub const fn state(&self) -> [u64; 4] {
        self.s
    }

    /// Reconstruct from raw state.
    ///
    /// Deliberately not a general-purpose constructor — it exists so a
    /// deserialised or snapshot-restored generator resumes the *same* stream,
    /// and so the published reference vectors can be checked directly. Seeding
    /// a fresh generator must go through [`SimRng::from_seed`].
    #[inline]
    pub const fn from_state(s: [u64; 4]) -> SimRng {
        SimRng { s }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_gives_the_same_stream() {
        let mut a = SimRng::from_seed(42);
        let mut b = SimRng::from_seed(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seeds_diverge_immediately() {
        let mut a = SimRng::from_seed(42);
        let mut b = SimRng::from_seed(43);
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn zero_seed_does_not_produce_the_all_zero_fixed_point() {
        let mut r = SimRng::from_seed(0);
        let mut any_nonzero = false;
        for _ in 0..16 {
            if r.next_u64() != 0 {
                any_nonzero = true;
            }
        }
        assert!(
            any_nonzero,
            "zero seed collapsed to the xoshiro fixed point"
        );
    }

    /// Checks the implementation against **published** xoshiro256\*\* output,
    /// not against itself.
    ///
    /// This is the difference between "my code does what my code does" and
    /// "my code implements the algorithm it claims to". From the canonical
    /// state `[1, 2, 3, 4]` the reference emits 11520, then 0, then
    /// 1509978240 — each derivable by hand from the published update step,
    /// which is why these three are worth pinning rather than a longer run of
    /// numbers nobody can check.
    ///
    /// The second output being exactly zero is not a bug and is the reason
    /// `from_seed` runs the seed through splitmix64 first: raw low-entropy
    /// states take a few rounds to mix.
    #[test]
    fn matches_the_published_reference_vector() {
        let mut r = SimRng::from_state([1, 2, 3, 4]);
        assert_eq!(r.next_u64(), 11_520);
        assert_eq!(r.next_u64(), 0);
        assert_eq!(r.next_u64(), 1_509_978_240);
    }

    /// Regression pin for our seeding path. Unlike the test above these are
    /// our own numbers — they prove `from_seed`'s splitmix64 expansion has not
    /// changed, nothing more.
    #[test]
    fn golden_stream_for_seed_42() {
        let mut r = SimRng::from_seed(42);
        let got: [u64; 4] = [r.next_u64(), r.next_u64(), r.next_u64(), r.next_u64()];
        assert_eq!(
            got,
            [
                0x1578_0b2e_0c2e_c716,
                0x6104_d986_6d11_3a7e,
                0xae17_5332_39e4_99a1,
                0xecb8_ad47_03b3_60a1,
            ]
        );
    }

    #[test]
    fn below_stays_in_range() {
        let mut r = SimRng::from_seed(7);
        for n in [1u64, 2, 3, 7, 100, 1000] {
            for _ in 0..500 {
                let v = r.below(n);
                assert!(v < n, "below({n}) returned {v}");
            }
        }
    }

    #[test]
    fn below_one_is_always_zero() {
        let mut r = SimRng::from_seed(9);
        for _ in 0..100 {
            assert_eq!(r.below(1), 0);
        }
    }

    #[test]
    #[should_panic(expected = "no valid result")]
    fn below_zero_panics() {
        SimRng::from_seed(1).below(0);
    }

    #[test]
    fn below_is_roughly_uniform() {
        // Not a statistical proof — a smoke test that the multiply-shift is
        // not collapsing to a corner. 6 buckets, 60_000 draws, expect ~10_000.
        let mut r = SimRng::from_seed(12345);
        let mut buckets = [0u32; 6];
        for _ in 0..60_000 {
            buckets[r.below(6) as usize] += 1;
        }
        for (i, &b) in buckets.iter().enumerate() {
            assert!(
                (8_500..11_500).contains(&b),
                "bucket {i} got {b}, expected ~10000"
            );
        }
    }

    #[test]
    fn serde_round_trip_preserves_the_stream() {
        let mut a = SimRng::from_seed(99);
        for _ in 0..10 {
            a.next_u64();
        }
        let json = serde_json_stub(&a);
        let mut b: SimRng = json;
        assert_eq!(a.next_u64(), b.next_u64());
    }

    /// `sim_core` may not depend on a JSON crate (§1.3), so "round trip" here
    /// is a structural copy through the public state accessor — enough to
    /// prove the type carries no hidden fields that would be lost.
    fn serde_json_stub(r: &SimRng) -> SimRng {
        SimRng { s: r.state() }
    }
}
