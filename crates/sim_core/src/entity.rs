//! `EntityAllocator` — generational slot allocation with an intrusive free
//! list. TECH_SPEC §3.
//!
//! # Determinism requirements this type carries
//! Allocation order must be a pure function of the sequence of alloc/free
//! calls. That rules out anything resembling "pick a convenient free slot":
//! the free list is LIFO and nothing else is permitted to reorder it. Two
//! clients replaying the same command log must hand out the same indices in
//! the same order, because the indices end up in the state hash — and because
//! §6.9 resolves ties by handle index, so the indices are themselves gameplay.

use serde::{Deserialize, Serialize};

use crate::ids::{EntityHandle, EntityIndex};

/// End-of-list marker for the intrusive free list.
const NIL: u32 = u32::MAX;

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct EntityAllocator {
    /// Current generation of each slot. Starts at 1; 0 means "invalid handle",
    /// so a slot is never in generation 0 while live.
    generations: Box<[u32]>,
    /// Intrusive free list: for a free slot, the next free slot (or NIL).
    /// For a live slot the value is meaningless and is not read.
    next_free: Box<[u32]>,
    /// Head of the free list, or NIL when full.
    free_head: u32,
    live: u32,
}

impl EntityAllocator {
    /// Build an allocator over `capacity` slots, all free.
    ///
    /// The free list is threaded so that `alloc()` hands out index 0 first,
    /// then 1, and so on, which makes tests and desync traces readable.
    pub fn with_capacity(capacity: u32) -> EntityAllocator {
        assert!(capacity > 0, "EntityAllocator capacity must be non-zero");
        assert!(
            capacity < LIVE_MARKER,
            "EntityAllocator capacity must stay below both sentinels"
        );
        let generations = vec![1u32; capacity as usize].into_boxed_slice();
        let mut next_free = vec![NIL; capacity as usize].into_boxed_slice();
        for i in 0..capacity {
            next_free[i as usize] = if i + 1 < capacity { i + 1 } else { NIL };
        }
        EntityAllocator {
            generations,
            next_free,
            free_head: 0,
            live: 0,
        }
    }

    #[inline]
    pub fn capacity(&self) -> u32 {
        self.generations.len() as u32
    }

    #[inline]
    pub fn live_count(&self) -> u32 {
        self.live
    }

    #[inline]
    pub fn is_full(&self) -> bool {
        self.free_head == NIL
    }

    /// Take the next free slot.
    ///
    /// Returns `None` when full rather than panicking: running out of entities
    /// is a content/capacity problem to surface as a dropped spawn and a
    /// logged event, not a crash mid-match. §1.4 sizes the arrays so this
    /// should not happen; if it does, `engine.yaml` is wrong.
    pub fn alloc(&mut self) -> Option<EntityHandle> {
        let index = self.free_head;
        if index == NIL {
            return None;
        }
        self.free_head = self.next_free[index as usize];
        // Mark the slot live. This is what makes `is_alive` O(1) without a
        // second array and without walking the free list.
        self.next_free[index as usize] = LIVE_MARKER;
        self.live += 1;
        Some(EntityHandle::new(index, self.generations[index as usize]))
    }

    /// Release a slot. Returns false if the handle was already stale, in which
    /// case nothing changes — double-free is a no-op, not corruption.
    pub fn free(&mut self, handle: EntityHandle) -> bool {
        if !self.is_alive(handle) {
            return false;
        }
        let i = handle.index as usize;
        // Bump the generation so every outstanding handle to this occupant
        // stops resolving. Skip 0 on wrap, since 0 means "invalid".
        self.generations[i] = match self.generations[i].wrapping_add(1) {
            0 => 1,
            g => g,
        };
        self.next_free[i] = self.free_head;
        self.free_head = handle.index;
        self.live -= 1;
        true
    }

    /// Does this handle still refer to the entity it was issued for?
    #[inline]
    pub fn is_alive(&self, handle: EntityHandle) -> bool {
        if handle.is_invalid() || handle.index >= self.capacity() {
            return false;
        }
        // A freed slot's generation has been bumped past the handle's, and a
        // never-allocated slot is only reachable via a fabricated handle.
        self.generations[handle.index as usize] == handle.generation && !self.is_free(handle.index)
    }

    /// Whether a slot is currently on the free list.
    ///
    /// O(1): `alloc` stamps `LIVE_MARKER` into `next_free` and `free` replaces
    /// it with a real link, so the slot's own entry says which it is. Walking
    /// the list to find out would be O(n) per query.
    #[inline]
    fn is_free(&self, index: EntityIndex) -> bool {
        self.next_free[index as usize] != LIVE_MARKER
    }

    /// Slots currently in use, in ascending index order.
    ///
    /// Ascending order is not a convenience — §6.9 requires deterministic
    /// iteration, and index order is the one total order every client agrees
    /// on without communicating.
    pub fn iter_live(&self) -> impl Iterator<Item = EntityHandle> + '_ {
        (0..self.capacity()).filter_map(move |i| {
            if self.next_free[i as usize] == LIVE_MARKER {
                Some(EntityHandle::new(i, self.generations[i as usize]))
            } else {
                None
            }
        })
    }

    /// Fold into the state hash. Order is fixed by construction.
    pub fn hash_into(&self, h: &mut crate::hash::SimHasher) {
        h.write_u32_slice(&self.generations);
        h.write_u32_slice(&self.next_free);
        h.write_u32(self.free_head);
        h.write_u32(self.live);
    }
}

/// Sentinel stored in `next_free` for a slot that is currently allocated.
///
/// Using a distinct marker rather than inferring liveness keeps `is_alive`
/// O(1) without a second array. `NIL` cannot serve double duty because a free
/// slot at the tail of the list legitimately holds `NIL`.
const LIVE_MARKER: u32 = u32::MAX - 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates_in_ascending_index_order() {
        let mut a = EntityAllocator::with_capacity(4);
        let hs: Vec<_> = (0..4).map(|_| a.alloc().unwrap()).collect();
        assert_eq!(hs.iter().map(|h| h.index).collect::<Vec<_>>(), [0, 1, 2, 3]);
        assert!(a.is_full());
        assert_eq!(a.alloc(), None, "must return None, not panic, when full");
    }

    #[test]
    fn live_count_tracks_alloc_and_free() {
        let mut a = EntityAllocator::with_capacity(8);
        assert_eq!(a.live_count(), 0);
        let h = a.alloc().unwrap();
        assert_eq!(a.live_count(), 1);
        assert!(a.free(h));
        assert_eq!(a.live_count(), 0);
    }

    #[test]
    fn a_freed_handle_stops_resolving() {
        let mut a = EntityAllocator::with_capacity(4);
        let h = a.alloc().unwrap();
        assert!(a.is_alive(h));
        assert!(a.free(h));
        assert!(
            !a.is_alive(h),
            "stale handle still resolved — use-after-free"
        );
    }

    #[test]
    fn double_free_is_a_no_op_not_corruption() {
        let mut a = EntityAllocator::with_capacity(4);
        let h = a.alloc().unwrap();
        assert!(a.free(h));
        assert!(!a.free(h), "second free should report failure");
        assert_eq!(a.live_count(), 0);
        // And the free list is still sane: we can allocate everything.
        let got: Vec<_> = std::iter::from_fn(|| a.alloc()).collect();
        assert_eq!(got.len(), 4);
    }

    #[test]
    fn recycled_slot_gets_a_new_generation() {
        let mut a = EntityAllocator::with_capacity(2);
        let first = a.alloc().unwrap();
        a.free(first);
        let second = a.alloc().unwrap();
        assert_eq!(second.index, first.index, "should reuse the slot");
        assert_ne!(second.generation, first.generation, "generation must bump");
        assert!(a.is_alive(second));
        assert!(!a.is_alive(first));
    }

    #[test]
    fn free_list_is_lifo_so_recycling_is_predictable() {
        let mut a = EntityAllocator::with_capacity(4);
        let h: Vec<_> = (0..4).map(|_| a.alloc().unwrap()).collect();
        a.free(h[1]);
        a.free(h[3]);
        // LIFO: 3 was freed last, so it comes back first.
        assert_eq!(a.alloc().unwrap().index, 3);
        assert_eq!(a.alloc().unwrap().index, 1);
    }

    #[test]
    fn fabricated_handles_do_not_resolve() {
        let mut a = EntityAllocator::with_capacity(4);
        let real = a.alloc().unwrap();
        assert!(!a.is_alive(EntityHandle::new(real.index, real.generation + 1)));
        assert!(!a.is_alive(EntityHandle::new(999, 1)), "out of range");
        assert!(!a.is_alive(EntityHandle::INVALID));
    }

    #[test]
    fn iter_live_is_ascending_and_matches_live_count() {
        let mut a = EntityAllocator::with_capacity(8);
        let h: Vec<_> = (0..8).map(|_| a.alloc().unwrap()).collect();
        a.free(h[2]);
        a.free(h[5]);
        let live: Vec<u32> = a.iter_live().map(|x| x.index).collect();
        assert_eq!(live, [0, 1, 3, 4, 6, 7]);
        assert_eq!(live.len() as u32, a.live_count());
    }

    #[test]
    fn alloc_free_cycles_are_reproducible() {
        // The determinism requirement stated in the module docs: the same
        // sequence of calls yields the same handles.
        fn run() -> Vec<EntityHandle> {
            let mut a = EntityAllocator::with_capacity(16);
            let mut out = Vec::new();
            let mut held = Vec::new();
            for i in 0..64 {
                if i % 3 == 2 && !held.is_empty() {
                    let h: EntityHandle = held.remove(0);
                    a.free(h);
                } else if let Some(h) = a.alloc() {
                    out.push(h);
                    held.push(h);
                }
            }
            out
        }
        assert_eq!(run(), run());
    }

    #[test]
    fn generation_never_lands_on_zero_when_it_wraps() {
        let mut a = EntityAllocator::with_capacity(1);
        // Force the generation to the top of its range.
        a.generations[0] = u32::MAX;
        let h = a.alloc().unwrap();
        assert_eq!(h.generation, u32::MAX);
        a.free(h);
        assert_eq!(a.generations[0], 1, "wrapped generation must skip 0");
    }
}
