use std::{
    any::{type_name, Any, TypeId},
    hash,
    num::NonZeroU64,
};

use crate::{
    core::{
        heap::{Heap, Slot},
        token::MainThreadToken,
        token_cell::NOptRefCell,
    },
    util::{
        arena::LeakyArena,
        block::{BlockAllocator, BlockReservation},
        hash_map::{FxHashMap, NopHashMap},
        set_map::{SetMap, SetMapPtr},
    },
    OptRefMut,
};

// === DB: Root === //

static DB: NOptRefCell<DbRoot> = NOptRefCell::new_empty();

pub(crate) fn db(token: &'static MainThreadToken) -> OptRefMut<'static, DbRoot> {
    if DB.is_empty(token) {
        DB.replace(
            token,
            Some(DbRoot {
                entity_gen: NonZeroU64::new(1).unwrap(),
                entities: NopHashMap::default(),
                comp_list_map: SetMap::default(),
                storages: FxHashMap::default(),
            }),
        );
    }

    DB.borrow_mut(token)
}

pub(crate) struct DbRoot {
    pub(crate) entity_gen: NonZeroU64,
    pub(crate) entities: NopHashMap<Entity, DbEntity>,
    pub(crate) comp_list_map: DbComponentListMap,
    pub(crate) storages: FxHashMap<TypeId, &'static (dyn Any + Sync)>,
}

pub(crate) struct DbEntity {
    pub(crate) comp_list: DbComponentListRef,
}

pub(crate) struct DbStorageInner<T: 'static> {
    pub(crate) misc_block_alloc: BlockAllocator<Heap<T>>,
    pub(crate) component_map: NopHashMap<Entity, DbEntityMapping<T>>,
}

pub(crate) struct DbEntityMapping<T: 'static> {
    pub(crate) target: Slot<T>,
    pub(crate) heap: DbEntityMappingHeap<T>,
}

pub(crate) enum DbEntityMappingHeap<T: 'static> {
    Misc(BlockReservation<Heap<T>>),
    Arch,
}

// === DB: ComponentList === //

#[derive(Copy, Clone)]
pub(crate) struct DbComponentType {
    pub(crate) id: TypeId,
    pub(crate) name: &'static str,
    pub(crate) dtor: fn(Entity),
}

impl DbComponentType {
    pub(crate) fn of<T: 'static>() -> Self {
        fn dtor<T: 'static>(entity: Entity) {
            todo!();
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

// === API: Entity === //

#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct Entity(NonZeroU64);
