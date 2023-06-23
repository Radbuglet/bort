use std::{
    any::{type_name, Any},
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
    entity::Entity,
    query::{RawTag, VirtualTagMarker},
    util::{
        arena::{FreeListArena, LeakyArena, SpecArena},
        block::{BlockAllocator, BlockReservation},
        hash_map::{FxHashMap, FxHashSet, NopHashMap},
        misc::{const_new_nz_u64, leak, xorshift64, AnyDowncastExt, NamedTypeId, RawFmt},
        set_map::{SetMap, SetMapPtr},
    },
};

// === Root === //

#[derive(Debug)]
pub struct DbRoot {
    // The last unique ID to have been generated.
    uid_gen: NonZeroU64,

    // A map from alive entity ID to its state.
    alive_entities: NopHashMap<InertEntity, DbEntity>,

    // A set map keeping track of all component lists present in our application.
    comp_list_map: DbComponentListMap,

    // A set map keeping track of all tag lists present in our application.
    tag_list_map: DbTagListMap,

    // A map from type ID to storage.
    storages: FxHashMap<NamedTypeId, &'static dyn DbAnyStorage>,

    // A list of entities which may need to be moved around before running the next query. May contain
    // false positives, duplicates, and even dead entities. Never contains false negatives.
    probably_alive_dirty_entities: Vec<InertEntity>,

    // An extension to `probably_alive_dirty_entities` but only contains dead entities and the
    // necessary metadata to move them around.
    dead_dirty_entities: Vec<DbDirtyDeadEntity>,

    // The total number of entities ever created by the application.
    debug_total_spawns: u64,
}

#[derive(Debug, Copy, Clone)]
struct DbEntity {
    // The complete list of components attached to this entity.
    comp_list: DbComponentListRef,

    // The complete list of tags the user wants attached to this entity.
    virtual_tag_list: DbTagListRef,

    // The tag list which is currently being used to lay components out.
    //
    // All components managed by this layout must either adhere to it or be missing from the entity
    // entirely.
    layout_tag_list: DbTagListRef,

    // The heap containing the entity given its current tag layout.
    heap_index: usize,

    // The slot containing the entity given its current tag layout.
    slot_index: usize,
}

#[derive(Debug, Copy, Clone)]
struct DbDirtyDeadEntity {
    entity: InertEntity,
    layout_tag_list: DbTagListRef,
    heap_index: usize,
    slot_index: usize,
}

// === Storage === //

pub trait DbAnyStorage: fmt::Debug + Sync {
    fn as_any(&self) -> &(dyn Any + Sync);
}

pub type DbStorage<T> = NOptRefCell<DbStorageInner<T>>;

#[derive_where(Debug)]
pub struct DbStorageInner<T: 'static> {
    anon_block_alloc: BlockAllocator<Heap<T>>,
    mappings: NopHashMap<InertEntity, DbEntityMapping<T>>,
}

impl<T: 'static> DbAnyStorage for DbStorage<T> {
    fn as_any(&self) -> &(dyn Any + Sync) {
        self
    }
}

struct DbEntityMapping<T: 'static> {
    target: Slot<T>,
    heap: DbEntityMappingHeap<T>,
}

impl<T> fmt::Debug for DbEntityMapping<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DbEntityMapping")
            .field("target", &self.target)
            .finish_non_exhaustive()
    }
}

enum DbEntityMappingHeap<T: 'static> {
    Anonymous(BlockReservation<Heap<T>>),
    External,
}

// === ComponentList === //

#[derive(Copy, Clone)]
struct DbComponentType {
    pub id: NamedTypeId,
    pub name: &'static str,
    pub dtor: fn(&MainThreadToken, &mut DbRoot, InertEntity),
}

impl fmt::Debug for DbComponentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DbComponentType")
            .field("id", &self.id)
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl DbComponentType {
    fn of<T: 'static>() -> Self {
        fn dtor<T: 'static>(token: &MainThreadToken, db: &mut DbRoot, entity: InertEntity) {
            let storage = db.get_storage::<T>();
            let comp = db.remove_component(token, &mut storage.borrow_mut(token), entity);
            debug_assert!(comp.is_ok());
        }

        Self {
            id: NamedTypeId::of::<T>(),
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

#[derive(Debug)]
struct DbTagList {
    managed: FxHashSet<NamedTypeId>,
    entity_heaps: Vec<Box<[InertEntity]>>,
    last_heap_len: usize,
}

impl DbTagList {
    fn new(tags: &[InertTag]) -> Self {
        Self {
            managed: FxHashSet::from_iter(tags.iter().filter_map(|tag| {
                (tag.ty != NamedTypeId::of::<VirtualTagMarker>()).then_some(tag.ty)
            })),
            entity_heaps: Vec::new(),
            last_heap_len: 0,
        }
    }
}

type DbTagListMap = SetMap<InertTag, DbTagList, FreeListArena>;
type DbTagListRef = SetMapPtr<InertTag, DbTagList, FreeListArena>;

// === Inert Handles === //

// N.B. it is all too easy to accidentally call `Entity`s debug handler while issuing an error,
// causing a borrow error when the database is reborrowed by the debug handler. Hence, we work
// entirely with inert objects at this layer and add all the useful but not-necessarily-idiomatic
// features at a higher layer.

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct InertEntity(NonZeroU64);

impl InertEntity {
    pub const PLACEHOLDER: Self = Self(const_new_nz_u64(u64::MAX));

    pub const fn into_dangerous_entity(self) -> Entity {
        Entity { inert: self }
    }

    pub fn id(self) -> NonZeroU64 {
        self.0
    }
}

#[derive(Debug, Copy, Clone)]
#[derive_where(Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct InertTag {
    id: NonZeroU64,
    #[derive_where(skip)]
    ty: NamedTypeId,
}

impl InertTag {
    pub fn into_dangerous_tag(self) -> RawTag {
        RawTag(self)
    }

    pub fn id(self) -> NonZeroU64 {
        self.id
    }

    pub fn ty(self) -> NamedTypeId {
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
                    tag_list_map: SetMap::new(DbTagList::new(&[])),
                    storages: FxHashMap::default(),
                    probably_alive_dirty_entities: Vec::new(),
                    dead_dirty_entities: Vec::new(),
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

    // === Entity management === //

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
        // Fetch the entity info
        let Some(entity_info) = self
            .alive_entities
            .remove(&entity)
		else {
			return Err(EntityDeadError);
		};

        // Mark this entity for cleanup if it's not in an empty layout.
        if &entity_info.layout_tag_list != self.tag_list_map.root() {
            self.dead_dirty_entities.push(DbDirtyDeadEntity {
                entity,
                layout_tag_list: entity_info.layout_tag_list,
                heap_index: entity_info.heap_index,
                slot_index: entity_info.slot_index,
            });
        }

        // Remove all of the object's components
        //
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

    pub fn spawn_tag(&mut self, ty: NamedTypeId) -> InertTag {
        InertTag {
            id: self.new_uid(),
            ty,
        }
    }

    // === Tag management === //

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
            self.tag_list_map.lookup_extension(
                Some(&entity_info.virtual_tag_list),
                tag,
                DbTagList::new,
            )
        } else {
            self.tag_list_map.lookup_de_extension(
                &entity_info.virtual_tag_list,
                tag,
                DbTagList::new,
            )
        };

        // Determine whether we became dirty
        let is_dirty = entity_info.layout_tag_list != entity_info.virtual_tag_list;

        // Add the entity to the dirty list if it became dirty. This may happen multiple times but
        // we don't really mind since this list can accept false positives.
        if is_dirty && !was_dirty {
            self.probably_alive_dirty_entities.push(entity);
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

    // === Queries === //

    pub fn flush_archetypes(&mut self, token: &MainThreadToken) {
        // Throughout this process, we keep track of which entities have been moved around and they
        // archetype they currently reside.
        let mut moved = NopHashMap::default();

        // Begin by removing dead entities
        'delete_dead: for info in mem::take(&mut self.dead_dirty_entities) {
            // N.B. we know this won't happen because we check for it before adding the entity to the
            // `dead_dirty_entities` list.
            debug_assert_ne!(info.layout_tag_list, *self.tag_list_map.root());

            // Determine the archetype we'll be working on.
            let archetype = self
                .tag_list_map
                .arena_mut()
                .get_mut(&info.layout_tag_list)
                .value_mut();

            // Determine the right candidate for the swap-remove.
            let (last_entity, last_entity_info) = {
                let Some(mut sub_heap) = archetype.entity_heaps.last() else {
					// There are no more heaps to work with because there are no more entities in this
					// archetype.
					continue 'delete_dead;
				};

                loop {
                    // Find a removal target from the back of the list.

                    // We know this index will succeed because we'll never have a trailing heap whose
                    // length is zero by invariant.
                    let last_entity = sub_heap[archetype.last_heap_len - 1];

                    // If this `last_entity` is alive, use it for the swap-remove.
                    if let Some(last_entity_info) = self.alive_entities.get_mut(&last_entity) {
                        break (last_entity, last_entity_info);
                    }

                    // Otherwise, remove it from the list. This action is fine because we're trying
                    // to get rid of these anyways and it is super dangerous to move these dead
                    // entities around.
                    archetype.last_heap_len -= 1;

                    if archetype.last_heap_len == 0 {
                        archetype.entity_heaps.pop();

                        let Some(new_sub_heap) = archetype.entity_heaps.last() else {
							// If we managed to consume all the entities in this archetype, we know
							// our target entity is already dead
							continue 'delete_dead;
						};

                        archetype.last_heap_len = new_sub_heap.len();
                        sub_heap = new_sub_heap;
                    }
                }
            };

            // Determine whether our target entity is still in the archetype.
            let replace_target = archetype
                .entity_heaps
                .get_mut(info.heap_index)
                .and_then(|heap| heap.get_mut(info.slot_index));

            if let Some(replace_target) = replace_target {
                // If it is, commit the swap-replace. Otherwise, ignore everything that went on here.

                // The only way for dead entities to be moved around is if they were removed by the
                // swap-remove pruning logic above.
                debug_assert_eq!(*replace_target, info.entity);

                // Mark the swap-remove "filler" as moved
                moved.insert(last_entity, info.layout_tag_list);

                // Replace the slot
                *replace_target = last_entity;
                last_entity_info.heap_index = info.heap_index;
                last_entity_info.slot_index = info.slot_index;

                // Pop from the list.
                archetype.last_heap_len -= 1;

                if archetype.last_heap_len == 0 {
                    archetype.entity_heaps.pop();

                    if let Some(new_last) = archetype.entity_heaps.last() {
                        archetype.last_heap_len = new_last.len();
                    }
                }
            }
        }

        // Now, move around the alive entities.
        for entity in mem::take(&mut self.probably_alive_dirty_entities) {
            // First, ensure that the entity is still alive and dirty since, although we've deleted
            // all dead entities from the heap, we may still have dead entities in our queue.
            let Some(entity_info) = self.alive_entities.get_mut(&entity) else {
				continue
			};

            if entity_info.layout_tag_list == entity_info.virtual_tag_list {
                continue;
            }

            // Now, swap-remove the entity from its source archetype.
            let src_arch_tag_list = entity_info.layout_tag_list;

            // The root archetype doesn't manage any heaps so we ignore transitions to it.
            if src_arch_tag_list != *self.tag_list_map.root() {
                let arch = self
                    .tag_list_map
                    .arena_mut()
                    .get_mut(&src_arch_tag_list)
                    .value_mut();

                // Determine the filler entity
                let last_entity = arch
                    .entity_heaps
                    .last_mut()
                    // This unwrap is guaranteed to succeed because at least one entity (our `entity`)
                    // which is known to be alive. Additionally, we know this entity will be alive
                    // because every archetype has already been cleared of its dead entities.
                    .unwrap()[arch.last_heap_len - 1];

                // Replace the slot
                arch.entity_heaps[entity_info.heap_index][entity_info.slot_index] = last_entity;

                // Update the filler's location mirror
                let entity_info = *entity_info;
                let last_entity_info = self.alive_entities.get_mut(&entity).unwrap();
                last_entity_info.heap_index = entity_info.heap_index;
                last_entity_info.slot_index = entity_info.slot_index;

                // Mark the swap-remove "filler" as moved
                moved.insert(last_entity, src_arch_tag_list);

                // Pop the end
                arch.last_heap_len -= 1;

                if arch.last_heap_len == 0 {
                    arch.entity_heaps.pop();

                    if let Some(new_last) = arch.entity_heaps.last() {
                        arch.last_heap_len = new_last.len();
                    }
                }
            }

            // ...and push it to the back of its target archetype.
            let entity_info = self.alive_entities.get_mut(&entity).unwrap();
            let dest_arch_tag_list = entity_info.virtual_tag_list;

            // The root archetype doesn't manage any heaps so we ignore transitions to it.
            if dest_arch_tag_list != *self.tag_list_map.root() {
                // Add to the target archetype list
                let arch = self
                    .tag_list_map
                    .arena_mut()
                    .get_mut(&dest_arch_tag_list)
                    .value_mut();

                if arch.last_heap_len == arch.entity_heaps.last().map_or(0, |heap| heap.len()) {
                    let mut sub_heap = Box::from_iter((0..128).map(|_| InertEntity::PLACEHOLDER));
                    sub_heap[0] = entity;

                    arch.entity_heaps.push(sub_heap);
                    arch.last_heap_len = 1;
                } else {
                    arch.entity_heaps.last_mut().unwrap()[arch.last_heap_len] = entity;
                    arch.last_heap_len += 1;
                }

                // Update the entity info
                entity_info.heap_index = arch.entity_heaps.len() - 1;
                entity_info.slot_index = arch.last_heap_len - 1;
            }

            // Regardless of whether we actually moved into a heap, we still have to update our layout
            // and mark ourselves as moved so storages can properly update their mapping to an
            // anonymous one.
            entity_info.layout_tag_list = entity_info.virtual_tag_list;
            moved.insert(entity, dest_arch_tag_list);
        }

        // Now that archetype locations are settled, we can handle actual memory swaps, moving
        // entities into their target slots.
        println!("Moved: {:?}", moved);

        // TODO!!!
    }

    // === Storage management === //

    pub fn get_storage<T: 'static>(&mut self) -> &'static DbStorage<T> {
        self.storages
            .entry(NamedTypeId::of::<T>())
            .or_insert_with(|| {
                leak(DbStorage::new_full(DbStorageInner::<T> {
                    anon_block_alloc: BlockAllocator::default(),
                    mappings: NopHashMap::default(),
                }))
            })
            .as_any()
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
                let resv = storage.anon_block_alloc.alloc(|sz| Heap::new(token, sz));
                let slot = storage
                    .anon_block_alloc
                    .block_mut(&resv.block)
                    .slot(resv.slot);

                // Write the value to the slot
                slot.set_value_owner_pair(token, Some((entity.into_dangerous_entity(), value)));

                // Insert the entry
                let slot = slot.slot();
                entry.insert(DbEntityMapping {
                    target: slot,
                    heap: DbEntityMappingHeap::Anonymous(resv),
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

    // === Debug === //

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
                if v.id != NamedTypeId::of::<DebugLabel>() {
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
