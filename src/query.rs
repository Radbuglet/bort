use std::{fmt, marker::PhantomData};

use derive_where::derive_where;

use crate::{
    core::token::MainThreadToken,
    database::{DbRoot, InertTag},
    util::misc::NamedTypeId,
    CompMut, Entity,
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

pub fn query_tagged<I>(tags: I) -> impl Iterator<Item = Entity>
where
    I: IntoIterator,
    I::Item: Into<RawTag>,
{
    let tags = tags.into_iter().map(|tag| tag.into().0).collect::<Vec<_>>();

    let token = MainThreadToken::acquire_fmt("enumerate tagged entities");
    let mut db = DbRoot::get(token);

    let guard = db.borrow_query_guard(token);
    let chunks = db.prepare_query(&tags);

    chunks
        .into_iter()
        .flat_map(move |chunk| {
            let _guard_capture = &guard;
            chunk.into_entities(token)
        })
        .map(|inert| inert.into_dangerous_entity())
}

pub fn query_i32(tag: Tag<i32>) -> impl Iterator<Item = (Entity, CompMut<i32>)> {
    let token = MainThreadToken::acquire_fmt("enumerate tagged entities");
    let mut db = DbRoot::get(token);

    let guard = db.borrow_query_guard(token);
    let chunks = db.prepare_query(&[tag.raw.0]);
    let storage = db.get_storage::<i32>();

    chunks.into_iter().flat_map(move |chunk| {
        let _guard_capture = &guard;

        let comps = chunk.iter_storage(&mut storage.borrow_mut(token), |slot| {
            slot.borrow_mut(token)
        });
        let entities = chunk
            .into_entities(token)
            .map(|ent| ent.into_dangerous_entity());

        entities.zip(comps)
    })
}
