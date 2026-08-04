//! `SimHasher` — the state hash. ADR-006.
//!
//! CLAUDE.md §1.5 makes one promise: same seed + same command log gives a
//! bit-identical state hash at every checkpoint on every platform. This type
//! is what computes that number, so it has exactly three requirements.
//!
//! 1. **Deterministic.** No ambient state, no randomised seed, no dependence
//!    on pointer values, allocation addresses, or iteration order.
//! 2. **Order-sensitive.** Two states holding the same values in different
//!    slots must hash differently, or an entity-ordering bug becomes invisible
//!    — which is the exact class of bug the arena exists to catch.
//! 3. **Avalanching.** A one-bit change anywhere must scramble the output. A
//!    desync that flips one HP value has to be visible at the checkpoint.
//!
//! Note what is *not* on that list: interoperability. Nothing outside this
//! repository ever reproduces these hashes, which is why ADR-006 rejected
//! vendoring xxh3 — a subtly incorrect xxh3 would carry a name implying
//! external test vectors it does not actually satisfy. This is an honest
//! custom fold with its own golden vectors pinned below.
//!
//! Endianness is explicit everywhere. Every CI platform is little-endian
//! today, so this costs nothing and removes a category of future surprise.

/// Fixed seed. Arbitrary but frozen — changing it invalidates every stored
/// replay and every committed golden hash.
const SEED: u64 = 0x243F_6A88_85A3_08D3;
const P1: u64 = 0x9E37_79B1_85EB_CA87;
const P2: u64 = 0xC2B2_AE3D_27D4_EB4F;

/// 64-bit finaliser (the murmur3 fmix64 constants). Strong avalanche, four
/// integer operations, no table.
#[inline]
const fn fmix64(mut z: u64) -> u64 {
    z ^= z >> 33;
    z = z.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    z ^= z >> 33;
    z = z.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    z ^= z >> 33;
    z
}

#[derive(Clone, Copy, Debug)]
pub struct SimHasher {
    acc: u64,
    /// Word count, folded in at the end so that appending zeroes changes the
    /// result. Without it, `[1]` and `[1, 0]` would be hard to separate.
    words: u64,
}

impl Default for SimHasher {
    fn default() -> Self {
        SimHasher::new()
    }
}

impl SimHasher {
    #[inline]
    pub const fn new() -> SimHasher {
        SimHasher {
            acc: SEED,
            words: 0,
        }
    }

    /// The rotate is what makes this order-sensitive: without it the fold
    /// would be closer to commutative, and two entities swapping slots would
    /// hash the same.
    #[inline]
    pub const fn write_u64(&mut self, v: u64) {
        self.acc = (self.acc ^ fmix64(v)).rotate_left(31).wrapping_mul(P1);
        self.words += 1;
    }

    #[inline]
    pub const fn write_i64(&mut self, v: i64) {
        self.write_u64(v as u64);
    }

    #[inline]
    pub const fn write_u32(&mut self, v: u32) {
        self.write_u64(v as u64);
    }

    #[inline]
    pub const fn write_i32(&mut self, v: i32) {
        self.write_u64(v as u32 as u64);
    }

    #[inline]
    pub const fn write_u8(&mut self, v: u8) {
        self.write_u64(v as u64);
    }

    #[inline]
    pub const fn write_bool(&mut self, v: bool) {
        self.write_u64(v as u64);
    }

    #[inline]
    pub fn write_u64_slice(&mut self, vs: &[u64]) {
        for &v in vs {
            self.write_u64(v);
        }
    }

    #[inline]
    pub fn write_i64_slice(&mut self, vs: &[i64]) {
        for &v in vs {
            self.write_i64(v);
        }
    }

    #[inline]
    pub fn write_i32_slice(&mut self, vs: &[i32]) {
        for &v in vs {
            self.write_i32(v);
        }
    }

    #[inline]
    pub fn write_u32_slice(&mut self, vs: &[u32]) {
        for &v in vs {
            self.write_u32(v);
        }
    }

    #[inline]
    pub const fn finish(&self) -> u64 {
        fmix64(self.acc ^ self.words.wrapping_mul(P2))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash_of(vs: &[u64]) -> u64 {
        let mut h = SimHasher::new();
        h.write_u64_slice(vs);
        h.finish()
    }

    // ---- golden vectors. Ours, not anyone else's — see ADR-006. ----------

    #[test]
    fn empty_hash_is_pinned() {
        assert_eq!(SimHasher::new().finish(), 0x7acd_bb98_b134_4213);
    }

    #[test]
    fn golden_vectors_are_stable() {
        assert_eq!(hash_of(&[0]), 0x6d2e_9c85_c8fb_d153);
        assert_eq!(hash_of(&[1]), 0xbc10_d92c_21f8_3304);
        assert_eq!(hash_of(&[1, 2, 3, 4]), 0xc153_8612_f09c_b71f);
    }

    // ---- the three requirements from the module docs ----------------------

    #[test]
    fn order_matters() {
        // Requirement 2. If this ever fails, entity-ordering bugs go invisible.
        assert_ne!(hash_of(&[1, 2]), hash_of(&[2, 1]));
        assert_ne!(hash_of(&[1, 2, 3]), hash_of(&[3, 2, 1]));
    }

    #[test]
    fn trailing_zeroes_change_the_result() {
        // The reason `words` is folded in at the end.
        assert_ne!(hash_of(&[1]), hash_of(&[1, 0]));
        assert_ne!(hash_of(&[]), hash_of(&[0]));
        assert_ne!(hash_of(&[0]), hash_of(&[0, 0]));
    }

    #[test]
    fn a_single_bit_flip_avalanches() {
        // Requirement 3. A desync that moves one HP by one point must be
        // loud at the checkpoint, not a near-miss.
        let base = hash_of(&[0xDEAD_BEEF_CAFE_0000]);
        for bit in 0..64 {
            let flipped = hash_of(&[0xDEAD_BEEF_CAFE_0000 ^ (1u64 << bit)]);
            let differing = (base ^ flipped).count_ones();
            assert!(
                differing >= 16,
                "flipping bit {bit} changed only {differing} output bits"
            );
        }
    }

    #[test]
    fn a_one_ulp_change_deep_in_a_long_run_is_visible() {
        // The realistic desync shape: 2048 entities, one of them off by one.
        let mut a: Vec<u64> = (0..2048).collect();
        let clean = hash_of(&a);
        a[1337] += 1;
        let dirty = hash_of(&a);
        assert_ne!(clean, dirty);
        assert!((clean ^ dirty).count_ones() >= 16);
    }

    #[test]
    fn hashing_is_reproducible() {
        // Requirement 1, locally. The cross-platform half is CI's job.
        assert_eq!(hash_of(&[7, 8, 9]), hash_of(&[7, 8, 9]));
    }

    // ---- width helpers must agree with their u64 forms --------------------

    #[test]
    fn signed_and_unsigned_writers_are_consistent() {
        let mut a = SimHasher::new();
        a.write_i32(-1);
        let mut b = SimHasher::new();
        b.write_u64(0xFFFF_FFFF);
        assert_eq!(
            a.finish(),
            b.finish(),
            "i32 -1 should widen as u32 0xFFFFFFFF"
        );

        let mut c = SimHasher::new();
        c.write_i64(-1);
        let mut d = SimHasher::new();
        d.write_u64(u64::MAX);
        assert_eq!(c.finish(), d.finish());
    }

    #[test]
    fn bool_writes_are_distinct() {
        let mut t = SimHasher::new();
        t.write_bool(true);
        let mut f = SimHasher::new();
        f.write_bool(false);
        assert_ne!(t.finish(), f.finish());
    }
}
