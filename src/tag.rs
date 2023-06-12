use std::{cmp::Ordering, fmt, marker::PhantomData, num::NonZeroU64};

use crate::{
    util::{FxHashMap, NopHashMap},
    Entity,
};

// === TagManager === //

struct TagManager {
    archetypes: FxHashMap<Box<[RawTag]>, ManagedArchetype>,
    tags: NopHashMap<RawTag, ManagedTag>,
}

struct ManagedTag {
    archetypes: Vec<()>,
}

struct ManagedArchetype {
    members: Vec<Entity>,
}

// === Tag === //

pub struct Tag<T: 'static> {
    _ty: PhantomData<fn() -> T>,
    raw: RawTag,
}

impl<T> fmt::Debug for Tag<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tag").field("raw", &self.raw).finish()
    }
}

impl<T> Copy for Tag<T> {}

impl<T> Clone for Tag<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Eq for Tag<T> {}

impl<T> PartialEq for Tag<T> {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl<T> Ord for Tag<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.raw.cmp(&other.raw)
    }
}

impl<T> PartialOrd for Tag<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.raw.partial_cmp(&other.raw)
    }
}

impl<T> Into<RawTag> for Tag<T> {
    fn into(self) -> RawTag {
        self.raw
    }
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct RawTag {
    id: NonZeroU64,
}

// === API === //

impl Entity {
    pub fn tag(self, tag: impl Into<RawTag>) {
        todo!();
    }

    pub fn untag(self, tag: impl Into<RawTag>) {
        todo!();
    }

    pub fn has_tag(self, tag: impl Into<RawTag>) -> bool {
        todo!()
    }
}

impl<T> Tag<T> {
    pub fn archetypes(self) {
        todo!();
    }
}

pub fn query() {
    todo!();
}
