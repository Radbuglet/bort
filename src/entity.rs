use std::{
    any::{type_name, TypeId},
    borrow, fmt, mem,
    num::NonZeroU64,
    sync::atomic,
};

use derive_where::derive_where;

use crate::{
    core::{
        cell::{OptRef, OptRefMut},
        heap::{Heap, Slot},
        token::MainThreadToken,
    },
    database::{
        db, DbComponentType, DbEntity, DbEntityMapping, DbEntityMappingHeap, DbStorage,
        DbStorageInner,
    },
    debug::{AsDebugLabel, DebugLabel},
    obj::{Obj, OwnedObj},
    util::{
        block::BlockAllocator,
        hash_map::NopHashMap,
        misc::{leak, xorshift64, AnyDowncastExt, RawFmt},
    },
};

// === Storage === //

// Aliases
pub type CompRef<T> = OptRef<'static, T>;

pub type CompMut<T> = OptRefMut<'static, T>;

// Storage API
pub fn storage<T: 'static>() -> Storage<T> {
    let token = MainThreadToken::acquire_fmt("fetch entity component data");
    let storage = *db(token)
        .storages
        .entry(TypeId::of::<T>())
        .or_insert_with(|| {
            leak(DbStorage::new_full(DbStorageInner::<T> {
                misc_block_alloc: BlockAllocator::default(),
                mappings: NopHashMap::default(),
            }))
        });

    Storage {
        inner: storage.downcast_ref::<DbStorage<T>>().unwrap(),
        token,
    }
}

#[derive(Debug)]
#[derive_where(Copy, Clone)]
pub struct Storage<T: 'static> {
    inner: &'static DbStorage<T>,
    token: &'static MainThreadToken,
}

impl<T: 'static> Storage<T> {
    pub fn acquire() -> Storage<T> {
        storage::<T>()
    }

    // === Insertion === //

    pub fn insert_with_obj(&self, entity: Entity, value: T) -> (Obj<T>, Option<T>) {
        let inner = &mut *self.inner.borrow_mut(self.token);

        // Ensure that the entity is alive
        let db = &mut *db(self.token);
        let entity_info = db.alive_entities.get_mut(&entity).unwrap_or_else(|| {
            panic!(
                "attempted to attach a component of type {} to the dead or cross-thread {:?}.",
                type_name::<T>(),
                entity
            )
        });

        match inner.mappings.entry(entity) {
            hashbrown::hash_map::Entry::Occupied(entry) => {
                // We're merely occupied so just mutate the component without any additional fuss.
                let entry = entry.get();
                let replaced = mem::replace(&mut *entry.target.borrow_mut(self.token), value);

                (Obj::from_raw_parts(entity, entry.target), Some(replaced))
            }
            hashbrown::hash_map::Entry::Vacant(entry) => {
                // Update the component list
                entity_info.comp_list = db
                    .comp_list_map
                    .lookup_extension(Some(&entity_info.comp_list), DbComponentType::of::<T>());

                // Allocate a slot for this object
                let resv = inner.misc_block_alloc.alloc(|sz| Heap::new(self.token, sz));
                let slot = inner
                    .misc_block_alloc
                    .block_mut(&resv.block)
                    .slot(resv.slot);

                // Write the value to the slot
                slot.set_value_owner_pair(self.token, Some((entity, value)));

                // Insert the entry
                let slot = slot.slot();
                entry.insert(DbEntityMapping {
                    target: slot,
                    heap: DbEntityMappingHeap::Misc(resv),
                });

                (Obj::from_raw_parts(entity, slot), None)
            }
        }
    }

    pub fn insert(&self, entity: Entity, value: T) -> Option<T> {
        self.insert_with_obj(entity, value).1
    }

    // === Removal === //

    pub fn remove(&self, entity: Entity) -> Option<T> {
        if let Some(removed) = self.try_remove_untracked(entity) {
            let db = &mut *db(self.token);

            // Modify the component list or fail silently if the entity does not exist.
            //
            // This behavior allows users to `remove` components explicitly from entities that are
            // in the of being destroyed. This is the opposite behavior of `insert`, which requires
            // the entity to be valid before modifying it. This pairing ensures that, by the time
            // `Entity::destroy()` resolves, all of the entity's components will have been removed.
            if let Some(entity) = db.alive_entities.get_mut(&entity) {
                entity.comp_list = db
                    .comp_list_map
                    .lookup_de_extension(&entity.comp_list, DbComponentType::of::<T>());
            }

            Some(removed)
        } else {
            // Only if the component is missing will we issue the standard error.
            assert!(
                entity.is_alive(),
                "attempted to remove a component of type {} from the already fully-dead {:?}",
                type_name::<T>(),
                entity,
            );
            None
        }
    }

    fn try_remove_untracked(&self, entity: Entity) -> Option<T> {
        let inner = &mut *self.inner.borrow_mut(self.token);

        // Unlink the entity
        let removed = inner.mappings.remove(&entity)?;

        // Remove the value from the heap
        let removed_value = removed.target.set_value_owner_pair(self.token, None);

        // Remove the reservation in the heap's allocator.
        match removed.heap {
            DbEntityMappingHeap::Misc(resv) => inner.misc_block_alloc.dealloc(resv, drop),
        }

        removed_value
    }

    // === Getters === //

    pub fn try_get_slot(&self, entity: Entity) -> Option<Slot<T>> {
        self.inner
            .borrow(self.token)
            .mappings
            .get(&entity)
            .map(|mapping| mapping.target)
    }

    pub fn get_slot(&self, entity: Entity) -> Slot<T> {
        let slot = self.try_get_slot(entity).unwrap_or_else(|| {
            panic!(
                "failed to find component of type {} for {:?}",
                type_name::<T>(),
                entity,
            )
        });
        debug_assert_eq!(slot.owner(self.token), Some(entity));

        slot
    }

    pub fn try_get(&self, entity: Entity) -> Option<CompRef<T>> {
        self.try_get_slot(entity)
            .map(|slot| slot.borrow(self.token))
    }

    pub fn try_get_mut(&self, entity: Entity) -> Option<CompMut<T>> {
        self.try_get_slot(entity)
            .map(|slot| slot.borrow_mut(self.token))
    }

    pub fn get(&self, entity: Entity) -> CompRef<T> {
        self.get_slot(entity).borrow(self.token)
    }

    pub fn get_mut(&self, entity: Entity) -> CompMut<T> {
        self.get_slot(entity).borrow_mut(self.token)
    }

    pub fn has(&self, entity: Entity) -> bool {
        self.try_get_slot(entity).is_some()
    }
}

// === Entity === //

pub(crate) static DEBUG_ENTITY_COUNTER: atomic::AtomicU64 = atomic::AtomicU64::new(0);

#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct Entity(NonZeroU64);

impl Entity {
    pub fn new_unmanaged() -> Self {
        let token = MainThreadToken::acquire_fmt("fetch entity component data");
        let db = &mut *db(token);

        // Allocate a slot
        db.entity_gen = xorshift64(db.entity_gen);
        let me = Self(db.entity_gen);

        // Register our slot in the alive set
        db.alive_entities.insert(
            me,
            DbEntity {
                comp_list: *db.comp_list_map.root(),
            },
        );

        // Increment the total entity counter
        // N.B. we do this once everything else has succeeded so that calls to `new_unmanaged` on
        // un-blessed threads don't result in a phantom entity being recorded in the counter.
        DEBUG_ENTITY_COUNTER.fetch_add(1, atomic::Ordering::Relaxed);

        me
    }

    pub fn with<T: 'static>(self, comp: T) -> Self {
        self.insert(comp);
        self
    }

    pub fn with_self_referential<T: 'static>(self, func: impl FnOnce(Entity) -> T) -> Self {
        self.insert(func(self));
        self
    }

    pub fn with_debug_label<L: AsDebugLabel>(self, label: L) -> Self {
        #[cfg(debug_assertions)]
        self.with(DebugLabel::from(label));
        #[cfg(not(debug_assertions))]
        let _ = label;
        self
    }

    pub fn insert_with_obj<T: 'static>(self, comp: T) -> (Obj<T>, Option<T>) {
        storage::<T>().insert_with_obj(self, comp)
    }

    pub fn insert<T: 'static>(self, comp: T) -> Option<T> {
        storage::<T>().insert(self, comp)
    }

    pub fn remove<T: 'static>(self) -> Option<T> {
        storage::<T>().remove(self)
    }

    pub fn try_get_slot<T: 'static>(self) -> Option<Slot<T>> {
        storage::<T>().try_get_slot(self)
    }

    pub fn try_get<T: 'static>(self) -> Option<CompRef<T>> {
        storage::<T>().try_get(self)
    }

    pub fn try_get_mut<T: 'static>(self) -> Option<CompMut<T>> {
        storage::<T>().try_get_mut(self)
    }

    pub fn get_slot<T: 'static>(self) -> Slot<T> {
        storage::<T>().get_slot(self)
    }

    pub fn get<T: 'static>(self) -> CompRef<T> {
        storage::<T>().get(self)
    }

    pub fn get_mut<T: 'static>(self) -> CompMut<T> {
        storage::<T>().get_mut(self)
    }

    pub fn has<T: 'static>(self) -> bool {
        storage::<T>().has(self)
    }

    pub fn obj<T: 'static>(self) -> Obj<T> {
        Obj::wrap(self)
    }

    pub fn is_alive(self) -> bool {
        db(MainThreadToken::acquire_fmt(
            "determine whether an entity was alive",
        ))
        .alive_entities
        .contains_key(&self)
    }

    pub fn destroy(self) {
        let token = MainThreadToken::acquire_fmt("destroy an entity");

        let entity_info = db(token)
            .alive_entities
            .remove(&self)
            .unwrap_or_else(|| panic!("attempted to destroy the already-dead {:?}.", self));

        // Run the component destructors
        for key in entity_info.comp_list.direct_borrow().keys() {
            (key.dtor)(self);
        }
    }
}

impl fmt::Debug for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        #[derive(Debug)]
        struct Id(NonZeroU64);

        if let Some(token) = MainThreadToken::try_acquire() {
            let db = db(token);
            if let Some(&entity_info) = db.alive_entities.get(self) {
                // Move ownership of EntityInfo out of the db so we can call `.try_get`.
                drop(db);

                // Format the component list
                let mut builder = f.debug_tuple("Entity");

                if let Some(label) = self.try_get::<DebugLabel>() {
                    builder.field(&label);
                }

                builder.field(&Id(self.0));

                for v in entity_info.comp_list.direct_borrow().keys().iter() {
                    if v.id != TypeId::of::<DebugLabel>() {
                        builder.field(&RawFmt(v.name));
                    }
                }

                builder.finish()
            } else {
                f.debug_tuple("Entity")
                    .field(&RawFmt("<dead>"))
                    .field(&Id(self.0))
                    .finish()
            }
        } else {
            f.debug_tuple("Entity")
                .field(&RawFmt("<cross-thread>"))
                .field(&Id(self.0))
                .finish()
        }
    }
}

// === OwnedEntity === //

#[derive(Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct OwnedEntity {
    entity: Entity,
}

impl OwnedEntity {
    // === Lifecycle === //

    pub fn new() -> Self {
        Self::from_raw_entity(Entity::new_unmanaged())
    }

    pub fn from_raw_entity(entity: Entity) -> Self {
        Self { entity }
    }

    pub fn entity(&self) -> Entity {
        self.entity
    }

    pub fn unmanage(self) -> Entity {
        let entity = self.entity;
        mem::forget(self);

        entity
    }

    pub fn split_guard(self) -> (Self, Entity) {
        let entity = self.entity();
        (self, entity)
    }

    // === Forwards === //

    pub fn with<T: 'static>(self, comp: T) -> Self {
        self.entity.insert(comp);
        self
    }

    pub fn with_self_referential<T: 'static>(self, func: impl FnOnce(Entity) -> T) -> Self {
        self.entity.insert(func(self.entity()));
        self
    }

    pub fn with_debug_label<L: AsDebugLabel>(self, label: L) -> Self {
        self.entity.with_debug_label(label);
        self
    }

    pub fn insert_with_obj<T: 'static>(&self, comp: T) -> (Obj<T>, Option<T>) {
        self.entity.insert_with_obj(comp)
    }

    pub fn insert<T: 'static>(&self, comp: T) -> Option<T> {
        self.entity.insert(comp)
    }

    pub fn remove<T: 'static>(&self) -> Option<T> {
        self.entity.remove()
    }

    pub fn try_get_slot<T: 'static>(&self) -> Option<Slot<T>> {
        self.entity.try_get_slot()
    }

    pub fn try_get<T: 'static>(&self) -> Option<CompRef<T>> {
        self.entity.try_get()
    }

    pub fn try_get_mut<T: 'static>(&self) -> Option<CompMut<T>> {
        self.entity.try_get_mut()
    }

    pub fn get_slot<T: 'static>(&self) -> Slot<T> {
        self.entity.get_slot()
    }

    pub fn get<T: 'static>(&self) -> CompRef<T> {
        self.entity.get()
    }

    pub fn get_mut<T: 'static>(&self) -> CompMut<T> {
        self.entity.get_mut()
    }

    pub fn has<T: 'static>(&self) -> bool {
        self.entity.has::<T>()
    }

    pub fn obj<T: 'static>(&self) -> Obj<T> {
        self.entity.obj()
    }

    pub fn into_obj<T: 'static>(self) -> OwnedObj<T> {
        OwnedObj::wrap(self)
    }

    pub fn is_alive(&self) -> bool {
        self.entity.is_alive()
    }

    pub fn destroy(self) {
        drop(self);
    }
}

impl Default for OwnedEntity {
    fn default() -> Self {
        Self::new()
    }
}

impl borrow::Borrow<Entity> for OwnedEntity {
    fn borrow(&self) -> &Entity {
        &self.entity
    }
}

impl Drop for OwnedEntity {
    fn drop(&mut self) {
        self.entity.destroy();
    }
}
