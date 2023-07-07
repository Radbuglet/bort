use std::{
    any::{type_name, Any},
    fmt, hash, mem,
    num::NonZeroU64,
    sync::Arc,
};

use derive_where::derive_where;

use crate::{
    core::{
        cell::{OptRef, OptRefMut},
        heap::{Heap, Slot},
        token::MainThreadToken,
        token_cell::{NMainCell, NOptRefCell},
    },
    debug::DebugLabel,
    entity::Entity,
    query::{RawTag, VirtualTagMarker},
    util::{
        arena::{FreeListArena, LeakyArena, SpecArena},
        block::{BlockAllocator, BlockReservation},
        hash_map::{FxHashMap, FxHashSet, NopHashMap},
        iter::{filter_duplicates, merge_iters},
        misc::{
            const_new_nz_u64, leak, xorshift64, AnyDowncastExt, ListFmt, MapFmt, NamedTypeId,
            RawFmt,
        },
        set_map::{SetMap, SetMapArena, SetMapPtr},
    },
};

// === Helpers === //

const POSSIBLY_A_PLACEHOLDER: RawFmt = RawFmt("<possibly a placeholder>");

// === Root === //

#[derive(Debug)]
pub struct DbRoot {
    // The last unique ID to have been generated.
    uid_gen: NonZeroU64,

    // A map from alive entity ID to its state.
    alive_entities: NopHashMap<InertEntity, DbEntity>,

    // A set map keeping track of all component lists present in our application.
    comp_list_map: DbComponentListMap,

    // A set map keeping track of all archetypes present in our application.
    arch_map: DbArchetypeMap,

    // A map from tag to metadata.
    tag_map: NopHashMap<InertTag, DbTag>,

    // A map from type ID to storage.
    storages: FxHashMap<NamedTypeId, &'static dyn DbAnyStorage>,

    // A map from type ID to event.
    events: FxHashMap<NamedTypeId, &'static dyn DbAnyEventSet>,

    // A list of entities which may need to be moved around before running the next query. May contain
    // false positives, duplicates, and even dead entities. Never contains false negatives.
    probably_alive_dirty_entities: Vec<InertEntity>,

    // An extension to `probably_alive_dirty_entities` but only contains dead entities and the
    // necessary metadata to move them around.
    dead_dirty_entities: Vec<DbDirtyDeadEntity>,

    // The total number of entities ever created by the application.
    debug_total_spawns: u64,

    // A guard to protect against flushing while querying. This doesn't prevent panics but it does
    // prevent nasty concurrent modification surprises.
    query_guard: &'static NOptRefCell<()>,
}

#[derive(Debug, Copy, Clone)]
struct DbEntity {
    // The complete list of components attached to this entity.
    comp_list: DbComponentListRef,

    // The complete list of tags the user wants attached to this entity.
    virtual_arch: DbArchetypeRef,

    // The archetype which is currently being used to lay components out.
    //
    // All components managed by this layout must either adhere to it or be missing from the entity
    // entirely.
    physical_arch: DbArchetypeRef,

    // The heap containing the entity given its current tag layout.
    heap_index: usize,

    // The slot containing the entity given its current tag layout.
    slot_index: usize,
}

#[derive(Debug, Copy, Clone)]
struct DbDirtyDeadEntity {
    entity: InertEntity,
    physical_arch: DbArchetypeRef,
    heap_index: usize,
    slot_index: usize,
}

#[derive(Debug, Default)]
struct DbTag {
    contained_by: FxHashSet<DbArchetypeRef>,
    sorted_containers: Vec<DbArchetypeRef>,
    are_sorted_containers_sorted: bool,
}

// === Storage === //

trait DbAnyStorage: fmt::Debug + Sync {
    fn as_any(&self) -> &(dyn Any + Sync);

    fn move_entity(
        &self,
        token: &'static MainThreadToken,
        entity: InertEntity,
        entity_info: &DbEntity,
        src_arch: DbArchetypeRef,
        dst_entry: &DbArchetype,
    );
}

pub type DbStorage<T> = NOptRefCell<DbStorageInner<T>>;

#[derive_where(Debug)]
pub struct DbStorageInner<T: 'static> {
    anon_block_alloc: BlockAllocator<Heap<T>>,
    mappings: NopHashMap<InertEntity, DbEntityMapping<T>>,
    heaps: FxHashMap<DbArchetypeRef, Vec<Arc<Heap<T>>>>,
}

struct DbEntityMapping<T: 'static> {
    slot: Slot<T>,
    heap: DbEntityMappingHeap<T>,
}

impl<T> fmt::Debug for DbEntityMapping<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DbEntityMapping")
            .field("target", &self.slot)
            .finish_non_exhaustive()
    }
}

enum DbEntityMappingHeap<T: 'static> {
    Anonymous(BlockReservation<Heap<T>>),
    External { heap: usize, slot: usize },
}

// === ComponentList === //

#[derive(Copy, Clone)]
struct DbComponentType {
    pub id: NamedTypeId,
    pub name: &'static str,
    pub dtor: fn(&'static MainThreadToken, &mut DbRoot, InertEntity),
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
        fn dtor<T: 'static>(token: &'static MainThreadToken, db: &mut DbRoot, entity: InertEntity) {
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
struct DbArchetype {
    managed: FxHashSet<NamedTypeId>,
    entity_heaps: Vec<Arc<[NMainCell<InertEntity>]>>,
    last_heap_len: usize,
}

impl DbArchetype {
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

type DbArchetypeMap = SetMap<InertTag, DbArchetype, FreeListArena>;
type DbArchetypeArena = SetMapArena<InertTag, DbArchetype, FreeListArena>;
type DbArchetypeRef = SetMapPtr<InertTag, DbArchetype, FreeListArena>;

// === Event === //

trait DbAnyEventSet: fmt::Debug + Sync {
    fn as_any(&self) -> &(dyn Any + Sync);

    fn clear(&self, token: &MainThreadToken);
}

pub type DbEventSet<T> = NOptRefCell<DbEventSetInner<T>>;

type DbEventSetGroup<T> = FxHashMap<DbArchetypeRef, Vec<(InertEntity, T)>>;

#[derive_where(Default)]
pub struct DbEventSetInner<T> {
    current_events: DbEventSetGroup<T>,
    old_events: Vec<Arc<DbEventSetGroup<T>>>,
}

impl<T> fmt::Debug for DbEventSetInner<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DbEventInner")
            .field(
                "events",
                &MapFmt(self.current_events.iter().map(|(arch, values)| {
                    (arch, ListFmt(values.iter().map(|(entity, _)| entity)))
                })),
            )
            .finish_non_exhaustive()
    }
}

impl<T: 'static> DbAnyEventSet for DbEventSet<T> {
    fn as_any(&self) -> &(dyn Any + Sync) {
        self
    }

    fn clear(&self, token: &MainThreadToken) {
        let mut me = self.borrow_mut(token);
        me.current_events.clear();
        me.old_events.clear();
    }
}

// === Inert Handles === //

// N.B. it is all too easy to accidentally call `Entity`s debug handler while issuing an error,
// causing a borrow error when the database is reborrowed by the debug handler. Hence, we work
// entirely with inert objects at this layer and add all the useful but not-necessarily-idiomatic
// features at a higher layer.

#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct InertEntity(NonZeroU64);

impl fmt::Debug for InertEntity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut b = f.debug_tuple("InertEntity");
        b.field(&self.0);

        if self == &Self::PLACEHOLDER {
            b.field(&POSSIBLY_A_PLACEHOLDER);
        }

        b.finish()
    }
}

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
                    arch_map: SetMap::new(DbArchetype::new(&[])),
                    tag_map: NopHashMap::default(),
                    storages: FxHashMap::default(),
                    events: FxHashMap::default(),
                    probably_alive_dirty_entities: Vec::new(),
                    dead_dirty_entities: Vec::new(),
                    debug_total_spawns: 0,
                    query_guard: leak(NOptRefCell::new_full(())),
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
                virtual_arch: *self.arch_map.root(),
                physical_arch: *self.arch_map.root(),
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
        token: &'static MainThreadToken,
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
        if &entity_info.physical_arch != self.arch_map.root() {
            self.dead_dirty_entities.push(DbDirtyDeadEntity {
                entity,
                physical_arch: entity_info.physical_arch,
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
        let was_dirty = entity_info.physical_arch != entity_info.virtual_arch;

        // Update the list
        let post_ctor = |arena: &mut DbArchetypeArena, target_ptr: &DbArchetypeRef| {
            let target = arena.get(target_ptr);

            for tag in target.keys() {
                let tag_state = self.tag_map.entry(*tag).or_insert_with(Default::default);

                tag_state.contained_by.insert(*target_ptr);
                tag_state.sorted_containers.push(*target_ptr);
                tag_state.are_sorted_containers_sorted = false;
            }
        };

        entity_info.virtual_arch = if is_add {
            self.arch_map.lookup_extension(
                Some(&entity_info.virtual_arch),
                tag,
                DbArchetype::new,
                post_ctor,
            )
        } else {
            self.arch_map.lookup_de_extension(
                &entity_info.virtual_arch,
                tag,
                DbArchetype::new,
                post_ctor,
            )
        };

        // Determine whether we became dirty
        let is_dirty = entity_info.physical_arch != entity_info.virtual_arch;

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
            .arch_map
            .arena()
            .get(&entity_info.virtual_arch)
            .has_key(&tag))
    }

    // === Queries === //

    pub fn borrow_query_guard(&self, token: &'static MainThreadToken) -> OptRef<'static, ()> {
        self.query_guard.borrow(token)
    }

    fn prepare_query_common(
        &mut self,
        tags: TagList,
        mut f: impl FnMut(&mut DbArchetypeArena, DbArchetypeRef),
    ) {
        if tags.iter().next().is_none() {
            return;
        }

        // Ensure that all tag containers are sorted
        for tag_id in tags.iter() {
            let Some(tag) = self.tag_map.get_mut(tag_id) else { continue };
            if !tag.are_sorted_containers_sorted {
                tag.sorted_containers.sort();
                tag.are_sorted_containers_sorted = true;
            }
        }

        // Collect a set of archetypes to include and prepare their chunks
        let mut tag_iters = tags
            .iter()
            .map(|tag_id| {
                self.tag_map
                    .get(tag_id)
                    .map_or(&[][..], |tag| tag.sorted_containers.as_slice())
                    .iter()
            })
            .collect::<Vec<_>>();

        let mut primary_iter = tag_iters.pop().unwrap();

        'scan: loop {
            // Determine the primary archetype we'll be scanning for.
            let Some(primary_arch) = primary_iter.next() else { break 'scan };

            // Ensure that the archetype exists in all other tags
            for other_iter in &mut tag_iters {
                // Consume all archetypes less than primary_arch
                let other_arch = loop {
                    let Some(other_arch) = other_iter.as_slice().first() else { break 'scan };

                    if other_arch < primary_arch {
                        let _ = other_iter.next();
                    } else {
                        break other_arch;
                    }
                };

                // If `other_arch` is not equal to our searched-for `primary_arch`, try again.
                if primary_arch != other_arch {
                    continue 'scan;
                }
            }

            // Otherwise, this archetype is in the intersection and we can add a chunk for it.
            f(self.arch_map.arena_mut(), *primary_arch);
        }
    }

    pub fn prepare_named_entity_query(&mut self, tags: TagList) -> Vec<QueryChunkWithEntities> {
        let mut chunks = Vec::new();

        self.prepare_query_common(tags, |arena, arch_id| {
            let arch = arena.get(&arch_id).value();
            chunks.push(QueryChunkWithEntities {
                archetype: arch_id,
                entity_subs: arch.entity_heaps.clone(),
                last_heap_len: arch.last_heap_len,
            })
        });

        chunks
    }

    pub fn prepare_anonymous_entity_query(&mut self, tags: TagList) -> Vec<QueryChunk> {
        let mut chunks = Vec::new();

        self.prepare_query_common(tags, |arena, arch_id| {
            let arch = arena.get(&arch_id).value();
            chunks.push(QueryChunk {
                archetype: arch_id,
                heap_count: arch.entity_heaps.len(),
                last_heap_len: arch.last_heap_len,
            })
        });

        chunks
    }

    pub fn flush_archetypes(&mut self, token: &'static MainThreadToken) {
        let _guard = self
            .query_guard
            .try_borrow_mut(token)
            .expect("cannot flush archetypes while a query is active");

        // Clear all events
        for event in self.events.values() {
            event.clear(token);
        }

        // Throughout this process, we keep track of which entities have been moved around and they
        // archetype they currently reside.
        let mut moved = NopHashMap::default();

        #[derive(Debug, Copy, Clone)]
        struct Moved {
            src: DbArchetypeRef,
            dst: DbArchetypeRef,
        }

        // Begin by removing dead entities
        'delete_dead: for info in mem::take(&mut self.dead_dirty_entities) {
            // N.B. we know this won't happen because we check for it before adding the entity to the
            // `dead_dirty_entities` list.
            debug_assert_ne!(info.physical_arch, *self.arch_map.root());

            // Determine the archetype we'll be working on.
            let archetype = self
                .arch_map
                .arena_mut()
                .get_mut(&info.physical_arch)
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
                    let last_entity = sub_heap[archetype.last_heap_len - 1].get(token);

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
                .and_then(|heap| heap.get(info.slot_index));

            if let Some(replace_target) = replace_target {
                // If it is, commit the swap-replace. Otherwise, ignore everything that went on here.

                // The only way for dead entities to be moved around is if they were removed by the
                // swap-remove pruning logic above.
                debug_assert_eq!(replace_target.get(token), info.entity);

                // Mark the swap-remove "filler" as moved
                moved.insert(
                    last_entity,
                    Moved {
                        // We know this entity came from this tag because we haven't moved entities
                        // around in archetypes yet.
                        src: info.physical_arch,
                        dst: info.physical_arch,
                    },
                );

                // Replace the slot
                replace_target.set(token, last_entity);
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

            if entity_info.physical_arch == entity_info.virtual_arch {
                continue;
            }

            // Now, swap-remove the entity from its source archetype.
            let src_arch_id = entity_info.physical_arch;

            // The root archetype doesn't manage any heaps so we ignore transitions to it.
            if src_arch_id != *self.arch_map.root() {
                let arch = self.arch_map.arena_mut().get_mut(&src_arch_id).value_mut();

                // Determine the filler entity
                let last_entity = arch
                    .entity_heaps
                    .last_mut()
                    // This unwrap is guaranteed to succeed because at least one entity (our `entity`)
                    // which is known to be alive. Additionally, we know this entity will be alive
                    // because every archetype has already been cleared of its dead entities.
                    .unwrap()[arch.last_heap_len - 1]
                    .get(token);

                // Replace the slot
                arch.entity_heaps[entity_info.heap_index][entity_info.slot_index]
                    .set(token, last_entity);

                // Update the filler's location mirror
                let entity_info = *entity_info;
                let last_entity_info = self.alive_entities.get_mut(&entity).unwrap();
                last_entity_info.heap_index = entity_info.heap_index;
                last_entity_info.slot_index = entity_info.slot_index;

                // Mark the swap-remove "filler" as moved. We only insert this entry if it hasn't
                // already been marked as moving before since a) we wouldn't be updating the
                // destination field in any useful way and b) we could clobber the src field.
                if let hashbrown::hash_map::Entry::Vacant(entry) = moved.entry(last_entity) {
                    entry.insert(Moved {
                        src: src_arch_id,
                        dst: src_arch_id,
                    });
                }

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
            let dest_arch_id = entity_info.virtual_arch;

            // The root archetype doesn't manage any heaps so we ignore transitions to it.
            if dest_arch_id != *self.arch_map.root() {
                // Add to the target archetype list
                let arch = self.arch_map.arena_mut().get_mut(&dest_arch_id).value_mut();

                if arch.last_heap_len == arch.entity_heaps.last().map_or(0, |heap| heap.len()) {
                    let sub_heap =
                        Arc::from_iter((0..128).map(|_| NMainCell::new(InertEntity::PLACEHOLDER)));
                    sub_heap[0].set(token, entity);

                    arch.entity_heaps.push(sub_heap);
                    arch.last_heap_len = 1;
                } else {
                    arch.entity_heaps.last_mut().unwrap()[arch.last_heap_len].set(token, entity);
                    arch.last_heap_len += 1;
                }

                // Update the entity info
                entity_info.heap_index = arch.entity_heaps.len() - 1;
                entity_info.slot_index = arch.last_heap_len - 1;
            }

            // Regardless of whether we actually moved into a heap, we still have to update our layout
            // and mark ourselves as moved so storages can properly update their mapping to an
            // anonymous one.
            entity_info.physical_arch = entity_info.virtual_arch;
            moved.insert(
                entity,
                Moved {
                    // We know this entity came from this tag because non-finalized entities are only
                    // moved into other archetypes when it's their turn to be processed.
                    src: src_arch_id,
                    dst: dest_arch_id,
                },
            );
        }

        // Finally, update storages to reflect this new template.
        for (entity, Moved { src, dst }) in moved {
            let entity_info = &self.alive_entities[&entity];

            let src_entry = self.arch_map.arena().get(&src);
            let dst_entry = self.arch_map.arena().get(&dst);

            // For every updated entity, determine the list of components which need to be updated.
            // This is just the union of `src` and `dst`.
            //
            // N.B. Yes, this could have duplicates if multiple tags decide to manage the same
            // component. This is fine for safety as storages can handle no-op move requests and
            // shouldn't affect performance too much since users typically won't be reusing the same
            // type in multiple tags.
            for managed_ty in filter_duplicates(merge_iters(
                src_entry.keys().iter().map(|key| key.ty),
                dst_entry.keys().iter().map(|key| key.ty),
            )) {
                // Now, we just have to notify the storage for it to take appropriate action.

                let Some(storage) = self.storages.get(&managed_ty) else {
					// If this fails, it merely means that we never attached this managed type to this
					// entity.
					continue;
				};

                storage.move_entity(token, entity, entity_info, src, dst_entry.value());
            }
        }
    }

    // === Storage management === //

    pub fn get_storage<T: 'static>(&mut self) -> &'static DbStorage<T> {
        self.storages
            .entry(NamedTypeId::of::<T>())
            .or_insert_with(|| {
                leak(DbStorage::new_full(DbStorageInner::<T> {
                    anon_block_alloc: BlockAllocator::default(),
                    mappings: NopHashMap::default(),
                    heaps: FxHashMap::default(),
                }))
            })
            .as_any()
            .downcast_ref()
            .unwrap()
    }

    pub fn insert_component<T: 'static>(
        &mut self,
        token: &'static MainThreadToken,
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
                let replaced = mem::replace(&mut *entry.slot.borrow_mut(token), value);

                Ok((Some(replaced), entry.slot))
            }
            hashbrown::hash_map::Entry::Vacant(entry) => {
                // Update the component list
                entity_info.comp_list = self.comp_list_map.lookup_extension(
                    Some(&entity_info.comp_list),
                    DbComponentType::of::<T>(),
                    |_| Default::default(),
                    |_, _| {},
                );

                // Allocate a slot for this component
                let external_heaps = match storage.heaps.entry(entity_info.physical_arch) {
                    hashbrown::hash_map::Entry::Occupied(entry) => Some(entry.into_mut()),
                    hashbrown::hash_map::Entry::Vacant(entry) => self
                        .arch_map
                        .arena()
                        .get(&entity_info.physical_arch)
                        .value()
                        .managed
                        .contains(&NamedTypeId::of::<T>())
                        .then(|| entry.insert(Vec::new())),
                };

                let (resv, slot) = if let Some(external_heaps) = external_heaps {
                    // Ensure that we have the appropriate slot for this entity
                    let min_heaps_len = entity_info.heap_index + 1;
                    if external_heaps.len() < min_heaps_len {
                        let arch = self
                            .arch_map
                            .arena()
                            .get(&entity_info.physical_arch)
                            .value();

                        external_heaps.extend(
                            (external_heaps.len()..min_heaps_len)
                                .map(|i| Arc::new(Heap::new(token, arch.entity_heaps[i].len()))),
                        );
                    }

                    // Write the value to the slot
                    let slot =
                        external_heaps[entity_info.heap_index].slot(token, entity_info.slot_index);
                    slot.set_value_owner_pair(token, Some((entity.into_dangerous_entity(), value)));

                    (
                        DbEntityMappingHeap::External {
                            heap: entity_info.heap_index,
                            slot: entity_info.slot_index,
                        },
                        slot.slot(),
                    )
                } else {
                    // Allocate a slot for this object
                    let resv = storage.anon_block_alloc.alloc(|sz| Heap::new(token, sz));
                    let slot = storage
                        .anon_block_alloc
                        .block_mut(&resv.block)
                        .slot(token, resv.slot);

                    // Write the value to the slot
                    slot.set_value_owner_pair(token, Some((entity.into_dangerous_entity(), value)));

                    let slot = slot.slot();
                    (DbEntityMappingHeap::Anonymous(resv), slot)
                };

                // Insert the mapping
                entry.insert(DbEntityMapping { slot, heap: resv });

                Ok((None, slot))
            }
        }
    }

    pub fn remove_component<T: 'static>(
        &mut self,
        token: &'static MainThreadToken,
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
        let removed_value = removed.slot.set_value_owner_pair(token, None);

        // Remove the reservation in the heap's allocator.
        match removed.heap {
            DbEntityMappingHeap::Anonymous(resv) => storage.anon_block_alloc.dealloc(resv, drop),
            DbEntityMappingHeap::External { .. } => { /* (left blank) */ }
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
                    |_, _| {},
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
        storage.mappings.get(&entity).map(|mapping| mapping.slot)
    }

    // === Event management === //

    pub fn get_event_set<T: 'static>(&mut self) -> &'static DbEventSet<T> {
        self.events
            .entry(NamedTypeId::of::<T>())
            .or_insert_with(|| leak(DbEventSet::<T>::default()))
            .as_any()
            .downcast_ref()
            .unwrap()
    }

    pub fn fire_event<T: 'static>(
        &self,
        event_set: &mut DbEventSetInner<T>,
        entity: InertEntity,
        value: T,
    ) -> Result<(), EntityDeadError> {
        let Some(entity_info) = self.alive_entities.get(&entity) else {
			return Err(EntityDeadError);
		};

        event_set
            .current_events
            .entry(entity_info.physical_arch)
            .or_default()
            .push((entity, value));

        Ok(())
    }

    pub fn get_event_snapshot<T: 'static>(
        event_set: &mut DbEventSetInner<T>,
    ) -> EventSetSnapshot<T> {
        let current_events = mem::take(&mut event_set.current_events);
        if !current_events.is_empty() {
            event_set.old_events.push(Arc::new(current_events));
        }

        EventSetSnapshot {
            runs: event_set.old_events.clone(),
            inclusion_cache: FxHashMap::default(),
        }
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
        token: &'static MainThreadToken,
        entity: InertEntity,
    ) -> fmt::Result {
        #[derive(Debug)]
        struct Id(NonZeroU64);

        let mut builder = f.debug_tuple("Entity");

        builder.field(&Id(entity.0));

        if entity == InertEntity::PLACEHOLDER {
            builder.field(&POSSIBLY_A_PLACEHOLDER);
        }

        if let Some(&entity_info) = self.alive_entities.get(&entity) {
            // Format the component list
            if let Some(label) =
                Self::get_component(&self.get_storage::<DebugLabel>().borrow(token), entity)
            {
                builder.field(&label.borrow(token));
            }

            for v in entity_info.comp_list.direct_borrow().keys().iter() {
                if v.id != NamedTypeId::of::<DebugLabel>() {
                    builder.field(&RawFmt(v.name));
                }
            }
        } else {
            builder.field(&RawFmt("<dead>"));
        }

        builder.finish()
    }
}

impl<T: 'static> DbAnyStorage for DbStorage<T> {
    fn as_any(&self) -> &(dyn Any + Sync) {
        self
    }

    fn move_entity(
        &self,
        token: &'static MainThreadToken,
        entity: InertEntity,
        entity_info: &DbEntity,
        src_arch: DbArchetypeRef,
        dst_arch: &DbArchetype,
    ) {
        let storage = &mut *self.borrow_mut(token);

        let Some(mapping) = storage.mappings.get_mut(&entity) else {
			// This may fail if a user never inserted a component for every managed type.
			return;
		};

        // Determine whether the new layout requires a managed allocation
        let external_heaps = match storage.heaps.entry(entity_info.physical_arch) {
            hashbrown::hash_map::Entry::Occupied(entry) => Some(entry.into_mut()),
            hashbrown::hash_map::Entry::Vacant(entry) => dst_arch
                .managed
                .contains(&NamedTypeId::of::<T>())
                .then(|| entry.insert(Vec::new())),
        };

        if let Some(external_heaps) = external_heaps {
            // Ensure that we have the appropriate slot for this entity
            let min_heaps_len = entity_info.heap_index + 1;
            if external_heaps.len() < min_heaps_len {
                external_heaps.extend(
                    (external_heaps.len()..min_heaps_len)
                        .map(|i| Arc::new(Heap::new(token, dst_arch.entity_heaps[i].len()))),
                );
            }

            // Swap value from the old slot. Deallocate the old anonymous reservation if applicable
            // and mark it as external
            let external_heaps = &storage.heaps[&entity_info.physical_arch];
            let target_heap = &external_heaps[entity_info.heap_index];

            match mem::replace(
                &mut mapping.heap,
                DbEntityMappingHeap::External {
                    heap: entity_info.heap_index,
                    slot: entity_info.slot_index,
                },
            ) {
                DbEntityMappingHeap::Anonymous(resv) => {
                    storage.anon_block_alloc.block(&resv.block).swap_slots(
                        token,
                        resv.slot,
                        target_heap,
                        entity_info.slot_index,
                    );
                    storage.anon_block_alloc.dealloc(resv, drop);
                }
                DbEntityMappingHeap::External {
                    heap: old_heap,
                    slot: old_slot,
                } => {
                    storage.heaps[&src_arch][old_heap].swap_slots(
                        token,
                        old_slot,
                        target_heap,
                        entity_info.slot_index,
                    );
                }
            }
        } else {
            // Ensure that we're not already in an anonymous reservation
            if let DbEntityMappingHeap::External {
                heap: old_heap,
                slot: old_slot,
            } = &mapping.heap
            {
                // Allocate a slot for this object
                let resv = storage.anon_block_alloc.alloc(|sz| Heap::new(token, sz));
                let new_heap = storage.anon_block_alloc.block_mut(&resv.block);

                // Swap the values to move the other object into its appropriate heap
                storage.heaps[&src_arch][*old_heap]
                    .swap_slots(token, *old_slot, new_heap, resv.slot);

                // Mark the heap as anonymous
                mapping.heap = DbEntityMappingHeap::Anonymous(resv);
            }
        }
    }
}

// === Public helpers === //

#[derive(Debug)]
pub struct EntityDeadError;

#[derive(Debug, Clone)]
pub struct QueryChunkWithEntities {
    archetype: DbArchetypeRef,
    entity_subs: Vec<Arc<[NMainCell<InertEntity>]>>,
    last_heap_len: usize,
}

impl QueryChunkWithEntities {
    pub fn split(self) -> (QueryChunk, Vec<Arc<[NMainCell<InertEntity>]>>) {
        (self.query_chunk(), self.into_entities())
    }

    pub fn query_chunk(&self) -> QueryChunk {
        QueryChunk {
            archetype: self.archetype,
            heap_count: self.entity_subs.len(),
            last_heap_len: self.last_heap_len,
        }
    }

    pub fn into_entities(self) -> Vec<Arc<[NMainCell<InertEntity>]>> {
        self.entity_subs
    }
}

#[derive(Debug, Clone)]
pub struct QueryChunk {
    archetype: DbArchetypeRef,
    heap_count: usize,
    last_heap_len: usize,
}

impl QueryChunk {
    pub fn is_empty(&self) -> bool {
        self.heap_count == 0
    }

    pub fn last_heap_len(&self) -> usize {
        self.last_heap_len
    }

    pub fn heap_count(&self) -> usize {
        self.heap_count
    }

    pub fn heaps<T: 'static>(&self, storage: &DbStorageInner<T>) -> Vec<Arc<Heap<T>>> {
        storage
            .heaps
            .get(&self.archetype)
            .map_or(Vec::new(), |v| v.clone())
    }
}

pub struct EventSetSnapshot<T> {
    runs: Vec<Arc<DbEventSetGroup<T>>>,
    inclusion_cache: FxHashMap<DbArchetypeRef, bool>,
}

impl<T> EventSetSnapshot<T> {
    pub fn set_archetypes(&mut self, db: &mut DbRoot, tag_list: TagList) {
        self.inclusion_cache.clear();

        for run in &self.runs {
            for arch in run.keys() {
                if self.inclusion_cache.contains_key(arch) {
                    continue;
                }

                let arch_data = db.arch_map.arena().get(arch);
                self.inclusion_cache
                    .insert(*arch, tag_list.iter().all(|tag| arch_data.has_key(tag)));
            }
        }
    }

    pub fn query(&self) -> impl Iterator<Item = (InertEntity, &T)> + '_ {
        self.runs.iter().flat_map(|run| {
            run.iter()
                .filter_map(|(arch, arch_data)| {
                    self.inclusion_cache
                        .get(arch)
                        .is_some_and(|&v| v)
                        .then(|| arch_data.iter().map(|(entity, value)| (*entity, value)))
                })
                .flatten()
        })
    }
}

#[derive(Debug, Copy, Clone)]
pub struct TagList<'a> {
    pub static_tags: &'a [Option<InertTag>],
    pub dynamic_tags: &'a [InertTag],
}

impl<'a> TagList<'a> {
    pub fn iter(self) -> impl Iterator<Item = &'a InertTag> + Clone + 'a {
        self.static_tags
            .iter()
            .filter_map(|v| v.as_ref())
            .chain(self.dynamic_tags.iter())
    }
}
