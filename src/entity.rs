// === ComponentList === //

use std::{
    any::{type_name, Any, TypeId},
    borrow::Borrow,
    cell::{Cell, RefCell},
    fmt, hash, mem,
    num::NonZeroU64,
    rc::Rc,
    sync::atomic,
};

use crate::{
    core::{
        cell::{OptRef, OptRefMut},
        heap::{Heap, Slot, WritableSlot},
        token::{ensure_main_thread, MainThreadToken},
        token_cell::{MainThreadJail, NOptRefCell},
    },
    debug::{AsDebugLabel, DebugLabel},
    obj::{Obj, OwnedObj},
    util::{
        hash_iter, leak, merge_iters, random_uid, AnyDowncastExt, ConstSafeBuildHasherDefault,
        FxHashMap, FxHashSet, NopHashMap, RawFmt,
    },
};

#[derive(Copy, Clone)]
pub(crate) struct ComponentType {
    id: TypeId,
    name: &'static str,
    dtor: fn(Entity),
}

impl ComponentType {
    fn of<T: 'static>() -> Self {
        fn dtor<T: 'static>(entity: Entity) {
            drop(storage::<T>().try_remove_untracked(entity)); // (ignores missing components)
        }

        Self {
            id: TypeId::of::<T>(),
            name: type_name::<T>(),
            dtor: dtor::<T>,
        }
    }
}

impl Ord for ComponentType {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.id.cmp(&other.id)
    }
}

impl PartialOrd for ComponentType {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl hash::Hash for ComponentType {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl Eq for ComponentType {}

impl PartialEq for ComponentType {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

pub(crate) struct ComponentList {
    comps: Box<[ComponentType]>,
    extensions: RefCell<FxHashMap<TypeId, &'static Self>>,
    de_extensions: RefCell<FxHashMap<TypeId, &'static Self>>,
}

impl hash::Hash for ComponentList {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        hash_iter(state, self.comps.iter());
    }
}

impl Eq for ComponentList {}

impl PartialEq for ComponentList {
    fn eq(&self, other: &Self) -> bool {
        self.comps == other.comps
    }
}

impl ComponentList {
    pub fn empty() -> &'static Self {
        thread_local! {
            // N.B. we don't need `assert_blessed` here because methods on `ComponentList` are only
            // called after liveness has been determined.
            static EMPTY: &'static ComponentList = leak(ComponentList {
                comps: Box::new([]),
                extensions: Default::default(),
                de_extensions: Default::default(),
            });
        }

        EMPTY.with(|v| *v)
    }

    pub fn run_dtors(&self, target: Entity) {
        for comp in &*self.comps {
            (comp.dtor)(target);
        }
    }

    pub fn extend(&'static self, with: ComponentType) -> &'static Self {
        self.extensions
            .borrow_mut()
            .entry(with.id)
            .or_insert_with(|| {
                if self.comps.contains(&with) {
                    self
                } else {
                    Self::find_extension_in_db(&self.comps, with)
                }
            })
    }

    pub fn de_extend(&'static self, without: ComponentType) -> &'static Self {
        self.de_extensions
            .borrow_mut()
            .entry(without.id)
            .or_insert_with(|| {
                if !self.comps.contains(&without) {
                    self
                } else {
                    Self::find_de_extension_in_db(&self.comps, without)
                }
            })
    }

    // === Database === //

    thread_local! {
        // N.B. we don't need `assert_blessed` here because methods on `ComponentList` are only
        // called after liveness has been determined.
        static COMP_LISTS: RefCell<FxHashSet<&'static ComponentList>> = {
            RefCell::new(FxHashSet::from_iter([
                ComponentList::empty(),
            ]))
        };
    }

    fn find_extension_in_db(base_set: &[ComponentType], with: ComponentType) -> &'static Self {
        struct ComponentListSearch<'a>(&'a [ComponentType], ComponentType);

        impl hash::Hash for ComponentListSearch<'_> {
            fn hash<H: hash::Hasher>(&self, state: &mut H) {
                hash_iter(state, merge_iters(self.0, &[self.1]));
            }
        }

        impl hashbrown::Equivalent<&'static ComponentList> for ComponentListSearch<'_> {
            fn equivalent(&self, key: &&'static ComponentList) -> bool {
                // See if the key component list without the additional component is equal to the
                // base list.
                key.comps.iter().filter(|v| **v == self.1).eq(self.0.iter())
            }
        }

        ComponentList::COMP_LISTS.with(|set| {
            *set.borrow_mut()
                .get_or_insert_with(&ComponentListSearch(base_set, with), |_| {
                    leak(Self {
                        comps: Box::from_iter(merge_iters(base_set.iter().copied(), [with])),
                        extensions: Default::default(),
                        de_extensions: Default::default(),
                    })
                })
        })
    }

    fn find_de_extension_in_db(
        base_set: &[ComponentType],
        without: ComponentType,
    ) -> &'static Self {
        struct ComponentListSearch<'a>(&'a [ComponentType], ComponentType);

        impl hash::Hash for ComponentListSearch<'_> {
            fn hash<H: hash::Hasher>(&self, state: &mut H) {
                hash_iter(state, self.0.iter().filter(|v| **v != self.1));
            }
        }

        impl hashbrown::Equivalent<&'static ComponentList> for ComponentListSearch<'_> {
            fn equivalent(&self, key: &&'static ComponentList) -> bool {
                // See if the base component list without the removed component is equal to the key
                // list.
                self.0.iter().filter(|v| **v == self.1).eq(key.comps.iter())
            }
        }

        ComponentList::COMP_LISTS.with(|set| {
            *set.borrow_mut()
                .get_or_insert_with(&ComponentListSearch(base_set, without), |_| {
                    leak(Self {
                        comps: base_set
                            .iter()
                            .copied()
                            .filter(|v| *v != without)
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                        extensions: Default::default(),
                        de_extensions: Default::default(),
                    })
                })
        })
    }
}

// === Storage === //

// Aliases
pub type CompRef<T> = OptRef<'static, T>;

pub type CompMut<T> = OptRefMut<'static, T>;

// Database
pub(crate) struct StorageDb {
    pub(crate) storages: FxHashMap<TypeId, &'static (dyn Any + Sync)>,
}

pub(crate) static STORAGES: NOptRefCell<StorageDb> = NOptRefCell::new_full(StorageDb {
    storages: FxHashMap::with_hasher(ConstSafeBuildHasherDefault::new()),
});

pub fn storage<T: 'static>() -> Storage<T> {
    let token = MainThreadToken::acquire();
    let mut db = STORAGES.borrow_mut(token);
    let inner = db
        .storages
        .entry(TypeId::of::<T>())
        .or_insert_with(|| {
            leak::<StorageData<T>>(NOptRefCell::new_full(StorageInner {
                mappings: NopHashMap::default(),
                alloc: StorageInnerAllocator {
                    target_block: None,
                    non_full_blocks: Vec::new(),
                },
            }))
        })
        .downcast_ref::<StorageData<T>>()
        .unwrap();

    Storage { inner, token }
}

// Structures
pub(crate) type StorageData<T> = NOptRefCell<StorageInner<T>>;

#[derive(Debug)]
pub(crate) struct StorageInner<T: 'static> {
    pub(crate) mappings: NopHashMap<Entity, EntityStorageMapping<T>>,
    alloc: StorageInnerAllocator<T>,
}

#[derive(Debug)]
pub(crate) struct EntityStorageMapping<T: 'static> {
    pub(crate) slot: Slot<T>,
    internal_meta: Option<(StorageBlock<T>, usize)>,
}

#[derive(Debug)]
struct StorageInnerAllocator<T: 'static> {
    target_block: Option<StorageBlock<T>>,
    non_full_blocks: Vec<StorageBlock<T>>,
}

type StorageBlock<T> = MainThreadJail<Rc<StorageBlockInner<T>>>;

#[derive(Debug)]
struct StorageBlockInner<T: 'static> {
    heap: RefCell<Heap<T>>,
    slot: Cell<usize>,
    free_mask: Cell<u128>,
}

const HAMMERED_OR_FULL_BLOCK_SLOT: usize = usize::MAX;

// Storage API
#[derive(Debug)]
pub struct Storage<T: 'static> {
    inner: &'static StorageData<T>,
    token: &'static MainThreadToken,
}

impl<T: 'static> Storage<T> {
    pub fn acquire() -> Storage<T> {
        storage()
    }

    // === Insertion === //

    pub fn try_preallocate_slot(
        &self,
        entity: Entity,
        slot: Option<WritableSlot<T>>,
    ) -> Result<Slot<T>, Slot<T>> {
        let me = &mut *self.inner.borrow_mut(self.token);

        match me.mappings.entry(entity) {
            hashbrown::hash_map::Entry::Occupied(entry) => Err(entry.get().slot),
            hashbrown::hash_map::Entry::Vacant(entry) => {
                let (slot, internal_meta) =
                    Self::allocate_slot_if_needed(self.token, &mut me.alloc, slot, None);

                entry.insert(EntityStorageMapping {
                    slot,
                    internal_meta,
                });
                Ok(slot)
            }
        }
    }

    pub fn insert_in_slot(
        &self,
        entity: Entity,
        value: T,
        slot: Option<WritableSlot<T>>,
    ) -> (Obj<T>, Option<T>) {
        // Ensure that the entity is alive and extend the component list.
        ALIVE.with(|slots| {
            let mut slots = slots.borrow_mut();
            let slot = slots.get_mut(&entity).unwrap_or_else(|| {
                panic!(
                    "attempted to attach a component of type {} to the dead or cross-thread {:?}.",
                    type_name::<T>(),
                    entity
                )
            });

            // N.B. we always run this, regardless of entry state, to account for preallocated slots,
            // which don't update the entry yet.
            *slot = slot.extend(ComponentType::of::<T>());
        });

        // Update the storage
        let me = &mut *self.inner.borrow_mut(self.token);

        let (slot, replaced) = match me.mappings.entry(entity) {
            hashbrown::hash_map::Entry::Occupied(entry) => {
                let slot = entry.get().slot;
                (
                    slot,
                    Some(mem::replace(
                        &mut *slot.borrow_mut(MainThreadToken::acquire()),
                        value,
                    )),
                )
            }
            hashbrown::hash_map::Entry::Vacant(entry) => {
                let (slot, internal_meta) =
                    Self::allocate_slot_if_needed(self.token, &mut me.alloc, slot, Some(value));

                entry.insert(EntityStorageMapping {
                    slot,
                    internal_meta,
                });
                (slot, None)
            }
        };

        (Obj::from_raw_parts(entity, slot), replaced)
    }

    fn allocate_slot_if_needed(
        token: &'static MainThreadToken,
        allocator: &mut StorageInnerAllocator<T>,
        slot: Option<WritableSlot<T>>,
        value: Option<T>,
    ) -> (Slot<T>, Option<(StorageBlock<T>, usize)>) {
        // If the user specified a slot of their own, use it.
        if let Some(slot) = slot {
            if let Some(value) = value {
                slot.write(token, Some(value));
            }
            return (*slot, None);
        }

        // Otherwise, acquire a block...
        let block = allocator.target_block.get_or_insert_with(|| {
            match allocator.non_full_blocks.pop() {
                Some(block) => {
                    // Set our slot to a sentinel value so people don't try to remove us when we
                    // become empty.
                    block.get(token).slot.set(HAMMERED_OR_FULL_BLOCK_SLOT);

                    block
                }
                None => {
                    MainThreadJail::new_unjail(
                        token,
                        Rc::new(StorageBlockInner {
                            // TODO: Make this dynamic
                            heap: RefCell::new(Heap::new(128)),
                            slot: Cell::new(HAMMERED_OR_FULL_BLOCK_SLOT),
                            free_mask: Cell::new(0),
                        }),
                    )
                }
            }
        });

        let block_inner = block.get_mut();

        // Find the first open slot
        let mut free_mask = block_inner.free_mask.get();
        let slot_idx = free_mask.trailing_ones();

        // Allocate a slot
        let heap = block_inner.heap.borrow_mut();
        let slot = heap.slot(slot_idx as usize);
        slot.write(token, value);
        let slot = *slot;
        drop(heap);

        // Mark the slot as occupied
        free_mask |= 1 << slot_idx;
        block_inner.free_mask.set(free_mask);

        // If our mask if full, remove the block
        let block_clone = MainThreadJail::new_unjail(token, block_inner.clone());
        if free_mask == u128::MAX {
            // N.B. `block` is already located in the `HAMMERED_OR_FULL_BLOCK_SLOT`.
            allocator.target_block = None;
        }

        (slot, Some((block_clone, slot_idx as usize)))
    }

    pub fn insert_and_return_slot(&self, entity: Entity, value: T) -> (Obj<T>, Option<T>) {
        self.insert_in_slot(entity, value, None)
    }

    pub fn insert(&self, entity: Entity, value: T) -> Option<T> {
        self.insert_and_return_slot(entity, value).1
    }

    // === Removal === //

    pub fn remove(&self, entity: Entity) -> Option<T> {
        if let Some(removed) = self.try_remove_untracked(entity) {
            // Modify the component list or fail silently if the entity lacks the component.
            // This behavior allows users to `remove` components explicitly from entities that are
            // in the of being destroyed. This is the opposite behavior of `insert`, which requires
            // the entity to be valid before modifying it. This pairing ensures that, by the time
            // `Entity::destroy()` resolves, all of the entity's components will have been removed.
            ALIVE.with(|slots| {
                let mut slots = slots.borrow_mut();
                let Some(slot) = slots.get_mut(&entity) else { return };

                *slot = slot.de_extend(ComponentType::of::<T>());
            });

            Some(removed)
        } else {
            // Only if the component is missing will we issue the standard error.
            assert!(
                entity.is_alive(),
                "attempted to remove a component of type {} from the already fully-dead or cross-thread {:?}",
                type_name::<T>(),
                entity,
            );
            None
        }
    }

    fn try_remove_untracked(&self, entity: Entity) -> Option<T> {
        let mut me = self.inner.borrow_mut(self.token);

        // Remove the mapping
        if let Some(mapping) = me.mappings.remove(&entity) {
            // Destroy the Orc
            let taken = mapping.slot.take(self.token);

            // If the slot is internal, deallocate from the block.
            if let Some((block, slot)) = mapping.internal_meta {
                let block = block.into_inner();

                // Update the free mask
                let old_free_mask = block.free_mask.get();
                let mut free_mask = old_free_mask;
                debug_assert_ne!(free_mask & (1 << slot), 0);
                free_mask ^= 1 << slot;
                block.free_mask.set(free_mask);

                // If the block was full but no longer is, add it to the `non_full_blocks` list.
                if old_free_mask == u128::MAX {
                    // In this case, the block is just full and non-hammered.
                    debug_assert_eq!(block.slot.get(), HAMMERED_OR_FULL_BLOCK_SLOT);

                    // Set the block's location
                    block.slot.set(me.alloc.non_full_blocks.len());

                    // Push the block back into the `non_free_blocks` set
                    me.alloc
                        .non_full_blocks
                        .push(MainThreadJail::new_unjail(self.token, block.clone()));
                }

                // If the mask is now empty and the block is not our "hammered" block, delete it!
                let block_index = block.slot.get();
                if free_mask == 0 && block_index != HAMMERED_OR_FULL_BLOCK_SLOT {
                    // Destroy our block reference.
                    drop(block);

                    // Swap-remove the block from the `non_full_blocks` list and take ownership of
                    // its contents.
                    let block = Rc::try_unwrap(
                        me.alloc
                            .non_full_blocks
                            .swap_remove(block_index)
                            .into_inner(),
                    )
                    .ok()
                    .unwrap();

                    // Update the perturbed index
                    if let Some(perturbed) = me.alloc.non_full_blocks.get_mut(block_index) {
                        perturbed.get_mut().slot.set(block_index);
                    }

                    // Deallocate the block
                    drop(block.heap);
                }
            }

            taken
        } else {
            None
        }
    }

    // === Getters === //

    pub fn try_get_slot(&self, entity: Entity) -> Option<Slot<T>> {
        self.inner
            .borrow(self.token)
            .mappings
            .get(&entity)
            .map(|mapping| mapping.slot)
    }

    pub fn get_slot(&self, entity: Entity) -> Slot<T> {
        self.try_get_slot(entity).unwrap_or_else(|| {
            panic!(
                "failed to find component of type {} for {:?}",
                type_name::<T>(),
                entity,
            )
        })
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

impl<T: 'static> Copy for Storage<T> {}

impl<T: 'static> Clone for Storage<T> {
    fn clone(&self) -> Self {
        *self
    }
}

// === Entity === //

pub(crate) static DEBUG_ENTITY_COUNTER: atomic::AtomicU64 = atomic::AtomicU64::new(0);

thread_local! {
    pub(crate) static ALIVE: RefCell<NopHashMap<Entity, &'static ComponentList>> = {
        ensure_main_thread("Spawned, despawned, or checked the liveness of an entity");
        Default::default()
    };
}

#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct Entity(NonZeroU64);

impl Entity {
    pub fn new_unmanaged() -> Self {
        // Allocate a slot
        let me = Self(random_uid());

        // Register our slot in the alive set
        // N.B. we call `ComponentList::empty()` within the `ALIVE.with` section to ensure that blessed
        // validation occurs before we allocate an empty component list.
        ALIVE.with(|slots| slots.borrow_mut().insert(me, ComponentList::empty()));

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

    pub fn try_preallocate_slot<T: 'static>(
        self,
        slot: Option<WritableSlot<T>>,
    ) -> Result<Slot<T>, Slot<T>> {
        storage::<T>().try_preallocate_slot(self, slot)
    }

    pub fn insert_in_slot<T: 'static>(
        self,
        comp: T,
        slot: Option<WritableSlot<T>>,
    ) -> (Obj<T>, Option<T>) {
        storage::<T>().insert_in_slot(self, comp, slot)
    }

    pub fn insert_and_return_slot<T: 'static>(self, comp: T) -> (Obj<T>, Option<T>) {
        storage::<T>().insert_and_return_slot(self, comp)
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
        ALIVE.with(|slots| slots.borrow().contains_key(&self))
    }

    pub fn destroy(self) {
        ALIVE.with(|slots| {
            // Remove the slot
            let slot = slots.borrow_mut().remove(&self).unwrap_or_else(|| {
                panic!(
                    "attempted to destroy the already-dead or cross-threaded {:?}.",
                    self
                )
            });

            // Run the component destructors
            slot.run_dtors(self);
        });
    }
}

impl fmt::Debug for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct StrLit<'a>(&'a str);

        impl fmt::Debug for StrLit<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.0)
            }
        }

        ALIVE.with(|alive| {
            #[derive(Debug)]
            struct Id(NonZeroU64);

            if let Some(components) = alive.borrow().get(self) {
                let mut builder = f.debug_tuple("Entity");

                if let Some(label) = self.try_get::<DebugLabel>() {
                    builder.field(&label);
                }

                builder.field(&Id(self.0));

                for v in components.comps.iter() {
                    if v.id != TypeId::of::<DebugLabel>() {
                        builder.field(&StrLit(v.name));
                    }
                }

                builder.finish()
            } else {
                f.debug_tuple("Entity")
                    .field(&RawFmt("<dead or cross-thread>"))
                    .field(&Id(self.0))
                    .finish()
            }
        })
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

    pub fn try_preallocate_slot<T: 'static>(
        &self,
        slot: Option<WritableSlot<T>>,
    ) -> Result<Slot<T>, Slot<T>> {
        self.entity.try_preallocate_slot(slot)
    }

    pub fn insert_in_slot<T: 'static>(
        &self,
        comp: T,
        slot: Option<WritableSlot<T>>,
    ) -> (Obj<T>, Option<T>) {
        self.entity.insert_in_slot(comp, slot)
    }

    pub fn insert_and_return_slot<T: 'static>(&self, comp: T) -> (Obj<T>, Option<T>) {
        self.entity.insert_and_return_slot(comp)
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

impl Borrow<Entity> for OwnedEntity {
    fn borrow(&self) -> &Entity {
        &self.entity
    }
}

impl Drop for OwnedEntity {
    fn drop(&mut self) {
        self.entity.destroy();
    }
}
