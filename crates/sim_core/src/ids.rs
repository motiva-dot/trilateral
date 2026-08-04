//! Handles and identifiers. CLAUDE.md §4 — do not invent synonyms for these.

use serde::{Deserialize, Serialize};

/// Index into the component arrays. Never store one of these across ticks;
/// store an [`EntityHandle`] and resolve it, or you will read a recycled slot.
pub type EntityIndex = u32;

/// A generational handle. TECH_SPEC §3.
///
/// `index` locates the slot; `generation` says which occupant of that slot you
/// meant. Freeing a slot bumps its generation, so every handle to the previous
/// occupant stops resolving — which turns "use after free" from a silent wrong
/// answer into a `None`.
///
/// Generation 0 is reserved to mean "never allocated", so a zeroed handle is
/// invalid rather than accidentally valid.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct EntityHandle {
    pub index: EntityIndex,
    pub generation: u32,
}

impl EntityHandle {
    /// A handle that can never resolve.
    pub const INVALID: EntityHandle = EntityHandle {
        index: u32::MAX,
        generation: 0,
    };

    #[inline]
    pub const fn new(index: EntityIndex, generation: u32) -> EntityHandle {
        EntityHandle { index, generation }
    }

    #[inline]
    pub const fn is_invalid(self) -> bool {
        self.generation == 0
    }
}

/// `Option<EntityHandle>` without the niche question.
///
/// Stored in component arrays, so it must be `Copy`, fixed-size, and hash
/// identically on every platform. A plain `Option` would work, but being
/// explicit about the sentinel keeps the hashed bytes obvious.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct OptionalHandle(EntityHandle);

impl OptionalHandle {
    pub const NONE: OptionalHandle = OptionalHandle(EntityHandle::INVALID);

    #[inline]
    pub const fn some(h: EntityHandle) -> OptionalHandle {
        OptionalHandle(h)
    }

    #[inline]
    pub const fn get(self) -> Option<EntityHandle> {
        if self.0.is_invalid() {
            None
        } else {
            Some(self.0)
        }
    }

    #[inline]
    pub const fn is_none(self) -> bool {
        self.0.is_invalid()
    }

    /// Raw parts, for the state hash.
    #[inline]
    pub const fn raw(self) -> (u32, u32) {
        (self.0.index, self.0.generation)
    }
}

impl Default for OptionalHandle {
    fn default() -> Self {
        OptionalHandle::NONE
    }
}

/// Which player owns a thing. Index into `SimState::players`.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize,
)]
#[repr(transparent)]
pub struct PlayerId(pub u8);

impl PlayerId {
    /// Owner of neutral things — resource nodes, watchtowers, map decor.
    pub const NEUTRAL: PlayerId = PlayerId(u8::MAX);
}

/// Index into the `UnitRegistry` / `BuildingRegistry` loaded from YAML.
///
/// The *only* thing that connects an entity to its stats. No gameplay number
/// lives on the entity itself (§1.8).
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize,
)]
#[repr(transparent)]
pub struct ArchetypeId(pub u16);

impl ArchetypeId {
    /// Archetype of an empty slot.
    pub const NONE: ArchetypeId = ArchetypeId(u16::MAX);
}

/// Index into the `TechRegistry`.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize,
)]
#[repr(transparent)]
pub struct TechId(pub u16);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zeroed_handle_is_invalid() {
        let h = EntityHandle::new(0, 0);
        assert!(h.is_invalid());
        assert_eq!(OptionalHandle::some(h).get(), None);
    }

    #[test]
    fn optional_handle_none_round_trips() {
        assert!(OptionalHandle::NONE.is_none());
        assert_eq!(OptionalHandle::NONE.get(), None);
        assert_eq!(OptionalHandle::default(), OptionalHandle::NONE);
    }

    #[test]
    fn optional_handle_some_round_trips() {
        let h = EntityHandle::new(7, 3);
        assert_eq!(OptionalHandle::some(h).get(), Some(h));
        assert!(!OptionalHandle::some(h).is_none());
    }

    #[test]
    fn handles_order_by_index_then_generation() {
        // Tie-breaking by handle index is CLAUDE.md §6.9; the derived Ord must
        // put index first for that to mean what the spec says.
        let a = EntityHandle::new(1, 9);
        let b = EntityHandle::new(2, 1);
        assert!(a < b, "ordering must be by index first");
    }
}
