use std::{
    any::{type_name, Any, TypeId},
    fmt, hash, mem,
    num::NonZeroU64,
};

use derive_where::derive_where;

use crate::{
    core::{
        cell::OptRefMut,
        heap::{Heap, Slot},
        token::MainThreadToken,
        token_cell::NOptRefCell,
    },
    debug::DebugLabel,
    entity::{Entity, RawTag},
    util::{
        arena::{FreeListArena, LeakyArena, SpecArena},
        block::{BlockAllocator, BlockReservation},
        hash_map::{FxHashMap, NopHashMap},
        misc::{const_new_nz_u64, leak, xorshift64, AnyDowncastExt, RawFmt},
        set_map::{SetMap, SetMapPtr},
    },
};

// === Root === //

pub struct DbRoot {
    uid_gen: NonZeroU64,
    alive_entities: NopHashMap<InertEntity, DbEntity>,
    comp_list_map: DbComponentListMap,
    tag_list_map: DbTagListMap,
    storages: FxHashMap<TypeId, &'static (dyn Any + Sync)>,
    dirty_entities: Vec<InertEntity>,
    debug_total_spawns: u64,
}

#[derive(Copy, Clone)]
struct DbEntity {
    comp_list: DbComponentListRef,
    virtual_tag_list: DbTagListRef,
    layout_tag_list: DbTagListRef,
    heap_index: usize,
    slot_index: usize,
}

pub type DbStorage<T> = NOptRefCell<DbStorageInner<T>>;

pub struct DbStorageInner<T: 'static> {
    anon_block_alloc: BlockAllocator<Heap<T>>,
    archetypes: FxHashMap<DbTagListRef, Vec<Heap<T>>>,
    mappings: NopHashMap<InertEntity, DbEntityMapping<T>>,
}

#[derive(Debug)]
struct DbEntityMapping<T: 'static> {
    target: Slot<T>,
    heap: DbEntityMappingHeap<T>,
}

#[derive(Debug)]
enum DbEntityMappingHeap<T: 'static> {
    Anonymous(BlockReservation<Heap<T>>),
    External,
}

// === ComponentList === //

#[derive(Copy, Clone)]
struct DbComponentType {
    pub id: TypeId,
    pub name: &'static str,
    pub dtor: fn(&MainThreadToken, &mut DbRoot, InertEntity),
}

impl DbComponentType {
    fn of<T: 'static>() -> Self {
        fn dtor<T: 'static>(token: &MainThreadToken, db: &mut DbRoot, entity: InertEntity) {
            let storage = db.get_storage::<T>();
            let comp = db.remove_component(token, &mut storage.borrow_mut(token), entity);
            debug_assert!(comp.is_ok());
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

type DbComponentListMap = SetMap<DbComponentType, (), LeakyArena>;
type DbComponentListRef = SetMapPtr<DbComponentType, (), LeakyArena>;

// === TagList === //

#[derive(Default)]
struct DbTagList {
    entity_heaps: Vec<Box<[InertEntity]>>,
    last_heap_len: usize,
}

type DbTagListMap = SetMap<InertTag, DbTagList, FreeListArena>;
type DbTagListRef = SetMapPtr<InertTag, DbTagList, FreeListArena>;

// === Inert Handles === //

// N.B. it is all too easy to accidentally call `Entity`s debug handler while issuing an error,
// causing a borrow error when the database is reborrowed by the debug handler. Hence, we work
// entirely with inert objects at this layer and add all the useful but not-necessarily-idiomatic
// features at a higher layer.

#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct InertEntity(NonZeroU64);

impl InertEntity {
    pub const PLACEHOLDER: Self = Self(const_new_nz_u64(u64::MAX));

    pub const fn into_dangerous_entity(self) -> Entity {
        Entity(self)
    }

    pub fn id(self) -> NonZeroU64 {
        self.0
    }
}

#[derive(Copy, Clone)]
#[derive_where(Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct InertTag {
    id: NonZeroU64,
    #[derive_where(skip)]
    ty: TypeId,
}

impl InertTag {
    pub fn into_dangerous_tag(self) -> RawTag {
        RawTag(self)
    }

    pub fn id(self) -> NonZeroU64 {
        self.id
    }

    pub fn ty(self) -> TypeId {
        self.ty
    }
}

// === Methods === //

impl DbRoot {
    pub fn get(token: &'static MainThreadToken) -> OptRefMut<'static, DbRoot> {
        static DB: NOptRefCell<DbRoot> = NOptRefCell::new_empty();

        if DB.is_empty(token) {
            DB.replace(
                token,
                Some(DbRoot {
                    uid_gen: NonZeroU64::new(1).unwrap(),
                    alive_entities: NopHashMap::default(),
                    comp_list_map: SetMap::default(),
                    tag_list_map: SetMap::default(),
                    storages: FxHashMap::default(),
                    dirty_entities: Vec::new(),
                    debug_total_spawns: 0,
                }),
            );
        }

        DB.borrow_mut(token)
    }

    fn new_uid(&mut self) -> NonZeroU64 {
        self.uid_gen = xorshift64(self.uid_gen);
        self.uid_gen
    }

    pub fn spawn_entity(&mut self) -> InertEntity {
        // Allocate a slot
        let me = InertEntity(self.new_uid());

        // Register our slot in the alive set
        self.alive_entities.insert(
            me,
            DbEntity {
                comp_list: *self.comp_list_map.root(),
                virtual_tag_list: *self.tag_list_map.root(),
                layout_tag_list: *self.tag_list_map.root(),
                heap_index: 0,
                slot_index: 0,
            },
        );

        // Increment the spawn counter
        self.debug_total_spawns += 1;

        me
    }
    pub fn despawn_entity(
        &mut self,
        token: &MainThreadToken,
        entity: InertEntity,
    ) -> Result<(), EntityDeadError> {
        let Some(entity_info) = self
            .alive_entities
            .remove(&entity)
		else {
			return Err(EntityDeadError);
		};

        // N.B. This `direct_borrow` operation could be dangerous since we're not just borrowing
        // they immutable component list, but also the metadata used by the set to update its
        // cache. Fortunately, the actual destructors are operating on logically dead entities and
        // so will therefore never update the component list, making the borrow temporarily safe.
        for comp in entity_info.comp_list.direct_borrow().keys() {
            (comp.dtor)(token, self, entity)
        }

        Ok(())
    }

    pub fn is_entity_alive(&self, entity: InertEntity) -> bool {
        self.alive_entities.contains_key(&entity)
    }

    pub fn spawn_tag(&mut self, ty: TypeId) -> InertTag {
        InertTag {
            id: self.new_uid(),
            ty,
        }
    }

    fn tag_common(
        &mut self,
        entity: InertEntity,
        tag: InertTag,
        is_add: bool,
    ) -> Result<(), EntityDeadError> {
        // Fetch the entity info
        let Some(entity_info) = self.alive_entities.get_mut(&entity) else {
			return Err(EntityDeadError);
        };

        // Determine whether we began dirty
        let was_dirty = entity_info.layout_tag_list != entity_info.virtual_tag_list;

        // Update the list
        entity_info.virtual_tag_list = if is_add {
            self.tag_list_map
                .lookup_extension(Some(&entity_info.virtual_tag_list), tag, |_| {
                    Default::default()
                })
        } else {
            self.tag_list_map
                .lookup_de_extension(&entity_info.virtual_tag_list, tag, |_| Default::default())
        };

        // Determine whether we became dirty
        let is_dirty = entity_info.layout_tag_list != entity_info.virtual_tag_list;

        // Add the entity to the dirty list if it became dirty.
        //
        // N.B. Yes, one could imagine a scenario where the entity becomes dirty, cleans itself, and
        // then becomes dirty again, allowing itself to be placed twice into the dirty list. This is
        // fine because the dirty list can support false positives and handling this rare case
        // properly would require additional metadata we really don't want to store.
        //
        // Finally, we fully ignore this logic if the entity's layout is empty because those layouts
        // will be transitioned to a non-empty layout on the first component insertion.
        if &entity_info.layout_tag_list != self.tag_list_map.root() && is_dirty && !was_dirty {
            self.dirty_entities.push(entity);
        }

        Ok(())
    }

    pub fn tag_entity(
        &mut self,
        entity: InertEntity,
        tag: InertTag,
    ) -> Result<(), EntityDeadError> {
        self.tag_common(entity, tag, true)
    }

    pub fn untag_entity(
        &mut self,
        entity: InertEntity,
        tag: InertTag,
    ) -> Result<(), EntityDeadError> {
        self.tag_common(entity, tag, false)
    }

    pub fn is_entity_tagged(
        &self,
        entity: InertEntity,
        tag: InertTag,
    ) -> Result<bool, EntityDeadError> {
        let Some(entity_info) = self.alive_entities.get(&entity) else {
			return Err(EntityDeadError);
        };

        Ok(self
            .tag_list_map
            .arena()
            .get(&entity_info.virtual_tag_list)
            .has_key(&tag))
    }

    pub fn get_storage<T: 'static>(&mut self) -> &'static DbStorage<T> {
        self.storages
            .entry(TypeId::of::<T>())
            .or_insert_with(|| {
                leak(DbStorage::new_full(DbStorageInner::<T> {
                    anon_block_alloc: BlockAllocator::default(),
                    archetypes: FxHashMap::default(),
                    mappings: NopHashMap::default(),
                }))
            })
            .downcast_ref()
            .unwrap()
    }

    pub fn insert_component<T: 'static>(
        &mut self,
        token: &MainThreadToken,
        storage: &mut DbStorageInner<T>,
        entity: InertEntity,
        value: T,
    ) -> Result<(Option<T>, Slot<T>), EntityDeadError> {
        // Ensure that the entity is alive.
        let Some(entity_info) = self.alive_entities.get_mut(&entity) else {
            return Err(EntityDeadError);
        };

        // If the entity is still in its empty layout and its virtual layout is not empty, transition
        // it to its virtual layout.
        if &entity_info.layout_tag_list == self.tag_list_map.root()
            && &entity_info.virtual_tag_list != self.tag_list_map.root()
        {
            // Update the marked layout
            entity_info.layout_tag_list = entity_info.virtual_tag_list;

            // Assign ourselves a slot in this layout
            let tag_list_info = self
                .tag_list_map
                .arena_mut()
                .get_mut(&entity_info.layout_tag_list)
                .value_mut();

            // First, ensure that we have the capacity for it.
            let last_heap_capacity = tag_list_info
                .entity_heaps
                .last()
                .map_or(0, |heap| heap.len());

            if tag_list_info.last_heap_len == last_heap_capacity {
                tag_list_info
                    .entity_heaps
                    .push(Box::from_iter((0..128).map(|_| InertEntity::PLACEHOLDER)));
                tag_list_info.last_heap_len = 0;
            }

            // Then, give ourself a slot
            entity_info.heap_index = tag_list_info.entity_heaps.len() - 1;
            entity_info.slot_index = tag_list_info.last_heap_len;

            // And mark ourselves in the archetype
            tag_list_info.entity_heaps.last_mut().unwrap()[tag_list_info.last_heap_len] = entity;
            tag_list_info.last_heap_len += 1;
        }

        // Update the value
        match storage.mappings.entry(entity) {
            hashbrown::hash_map::Entry::Occupied(entry) => {
                // We're merely occupied so just mutate the component without any additional fuss.
                let entry = entry.get();
                let replaced = mem::replace(&mut *entry.target.borrow_mut(token), value);

                Ok((Some(replaced), entry.target))
            }
            hashbrown::hash_map::Entry::Vacant(entry) => {
                // Update the component list
                entity_info.comp_list = self.comp_list_map.lookup_extension(
                    Some(&entity_info.comp_list),
                    DbComponentType::of::<T>(),
                    |_| Default::default(),
                );

                // Allocate a slot for this object
                let (resv, slot) = if let Some(heaps) =
                    storage.archetypes.get_mut(&entity_info.layout_tag_list)
                {
                    // We need to allocate in the arena

                    // Determine our tag list
                    let tag_list_info = self
                        .tag_list_map
                        .arena()
                        .get(&entity_info.layout_tag_list)
                        .value();

                    // Ensure that we have enough heaps to process the request
                    let min_len = entity_info.heap_index + 1;
                    if heaps.len() < min_len {
                        heaps.extend(
                            (heaps.len()..min_len)
                                .map(|i| Heap::new(token, tag_list_info.entity_heaps[i].len())),
                        );
                    }

                    // Fetch our slot
                    let slot = heaps[entity_info.heap_index].slot(entity_info.slot_index);

                    // Write the value to the slot
                    slot.set_value_owner_pair(token, Some((entity.into_dangerous_entity(), value)));

                    let slot = slot.slot();
                    (DbEntityMappingHeap::External, slot)
                } else {
                    // We need to allocate an anonymous block
                    let resv = storage.anon_block_alloc.alloc(|sz| Heap::new(token, sz));
                    let slot = storage
                        .anon_block_alloc
                        .block_mut(&resv.block)
                        .slot(resv.slot);

                    // Write the value to the slot
                    slot.set_value_owner_pair(token, Some((entity.into_dangerous_entity(), value)));

                    let slot = slot.slot();
                    (DbEntityMappingHeap::Anonymous(resv), slot)
                };

                // Insert the entry
                entry.insert(DbEntityMapping {
                    target: slot,
                    heap: resv,
                });

                Ok((None, slot))
            }
        }
    }

    pub fn remove_component<T: 'static>(
        &mut self,
        token: &MainThreadToken,
        storage: &mut DbStorageInner<T>,
        entity: InertEntity,
    ) -> Result<Option<T>, EntityDeadError> {
        // (entity liveness checks are deferred until later)

        // Unlink the entity
        let Some(removed) = storage.mappings.remove(&entity) else {
			return if self.alive_entities.contains_key(&entity) {
				Ok(None)
			} else {
				Err(EntityDeadError)
			};
		};

        // Remove the value from the heap
        let removed_value = removed.target.set_value_owner_pair(token, None);

        // Remove the reservation in the heap's allocator.
        match removed.heap {
            DbEntityMappingHeap::Anonymous(resv) => storage.anon_block_alloc.dealloc(resv, drop),
            DbEntityMappingHeap::External => { /* (left blank) */ }
        }

        // If we actually removed something, update the component list. Otherwise, ignore it.
        //
        // We never actually check for entity liveness in this branch because entity teardown
        // unregisters the entity from the alive list before removing the components so we need
        // to support removing components from logically dead entities.
        if let Some(removed_value) = removed_value {
            if let Some(entity_info) = self.alive_entities.get_mut(&entity) {
                entity_info.comp_list = self.comp_list_map.lookup_de_extension(
                    &entity_info.comp_list,
                    DbComponentType::of::<T>(),
                    |_| Default::default(),
                );
            }

            Ok(Some(removed_value))
        } else {
            Ok(None)
        }
    }

    pub fn get_component<T: 'static>(
        storage: &DbStorageInner<T>,
        entity: InertEntity,
    ) -> Option<Slot<T>> {
        storage.mappings.get(&entity).map(|mapping| mapping.target)
    }

    pub fn debug_total_spawns(&self) -> u64 {
        self.debug_total_spawns
    }

    pub fn debug_alive_list(&self) -> impl ExactSizeIterator<Item = InertEntity> + '_ {
        self.alive_entities.keys().copied()
    }

    pub fn debug_format_entity(
        &mut self,
        f: &mut fmt::Formatter,
        token: &MainThreadToken,
        entity: InertEntity,
    ) -> fmt::Result {
        #[derive(Debug)]
        struct Id(NonZeroU64);

        if let Some(&entity_info) = self.alive_entities.get(&entity) {
            // Format the component list
            let mut builder = f.debug_tuple("Entity");

            if let Some(label) =
                Self::get_component(&self.get_storage::<DebugLabel>().borrow(token), entity)
            {
                builder.field(&label.borrow(token));
            }

            if entity == InertEntity::PLACEHOLDER {
                builder.field(&RawFmt("<possibly a placeholder>"));
            }

            builder.field(&Id(entity.0));

            for v in entity_info.comp_list.direct_borrow().keys().iter() {
                if v.id != TypeId::of::<DebugLabel>() {
                    builder.field(&RawFmt(v.name));
                }
            }

            builder.finish()
        } else {
            f.debug_tuple("Entity")
                .field(&RawFmt("<dead>"))
                .field(&Id(entity.0))
                .finish()
        }
    }
}

#[derive(Debug)]
pub struct EntityDeadError;
