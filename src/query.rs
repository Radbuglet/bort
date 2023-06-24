use std::{fmt, marker::PhantomData};

use derive_where::derive_where;

use crate::{
    core::{heap::DirectSlot, token::MainThreadToken},
    database::{
        DbRoot, DbStorage, InertTag, QueryChunk, QueryChunkStorageIter,
        QueryChunkStorageIterConverter,
    },
    entity::{CompMut, CompRef, Entity},
    util::misc::NamedTypeId,
};

// === Tag === //

pub struct VirtualTagMarker {
    _never: (),
}

pub type VirtualTag = Tag<VirtualTagMarker>;

#[derive_where(Debug, Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct Tag<T: 'static> {
    _ty: PhantomData<fn() -> T>,
    raw: RawTag,
}

impl<T> Tag<T> {
    pub fn new() -> Self {
        Self {
            _ty: PhantomData,
            raw: RawTag::new(NamedTypeId::of::<T>()),
        }
    }

    pub fn raw(self) -> RawTag {
        self.raw
    }
}

impl<T> Into<RawTag> for Tag<T> {
    fn into(self) -> RawTag {
        self.raw
    }
}

#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct RawTag(pub(crate) InertTag);

impl RawTag {
    pub fn new(ty: NamedTypeId) -> Self {
        DbRoot::get(MainThreadToken::acquire_fmt("create tag"))
            .spawn_tag(ty)
            .into_dangerous_tag()
    }
}

impl fmt::Debug for RawTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawTag")
            .field("id", &self.0.id())
            .field("ty", &self.0.ty())
            .finish()
    }
}

// === Flushing === //

pub fn flush() {
    let token = MainThreadToken::acquire_fmt("flush entity archetypes");
    DbRoot::get(token).flush_archetypes(token);
}

// === Queries === //

pub trait Query {
    type PreparedState;
    type Iter: IntoIterator<Item = Self::Item>;

    type Item;
    type Zipped;

    fn extend_tags(self, tags: &mut Vec<InertTag>);

    fn prepare_state(db: &mut DbRoot, token: &'static MainThreadToken) -> Self::PreparedState;

    fn iter(
        token: &'static MainThreadToken,
        state: &mut Self::PreparedState,
        chunk: &mut QueryChunk,
    ) -> Self::Iter;

    fn zip(item: Self::Item, entity: Entity) -> Self::Zipped;
}

// Converters
mod sealed_converters {
    use super::*;

    pub struct RefConverter(pub &'static MainThreadToken);

    impl<T: 'static> QueryChunkStorageIterConverter<T> for RefConverter {
        type Output = CompRef<T>;

        fn convert(&mut self, slot: DirectSlot<'_, T>) -> Self::Output {
            slot.borrow(self.0)
        }
    }

    pub struct MutConverter(pub &'static MainThreadToken);

    impl<T: 'static> QueryChunkStorageIterConverter<T> for MutConverter {
        type Output = CompMut<T>;

        fn convert(&mut self, slot: DirectSlot<'_, T>) -> Self::Output {
            slot.borrow_mut(self.0)
        }
    }
}

// Ref
#[derive_where(Debug, Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct Ref<T: 'static>(pub Tag<T>);

impl<T: 'static> Query for Ref<T> {
    type PreparedState = &'static DbStorage<T>;
    type Iter = QueryChunkStorageIter<T, sealed_converters::RefConverter>;
    type Item = CompRef<T>;
    type Zipped = (Entity, CompRef<T>);

    fn extend_tags(self, tags: &mut Vec<InertTag>) {
        tags.push(self.0.raw.0);
    }

    fn prepare_state(db: &mut DbRoot, _token: &'static MainThreadToken) -> Self::PreparedState {
        db.get_storage()
    }

    fn iter(
        token: &'static MainThreadToken,
        state: &mut Self::PreparedState,
        chunk: &mut QueryChunk,
    ) -> Self::Iter {
        chunk.iter_storage(
            &mut state.borrow_mut(token),
            sealed_converters::RefConverter(token),
        )
    }

    fn zip(item: Self::Item, entity: Entity) -> Self::Zipped {
        (entity, item)
    }
}

// Mut
#[derive_where(Debug, Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct Mut<T: 'static>(pub Tag<T>);

impl<T: 'static> Query for Mut<T> {
    type PreparedState = &'static DbStorage<T>;
    type Iter = QueryChunkStorageIter<T, sealed_converters::MutConverter>;
    type Item = CompMut<T>;
    type Zipped = (Entity, CompMut<T>);

    fn extend_tags(self, tags: &mut Vec<InertTag>) {
        tags.push(self.0.raw.0);
    }

    fn prepare_state(db: &mut DbRoot, _token: &'static MainThreadToken) -> Self::PreparedState {
        db.get_storage()
    }

    fn iter(
        token: &'static MainThreadToken,
        state: &mut Self::PreparedState,
        chunk: &mut QueryChunk,
    ) -> Self::Iter {
        chunk.iter_storage(
            &mut state.borrow_mut(token),
            sealed_converters::MutConverter(token),
        )
    }

    fn zip(item: Self::Item, entity: Entity) -> Self::Zipped {
        (entity, item)
    }
}

// Driver
pub fn query_all<Q: Query>(query: Q) -> impl Iterator<Item = Q::Zipped> {
    // Acquire guards
    let token = MainThreadToken::acquire_fmt("query entity data");
    let mut db = DbRoot::get(token);
    let guard = db.borrow_query_guard(token);

    // Acquire tags
    let mut tags = Vec::new();
    query.extend_tags(&mut tags);

    // Acquire chunks to be queried
    let chunks = db.prepare_query(&tags);

    // Prepare query state
    let mut prepared = Q::prepare_state(&mut db, token);

    chunks.into_iter().flat_map(move |mut chunk| {
        let _guard_bind = &guard;

        // Get an iterator for storage data
        let data = Q::iter(token, &mut prepared, &mut chunk);

        // Get an iterator for entity data
        let entities = chunk.into_entities(token);

        // Zip up the two iterators
        entities
            .zip(data)
            .map(|(entity, item)| Q::zip(item, entity.into_dangerous_entity()))
    })
}
