use std::{
    any::{type_name, Any, TypeId},
    hash,
    num::NonZeroU64,
};

use crate::{
    core::{
        cell::OptRefMut,
        heap::{Heap, Slot},
        token::MainThreadToken,
        token_cell::NOptRefCell,
    },
    entity::{Entity, Tag},
    util::{
        arena::{FreeListArena, LeakyArena},
        block::{BlockAllocator, BlockReservation},
        hash_map::{FxHashMap, NopHashMap},
        misc::xorshift64,
        set_map::{SetMap, SetMapPtr},
    },
};

// === Root === //

static DB: NOptRefCell<DbRoot> = NOptRefCell::new_empty();

pub(crate) fn db(token: &'static MainThreadToken) -> OptRefMut<'static, DbRoot> {
    if DB.is_empty(token) {
        DB.replace(
            token,
            Some(DbRoot {
                uid_gen: NonZeroU64::new(1).unwrap(),
                alive_entities: NopHashMap::default(),
                comp_list_map: SetMap::default(),
                tag_list_map: SetMap::default(),
                storages: FxHashMap::default(),
            }),
        );
    }

    DB.borrow_mut(token)
}

pub(crate) struct DbRoot {
    pub(crate) uid_gen: NonZeroU64,
    pub(crate) alive_entities: NopHashMap<Entity, DbEntity>,
    pub(crate) comp_list_map: DbComponentListMap,
    pub(crate) tag_list_map: DbArchetypeListMap,
    pub(crate) storages: FxHashMap<TypeId, &'static (dyn Any + Sync)>,
}

#[derive(Copy, Clone)]
pub(crate) struct DbEntity {
    pub(crate) comp_list: DbComponentListRef,
    pub(crate) tag_list: DbArchetypeListRef,
}

pub(crate) type DbStorage<T> = NOptRefCell<DbStorageInner<T>>;

#[derive(Debug)]
pub(crate) struct DbStorageInner<T: 'static> {
    pub(crate) misc_block_alloc: BlockAllocator<Heap<T>>,
    pub(crate) mappings: NopHashMap<Entity, DbEntityMapping<T>>,
}

#[derive(Debug)]
pub(crate) struct DbEntityMapping<T: 'static> {
    pub(crate) target: Slot<T>,
    pub(crate) heap: DbEntityMappingHeap<T>,
}

#[derive(Debug)]
pub(crate) enum DbEntityMappingHeap<T: 'static> {
    Misc(BlockReservation<Heap<T>>),
}

// === ComponentList === //

#[derive(Copy, Clone)]
pub(crate) struct DbComponentType {
    pub(crate) id: TypeId,
    pub(crate) name: &'static str,
    pub(crate) dtor: fn(Entity),
}

impl DbComponentType {
    pub(crate) fn of<T: 'static>() -> Self {
        fn dtor<T: 'static>(entity: Entity) {
            entity.remove::<T>();
        }

        Self {
            id: TypeId::of::<T>(),
            name: type_name::<T>(),
            dtor: dtor::<T>,
        }
    }
}

impl Ord for DbComponentType {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.id.cmp(&other.id)
    }
}

impl PartialOrd for DbComponentType {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl hash::Hash for DbComponentType {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl Eq for DbComponentType {}

impl PartialEq for DbComponentType {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

pub(crate) type DbComponentListMap = SetMap<DbComponentType, (), LeakyArena>;
pub(crate) type DbComponentListRef = SetMapPtr<DbComponentType, (), LeakyArena>;

// === Archetype === //

#[derive(Default)]
pub(crate) struct DbArchetype {}

pub(crate) type DbArchetypeListMap = SetMap<Tag, DbArchetype, FreeListArena>;
pub(crate) type DbArchetypeListRef = SetMapPtr<Tag, DbArchetype, FreeListArena>;

// === Shared logic === //

impl DbRoot {
    pub fn new_uid(&mut self) -> NonZeroU64 {
        self.uid_gen = xorshift64(self.uid_gen);
        self.uid_gen
    }
}
