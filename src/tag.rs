use std::{marker::PhantomData, num::NonZeroU64};

use derive_where::derive_where;

use crate::{
    core::{cell::OptRefMut, token::MainThreadToken, token_cell::NOptRefCell},
    entity::Entity,
    util::{
        map::NopHashMap,
        set::{FreeListHeap, SetMap, SetMapRef},
    },
};

// === TagManager === //

type ArchetypeRef = SetMapRef<RawTag, ManagedArchetype, FreeListHeap>;

#[derive(Default)]
struct TagManager {
    archetypes: SetMap<RawTag, ManagedArchetype, FreeListHeap>,
    tags: NopHashMap<RawTag, ManagedTag>,
}

struct ManagedTag {
    archetypes: Vec<ArchetypeRef>,
}

#[derive(Default)]
struct ManagedArchetype {
    members: Vec<Entity>,
}

fn tag_manager() -> OptRefMut<'static, TagManager> {
    static TAG_MANAGER: NOptRefCell<TagManager> = NOptRefCell::new_empty();

    let token = MainThreadToken::try_acquire()
        .expect("attempted to perform a tag operation on a non-main thread");

    if TAG_MANAGER.is_empty(token) {
        TAG_MANAGER.replace(token, Some(TagManager::default()));
    }

    TAG_MANAGER.borrow_mut(token)
}

// === Tag === //

#[derive_where(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Tag<T: 'static> {
    _ty: PhantomData<fn() -> T>,
    raw: RawTag,
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
