//! `BitSet` — fixed-capacity bit array over `u64` words.
//!
//! Used for the `alive` mask and, later, for per-player fog masks (§3.1,
//! audit item 23: fog is a bitmask, never a raycast).
//!
//! Fixed capacity because §1.4 forbids steady-state allocation, and because a
//! growable bitset would make `hash()` depend on how the set had been used
//! rather than on what it contains.

use serde::{Deserialize, Serialize};

const BITS: u32 = 64;

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct BitSet {
    words: Box<[u64]>,
    len: u32,
}

impl BitSet {
    /// All bits clear. Allocating with `vec![0; n]` also **pre-touches** every
    /// page, which ADR-005 requires: a first-touch page fault landing inside
    /// tick 1,000 would show up as an allocation-gate failure or a spurious
    /// benchmark regression.
    pub fn new(len: u32) -> BitSet {
        let words = len.div_ceil(BITS) as usize;
        BitSet {
            words: vec![0u64; words].into_boxed_slice(),
            len,
        }
    }

    #[inline]
    pub fn len(&self) -> u32 {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Out-of-range reads are `false` rather than a panic — a stale handle
    /// resolving to "not alive" is the correct answer, not a crash.
    #[inline]
    pub fn get(&self, i: u32) -> bool {
        if i >= self.len {
            return false;
        }
        self.words[(i / BITS) as usize] & (1u64 << (i % BITS)) != 0
    }

    /// # Panics
    /// If `i` is out of range. Unlike `get`, writing out of range is always a
    /// caller bug, and silently discarding a write would corrupt state.
    #[inline]
    pub fn set(&mut self, i: u32, value: bool) {
        assert!(
            i < self.len,
            "BitSet::set index {i} out of range {}",
            self.len
        );
        let (w, b) = ((i / BITS) as usize, i % BITS);
        if value {
            self.words[w] |= 1u64 << b;
        } else {
            self.words[w] &= !(1u64 << b);
        }
    }

    pub fn clear_all(&mut self) {
        self.words.fill(0);
    }

    #[inline]
    pub fn count_ones(&self) -> u32 {
        self.words.iter().map(|w| w.count_ones()).sum()
    }

    /// Set bits in ascending order. Ascending because §6.9 makes iteration
    /// order gameplay.
    pub fn iter_ones(&self) -> impl Iterator<Item = u32> + '_ {
        (0..self.len).filter(move |&i| self.get(i))
    }

    pub fn hash_into(&self, h: &mut crate::hash::SimHasher) {
        // Hash the words, not the bits: 64x fewer operations, same information.
        // Safe because the tail bits beyond `len` are never set — `set` range
        // checks, and nothing else writes words directly.
        h.write_u64_slice(&self.words);
        h.write_u32(self.len);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_bitset_is_empty() {
        let b = BitSet::new(100);
        assert_eq!(b.count_ones(), 0);
        assert_eq!(b.len(), 100);
        assert!(!b.get(0));
    }

    #[test]
    fn set_and_get_round_trip() {
        let mut b = BitSet::new(200);
        for i in [0u32, 1, 63, 64, 65, 127, 128, 199] {
            b.set(i, true);
        }
        for i in [0u32, 1, 63, 64, 65, 127, 128, 199] {
            assert!(b.get(i), "bit {i} should be set");
        }
        assert_eq!(b.count_ones(), 8);
        assert!(!b.get(2));
    }

    #[test]
    fn clearing_a_bit_leaves_its_neighbours_alone() {
        // The classic word-boundary bug: clearing bit 64 wiping bit 63.
        let mut b = BitSet::new(128);
        b.set(63, true);
        b.set(64, true);
        b.set(65, true);
        b.set(64, false);
        assert!(b.get(63));
        assert!(!b.get(64));
        assert!(b.get(65));
    }

    #[test]
    fn out_of_range_get_is_false_not_a_panic() {
        let b = BitSet::new(10);
        assert!(!b.get(10));
        assert!(!b.get(u32::MAX));
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn out_of_range_set_panics() {
        BitSet::new(10).set(10, true);
    }

    #[test]
    fn a_length_that_is_not_a_multiple_of_sixty_four_still_works() {
        let mut b = BitSet::new(65);
        b.set(64, true);
        assert!(b.get(64));
        assert_eq!(b.count_ones(), 1);
        assert!(!b.get(65));
    }

    #[test]
    fn iter_ones_is_ascending() {
        let mut b = BitSet::new(300);
        for i in [200u32, 5, 130, 64, 0] {
            b.set(i, true);
        }
        assert_eq!(b.iter_ones().collect::<Vec<_>>(), [0, 5, 64, 130, 200]);
    }

    #[test]
    fn clear_all_empties_it() {
        let mut b = BitSet::new(100);
        b.set(50, true);
        b.clear_all();
        assert_eq!(b.count_ones(), 0);
    }

    #[test]
    fn hashing_distinguishes_different_contents() {
        let mut a = BitSet::new(128);
        let b = BitSet::new(128);
        let mut ha = crate::hash::SimHasher::new();
        let mut hb = crate::hash::SimHasher::new();
        a.hash_into(&mut ha);
        b.hash_into(&mut hb);
        assert_eq!(ha.finish(), hb.finish(), "identical sets must agree");

        a.set(77, true);
        let mut ha2 = crate::hash::SimHasher::new();
        a.hash_into(&mut ha2);
        assert_ne!(ha2.finish(), hb.finish(), "one bit must change the hash");
    }

    #[test]
    fn same_bits_different_capacity_hash_differently() {
        let a = BitSet::new(64);
        let b = BitSet::new(128);
        let mut ha = crate::hash::SimHasher::new();
        let mut hb = crate::hash::SimHasher::new();
        a.hash_into(&mut ha);
        b.hash_into(&mut hb);
        assert_ne!(ha.finish(), hb.finish());
    }
}
