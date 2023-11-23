use std::{
    any::{type_name, Any, TypeId},
    cell::RefCell,
    fmt, hash,
    marker::PhantomData,
    mem,
    num::NonZeroU64,
    sync::{Arc, Mutex},
};

use autoken::PotentialMutableBorrow;
use derive_where::derive_where;
use hashbrown::hash_map::Entry as HmEntry;

use crate::{
    core::{
        cell::{OptRef, OptRefMut},
        heap::{Heap, Slot},
        token::{MainThreadToken, TrivialUnjailToken},
        token_cell::{NMainCell, NOptRefCell},
    },
    debug::DebugLabel,
    entity::Entity,
    query::RawTag,
    util::{
        arena::{FreeListArena, LeakyArena, SpecArena},
        block::{BlockAllocator, BlockReservation},
        hash_map::{ConstSafeBuildHasherDefault, FxHashMap, FxHashSet, NopHashMap},
        iter::{filter_duplicates, merge_iters},
        misc::{const_new_nz_u64, leak, unpoison, xorshift64, AnyDowncastExt, NamedTypeId, RawFmt},
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
    query_guard: &'static NOptRefCell<RecursiveQueryGuardTy>,
}

// This has its own type for the sake of autoken analysis.
#[derive(Debug)]
#[non_exhaustive]
pub struct RecursiveQueryGuardTy;

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
    sorted_containers: Vec<DbArchetypeRef>,
    are_sorted_containers_sorted: bool,
}

// === Storage === //

trait DbAnyStorage: fmt::Debug + Sync {
    fn as_any(&self) -> &(dyn Any + Sync);

    fn move_entity_into_empty_never_truncate(
        &self,
        token: &'static MainThreadToken,
        target: InertEntity,
        completed_target_info: &DbEntity,
        src_arch: DbArchetypeRef,
        dst_arch_info: &DbArchetype,
    );

    fn truncate_archetype_heap_len(
        &self,
        token: &'static MainThreadToken,
        arch: DbArchetypeRef,
        heap_count: usize,
    );

    fn contains_entity(&self, storage: &'static MainThreadToken, entity: InertEntity) -> bool;
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
    pub dtor: fn(PhantomData<ComponentDestructorMarker>, &'static MainThreadToken, InertEntity),
}

// For AuToken function analysis.
struct ComponentDestructorMarker;

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
        fn dtor<T: 'static>(
            _marker: PhantomData<ComponentDestructorMarker>,
            token: &'static MainThreadToken,
            entity: InertEntity,
        ) {
            let comp = {
                let mut db = DbRoot::get(token);
                let storage = db.get_storage::<T>(token);

                // FIXME: AuToken doesn't really know how to handle the interaction between option
                // pattern matching and its destructor and falsely reports that this function could
                // drop components. Fixing each individual instance of this would be pretty involved
                // so we're just ignoring the entire function for now and waiting for AuToken to fix
                // this issue.
                autoken::assume_black_box(|| {
                    db.remove_component(token, &mut storage.borrow_mut(token), entity)
                })
            };
            debug_assert!(comp.is_ok());
            drop(comp);
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
    tags: Box<[InertTag]>,
    managed: FxHashSet<NamedTypeId>,
    managed_sorted: Box<[NamedTypeId]>,
    entity_heaps: Vec<Arc<[NMainCell<InertEntity>]>>,
    last_heap_len: usize,
    virtual_count: u64,
}

impl DbArchetype {
    fn new(tags: &[InertTag]) -> Self {
        let mut managed_sorted = tags
            .iter()
            .filter_map(|tag| (tag.ty != InertTag::inert_ty_id()).then_some(tag.ty))
            .collect::<Box<[_]>>();

        managed_sorted.sort();

        Self {
            tags: Box::from_iter(tags.iter().copied()),
            managed: FxHashSet::from_iter(managed_sorted.iter().copied()),
            managed_sorted,
            entity_heaps: Vec::new(),
            last_heap_len: 0,
            virtual_count: 0,
        }
    }
}

type DbArchetypeMap = SetMap<InertTag, DbArchetype, FreeListArena>;
type DbArchetypeArena = SetMapArena<InertTag, DbArchetype, FreeListArena>;
type DbArchetypeRef = SetMapPtr<InertTag, DbArchetype, FreeListArena>;

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
    pub fn inert_ty_id() -> NamedTypeId {
        struct VirtualTagMarker;

        NamedTypeId::of::<VirtualTagMarker>()
    }

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

impl Default for DbRoot {
    fn default() -> Self {
        Self {
            uid_gen: NonZeroU64::new(1).unwrap(),
            alive_entities: NopHashMap::default(),
            comp_list_map: SetMap::default(),
            arch_map: SetMap::new(DbArchetype::new(&[])),
            tag_map: NopHashMap::default(),
            storages: FxHashMap::default(),
            probably_alive_dirty_entities: Vec::new(),
            dead_dirty_entities: Vec::new(),
            debug_total_spawns: 0,
            query_guard: leak(NOptRefCell::new_full(
                &TrivialUnjailToken,
                RecursiveQueryGuardTy,
            )),
        }
    }
}

impl DbRoot {
    #[track_caller]
    pub fn get(token: &'static MainThreadToken) -> OptRefMut<'static, DbRoot, DbRoot> {
        static DB: NOptRefCell<DbRoot> = NOptRefCell::new_empty();

        if DB.is_empty(token) {
            DB.replace(token, Some(Self::default()));
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

    pub fn despawn_entity_without_comp_cleanup(
        &mut self,
        entity: InertEntity,
    ) -> Result<ComponentListSnapshot, EntityDeadError> {
        // Fetch the entity info
        let Some(entity_info) = self.alive_entities.remove(&entity) else {
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

        // Update the virtual archetype counters and do cleanup if possible
        if &entity_info.virtual_arch != self.arch_map.root() {
            self.arch_map
                .arena_mut()
                .get_mut(&entity_info.virtual_arch)
                .value_mut()
                .virtual_count -= 1;

            if Self::can_remove_archetype(&self.arch_map, entity_info.virtual_arch) {
                Self::rec_remove_stepping_stone_arches(
                    &mut self.arch_map,
                    entity_info.virtual_arch,
                );
            }
        }

        Ok(ComponentListSnapshot(entity_info.comp_list))
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

        // Decrement old virtual counter
        if &entity_info.virtual_arch != self.arch_map.root() {
            self.arch_map
                .arena_mut()
                .get_mut(&entity_info.virtual_arch)
                .value_mut()
                .virtual_count -= 1;
        }

        // Update the list
        let post_ctor = |arena: &mut DbArchetypeArena, target_ptr: &DbArchetypeRef| {
            let target = arena.get(target_ptr);

            for tag in target.keys() {
                let tag_state = self.tag_map.entry(*tag).or_insert_with(Default::default);

                tag_state.sorted_containers.push(*target_ptr);
                tag_state.are_sorted_containers_sorted = false;
            }
        };

        let old_virtual_arch = entity_info.virtual_arch;

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

        // Increment new virtual counter
        if &entity_info.virtual_arch != self.arch_map.root() {
            self.arch_map
                .arena_mut()
                .get_mut(&entity_info.virtual_arch)
                .value_mut()
                .virtual_count += 1;
        }

        // Try to delete the old archetype
        if Self::can_remove_archetype(&self.arch_map, old_virtual_arch) {
            Self::rec_remove_stepping_stone_arches(&mut self.arch_map, old_virtual_arch);
        }

        // Determine whether we became dirty
        let is_dirty = entity_info.physical_arch != entity_info.virtual_arch;

        // Add the entity to the dirty list if it became dirty. This is only a heuristic and may
        // happen multiple times if we keep on returning to our original archetype but we don't really
        // mind since this list can accept false positives.
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

    pub fn borrow_query_guard(
        &self,
        token: &'static MainThreadToken,
    ) -> OptRef<'static, RecursiveQueryGuardTy> {
        self.query_guard.borrow(token)
    }

    fn prepare_query_common(
        &mut self,
        tags: ReifiedTagList,
        mut f: impl FnMut(&mut DbArchetypeArena, DbArchetypeRef),
    ) {
        if tags.iter().next().is_none() {
            return;
        }

        // Ensure that all tag containers are sorted
        for tag_id in tags.iter() {
            let Some(tag) = self.tag_map.get_mut(tag_id) else {
                continue;
            };
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
            let Some(primary_arch) = primary_iter.next() else {
                break 'scan;
            };

            // Ensure that the archetype exists in all other tags
            for other_iter in &mut tag_iters {
                // Consume all archetypes less than primary_arch
                let other_arch = loop {
                    let Some(other_arch) = other_iter.as_slice().first() else {
                        break 'scan;
                    };

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

    pub fn prepare_named_entity_query(
        &mut self,
        tags: ReifiedTagList,
    ) -> Vec<QueryChunkWithEntities> {
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

    pub fn prepare_anonymous_entity_query(&mut self, tags: ReifiedTagList) -> Vec<QueryChunk> {
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

    pub fn flush_archetypes(
        &mut self,
        token: &'static MainThreadToken,
    ) -> Result<(), ConcurrentFlushError> {
        let mut guard_loaner = PotentialMutableBorrow::new();
        let _guard = self
            .query_guard
            .try_borrow_mut(token, &mut guard_loaner)
            .map_err(|_| ConcurrentFlushError)?;

        let mut may_need_truncation = FxHashSet::default();
        let mut may_need_arch_deletion = Vec::new();

        // Begin by removing dead entities.
        'delete_dead: for info in mem::take(&mut self.dead_dirty_entities) {
            // We know this won't happen because we check for it before adding the entity to the
            // `dead_dirty_entities` list.
            debug_assert_ne!(info.physical_arch, *self.arch_map.root());

            // Determine the archetype we'll be working on.
            let arch_id = info.physical_arch;
            let arch = self.arch_map.arena_mut().get_mut(&arch_id).value_mut();

            // Determine the right candidate for the swap-remove.
            let (last_entity, last_entity_info) = {
                let Some(mut curr_last_heap) = arch.entity_heaps.last() else {
                    // There are no more heaps to work with because there are no more entities in
                    // this archetype.
                    continue 'delete_dead;
                };

                loop {
                    // Find a removal target from the back of the list.

                    // We know this index will succeed because we'll never have a trailing heap whose
                    // length is zero by invariant.
                    let last_entity = curr_last_heap[arch.last_heap_len - 1].get(token);

                    // If this `last_entity` is alive, use it for the swap-remove.
                    if let Some(last_entity_info) = self.alive_entities.get_mut(&last_entity) {
                        break (last_entity, last_entity_info);
                    }

                    // Otherwise, remove it from the list. This action is fine because we're trying
                    // to get rid of these anyways and it is super dangerous to move these dead
                    // entities around.
                    #[cfg(debug_assertions)]
                    curr_last_heap[arch.last_heap_len - 1].set(token, InertEntity::PLACEHOLDER);

                    arch.last_heap_len -= 1;

                    if arch.last_heap_len == 0 {
                        arch.entity_heaps.pop();
                        may_need_truncation.insert(arch_id);

                        let Some(new_last_heap) = arch.entity_heaps.last() else {
                            may_need_arch_deletion.push(arch_id);

                            // If we managed to consume all the entities in this archetype, we know
                            // our target entity is already dead
                            continue 'delete_dead;
                        };

                        arch.last_heap_len = new_last_heap.len();
                        curr_last_heap = new_last_heap;
                    }
                }
            };

            // Determine whether our target entity is still in the archetype. We can use this static
            // index to find the dead entity because dead entities never move and we never insert
            // anything in their place.
            if info.heap_index == arch.entity_heaps.len() - 1
                && info.slot_index >= arch.last_heap_len
            {
                continue;
            }

            let Some(replace_target) = arch
                .entity_heaps
                .get_mut(info.heap_index)
                .map(|heap| &heap[info.slot_index])
            else {
                continue;
            };

            // If it is, commit the swap-replace.

            // The only way for dead entities to be moved around is if they were removed by the
            // swap-remove pruning logic above.
            debug_assert_eq!(replace_target.get(token), info.entity);

            // Replace the slot
            replace_target.set(token, last_entity);
            last_entity_info.heap_index = info.heap_index;
            last_entity_info.slot_index = info.slot_index;

            // Pop from the list.
            arch.last_heap_len -= 1;

            #[cfg(debug_assertions)]
            arch.entity_heaps.last_mut().unwrap()[arch.last_heap_len]
                .set(token, InertEntity::PLACEHOLDER);

            if arch.last_heap_len == 0 {
                arch.entity_heaps.pop();
                may_need_truncation.insert(arch_id);

                if let Some(new_last) = arch.entity_heaps.last() {
                    arch.last_heap_len = new_last.len();
                } else {
                    may_need_arch_deletion.push(arch_id);
                }
            }

            // Move the swap-remove "filler" entity's heap data into the target slot.
            for &managed_ty in &arch.managed {
                let Some(storage) = self.storages.get(&managed_ty) else {
                    // If this fails, it merely means that we never attached this managed type to any
                    // entity, including our own.
                    continue;
                };

                storage.move_entity_into_empty_never_truncate(
                    token,
                    last_entity,
                    last_entity_info,
                    info.physical_arch,
                    arch,
                );
            }
        }

        // Now, move around the alive entities.
        for target in mem::take(&mut self.probably_alive_dirty_entities) {
            // First, ensure that the entity is still alive and dirty since, although we've deleted
            // all dead entities from the heap, we may still have dead entities in our queue.
            let Some(target_info) = self.alive_entities.get_mut(&target) else {
                continue;
            };

            let src_arch_id = target_info.physical_arch;
            let dst_arch_id = target_info.virtual_arch;

            if src_arch_id == dst_arch_id {
                continue;
            }

            let src_target_heap = target_info.heap_index;
            let src_target_slot = target_info.slot_index;

            // We start by moving the entity into its target archetype. We do this first instead of
            // the swap remove because we don't want our entity state to get clobbered by the swap
            // remove.
            {
                let target_info = self.alive_entities.get_mut(&target).unwrap();

                // Allocate some space for the new entity we're inserting.
                //
                // The root archetype doesn't manage any heaps so we don't allocate any space in it.
                if dst_arch_id != *self.arch_map.root() {
                    // Add to the target archetype list
                    let dst_arch = self.arch_map.arena_mut().get_mut(&dst_arch_id).value_mut();

                    if dst_arch.last_heap_len
                        == dst_arch.entity_heaps.last().map_or(0, |heap| heap.len())
                    {
                        let sub_heap = Arc::from_iter(
                            (0..128).map(|_| NMainCell::new(InertEntity::PLACEHOLDER)),
                        );
                        sub_heap[0].set(token, target);

                        dst_arch.entity_heaps.push(sub_heap);
                        dst_arch.last_heap_len = 1;
                    } else {
                        // In debug builds, these values are properly reset to help avoid confusion.
                        debug_assert_eq!(
                            dst_arch.entity_heaps.last().unwrap()[dst_arch.last_heap_len]
                                .get(token),
                            InertEntity::PLACEHOLDER,
                        );

                        dst_arch.entity_heaps.last_mut().unwrap()[dst_arch.last_heap_len]
                            .set(token, target);
                        dst_arch.last_heap_len += 1;
                    }

                    // Update the entity info
                    target_info.heap_index = dst_arch.entity_heaps.len() - 1;
                    target_info.slot_index = dst_arch.last_heap_len - 1;
                }

                // Move the entity's heap data into its target archetype. The set of components to
                // move is the union of the set of components the old archetype used to manage and
                // the set of components the new archetype now manages. We only care about managed
                // components since an anonymous-to-anonymous move is a no-op.
                target_info.physical_arch = dst_arch_id;

                let src_arch = self.arch_map.arena().get(&src_arch_id).value();
                let dst_arch = self.arch_map.arena().get(&dst_arch_id).value();

                for &managed_ty in filter_duplicates(merge_iters(
                    &*src_arch.managed_sorted,
                    &*dst_arch.managed_sorted,
                )) {
                    let Some(storage) = self.storages.get(&managed_ty) else {
                        // If this fails, it merely means that we never attached this managed type to
                        // any entity, including our own.
                        continue;
                    };

                    storage.move_entity_into_empty_never_truncate(
                        token,
                        target,
                        target_info,
                        src_arch_id,
                        dst_arch,
                    );
                }
            }

            // This entity's old slot is now potentially empty. We solve this with a swap-remove.
            {
                let root_arch_id = *self.arch_map.root();

                // The root archetype doesn't manage any heaps so don't have to manage anything in it.
                if src_arch_id != root_arch_id {
                    let src_arch = self.arch_map.arena_mut().get_mut(&src_arch_id).value_mut();

                    // We also ignore now-empty archetypes (where `last_heap_len == 1`) and source
                    // heap slots which are already in the last position in the list.
                    if src_target_heap != src_arch.entity_heaps.len() - 1
                        || src_target_slot != src_arch.last_heap_len - 1
                    {
                        // Determine the filler entity
                        let last_entity = src_arch
                            .entity_heaps
                            .last_mut()
                            // This unwrap is guaranteed to succeed because at least one entity (our `entity`)
                            // which is known to be alive. Additionally, we know this entity will be alive
                            // because every archetype has already been cleared of its dead entities.
                            .unwrap()[src_arch.last_heap_len - 1]
                            .get(token);

                        // Replace the slot
                        src_arch.entity_heaps[src_target_heap][src_target_slot]
                            .set(token, last_entity);

                        // Update the filler's location mirror
                        let last_entity_info = self.alive_entities.get_mut(&last_entity).unwrap();
                        last_entity_info.heap_index = src_target_heap;
                        last_entity_info.slot_index = src_target_slot;

                        // Now, move the component data to the appropriate position
                        for &managed_ty in &src_arch.managed {
                            let Some(storage) = self.storages.get(&managed_ty) else {
                                // If this fails, it merely means that we never attached this managed type to any
                                // entity, including our own.
                                continue;
                            };

                            storage.move_entity_into_empty_never_truncate(
                                token,
                                last_entity,
                                last_entity_info,
                                src_arch_id,
                                src_arch,
                            );
                        }
                    }

                    // Regardless of whether we actually had to move an entity, we still have to
                    // truncate the source archetype.
                    src_arch.last_heap_len -= 1;
                    #[cfg(debug_assertions)]
                    src_arch.entity_heaps.last_mut().unwrap()[src_arch.last_heap_len]
                        .set(token, InertEntity::PLACEHOLDER);

                    if src_arch.last_heap_len == 0 {
                        src_arch.entity_heaps.pop();
                        may_need_truncation.insert(src_arch_id);

                        if src_arch.entity_heaps.is_empty() {
                            may_need_arch_deletion.push(src_arch_id);
                        }

                        if let Some(new_last) = src_arch.entity_heaps.last() {
                            src_arch.last_heap_len = new_last.len();
                        }
                    }
                }
            }
        }

        // Truncate storages which need it
        for arch_id in may_need_truncation {
            let arch = self.arch_map.arena().get(&arch_id).value();
            for &managed in &arch.managed {
                let Some(storage) = self.storages.get(&managed) else {
                    continue;
                };

                storage.truncate_archetype_heap_len(token, arch_id, arch.entity_heaps.len());
            }
        }

        // Destroy archetypes which no longer exist.
        for arch_id in may_need_arch_deletion {
            debug_assert_ne!(&arch_id, self.arch_map.root());

            if !Self::can_remove_archetype(&self.arch_map, arch_id) {
                continue;
            }

            // Remove the archetype from the tags
            let arch = self.arch_map.arena().get(&arch_id).value();

            for tag in arch.tags.iter().copied() {
                let HmEntry::Occupied(mut entry) = self.tag_map.entry(tag) else {
                    unreachable!()
                };

                entry.get_mut().sorted_containers.retain(|v| *v != arch_id);

                if entry.get().sorted_containers.is_empty() {
                    entry.remove();
                }
            }

            // Remove the archetype from the map
            Self::rec_remove_stepping_stone_arches(&mut self.arch_map, arch_id);
        }

        Ok(())
    }

    fn can_remove_archetype(arch_map: &DbArchetypeMap, arch_id: DbArchetypeRef) -> bool {
        // We can't remove the root.
        if &arch_id == arch_map.root() {
            return false;
        }

        let arch_entry = arch_map.arena().get(&arch_id);
        let arch = arch_entry.value();

        // We can't remove archetypes with virtual or physical entities in them.
        if !arch.entity_heaps.is_empty() || arch.virtual_count > 0 {
            return false;
        }

        // We shouldn't remove archetypes who are used as stepping stones to other archetypes.
        if arch_entry
            .extensions()
            .iter()
            .any(|(_, target)| target != &arch_id)
        {
            return false;
        }

        // Otherwise, it's safe to remove.
        true
    }

    fn rec_remove_stepping_stone_arches(arch_map: &mut DbArchetypeMap, arch_id: DbArchetypeRef) {
        debug_assert!(Self::can_remove_archetype(arch_map, arch_id));

        let arch = arch_map.remove(arch_id);

        for src in arch.de_extensions().values() {
            if Self::can_remove_archetype(arch_map, *src) && *src != arch_id {
                Self::rec_remove_stepping_stone_arches(arch_map, *src);
            }
        }
    }

    // === Storage management === //

    pub fn get_storage<T: 'static>(
        &mut self,
        token: &'static MainThreadToken,
    ) -> &'static DbStorage<T> {
        self.storages
            .entry(NamedTypeId::of::<T>())
            .or_insert_with(|| {
                leak(DbStorage::new_full(
                    token,
                    DbStorageInner::<T> {
                        anon_block_alloc: BlockAllocator::default(),
                        mappings: NopHashMap::default(),
                        heaps: FxHashMap::default(),
                    },
                ))
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
        // Unlink the entity
        let Some(removed) = storage.mappings.remove(&entity) else {
            // We only perform a liveness check if the entity doesn't have this component.
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

    pub fn entity_has_component_dyn(
        &self,
        token: &'static MainThreadToken,
        entity: InertEntity,
        ty: TypeId,
    ) -> bool {
        self.storages
            .get(&ty)
            .is_some_and(|storage| storage.contains_entity(token, entity))
    }

    // === Debug === //

    pub fn debug_total_spawns(&self) -> u64 {
        self.debug_total_spawns
    }

    pub fn debug_alive_list(&self) -> impl ExactSizeIterator<Item = InertEntity> + '_ {
        self.alive_entities.keys().copied()
    }

    pub fn debug_archetype_count(&self) -> u64 {
        self.arch_map.len() as u64
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
                Self::get_component(&self.get_storage::<DebugLabel>(token).borrow(token), entity)
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

    fn move_entity_into_empty_never_truncate(
        &self,
        token: &'static MainThreadToken,
        target: InertEntity,
        completed_target_info: &DbEntity,
        src_arch: DbArchetypeRef,
        dst_arch_info: &DbArchetype,
    ) {
        let storage = &mut *self.borrow_mut(token);

        let Some(mapping) = storage.mappings.get_mut(&target) else {
            // This may fail if a user never inserted a component for every managed type.
            return;
        };

        // Determine whether the new layout requires a managed allocation
        let external_heaps = match storage.heaps.entry(completed_target_info.physical_arch) {
            hashbrown::hash_map::Entry::Occupied(entry) => Some(entry.into_mut()),
            hashbrown::hash_map::Entry::Vacant(entry) => dst_arch_info
                .managed
                .contains(&NamedTypeId::of::<T>())
                .then(|| entry.insert(Vec::new())),
        };

        if let Some(external_heaps) = external_heaps {
            // Ensure that we have an appropriate slot for this entity
            let min_heaps_len = completed_target_info.heap_index + 1;
            if external_heaps.len() < min_heaps_len {
                external_heaps.extend(
                    (external_heaps.len()..min_heaps_len)
                        .map(|i| Arc::new(Heap::new(token, dst_arch_info.entity_heaps[i].len()))),
                );
            }

            // Ensure that the target slot is indeed ownerless as per contract.
            debug_assert_eq!(
                dst_arch_info.entity_heaps[completed_target_info.heap_index]
                    [completed_target_info.slot_index]
                    .get(token),
                target,
            );
            debug_assert_eq!(
                external_heaps[completed_target_info.heap_index]
                    .slot(token, completed_target_info.slot_index)
                    .owner(token)
                    .map(|v| v.inert),
                None,
            );

            // Swap value from the old slot. Deallocate the old anonymous reservation if applicable
            // and mark it as external
            let external_heaps = &storage.heaps[&completed_target_info.physical_arch];
            let target_heap = &external_heaps[completed_target_info.heap_index];

            match mem::replace(
                &mut mapping.heap,
                DbEntityMappingHeap::External {
                    heap: completed_target_info.heap_index,
                    slot: completed_target_info.slot_index,
                },
            ) {
                DbEntityMappingHeap::Anonymous(resv) => {
                    storage.anon_block_alloc.block(&resv.block).swap_slots(
                        token,
                        resv.slot,
                        target_heap,
                        completed_target_info.slot_index,
                    );
                    storage.anon_block_alloc.dealloc(resv, drop);
                }
                DbEntityMappingHeap::External {
                    heap: old_heap,
                    slot: old_slot,
                } => {
                    debug_assert_eq!(
                        storage.heaps[&src_arch][old_heap]
                            .slot(token, old_slot)
                            .owner(token)
                            .map(|v| v.inert),
                        Some(target),
                    );

                    storage.heaps[&src_arch][old_heap].swap_slots(
                        token,
                        old_slot,
                        target_heap,
                        completed_target_info.slot_index,
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

    fn truncate_archetype_heap_len(
        &self,
        token: &'static MainThreadToken,
        arch: DbArchetypeRef,
        heap_count: usize,
    ) {
        let storage = &mut *self.borrow_mut(token);

        if heap_count > 0 {
            let Some(heap_list) = storage.heaps.get_mut(&arch) else {
                return;
            };

            heap_list.truncate(heap_count);
        } else {
            drop(storage.heaps.remove(&arch));
        }
    }

    fn contains_entity(&self, token: &'static MainThreadToken, entity: InertEntity) -> bool {
        self.borrow(token).mappings.contains_key(&entity)
    }
}

pub fn get_global_tag(id: NamedTypeId, managed_ty: NamedTypeId) -> RawTag {
    static TAGS: Mutex<FxHashMap<NamedTypeId, RawTag>> =
        Mutex::new(FxHashMap::with_hasher(ConstSafeBuildHasherDefault::new()));

    thread_local! {
        static TAG_CACHE: RefCell<FxHashMap<NamedTypeId, RawTag>> = const {
            RefCell::new(FxHashMap::with_hasher(ConstSafeBuildHasherDefault::new()))
        };
    }

    TAG_CACHE.with(|cache| {
        *cache.borrow_mut().entry(id).or_insert_with(|| {
            *unpoison(TAGS.lock())
                .entry(id)
                .or_insert_with(|| RawTag::new(managed_ty))
        })
    })
}

// === Public helpers === //

#[derive(Debug)]
pub struct EntityDeadError;

#[derive(Debug)]
pub struct ConcurrentFlushError;

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

#[derive(Debug, Copy, Clone)]
pub struct ReifiedTagList<'a> {
    pub static_tags: &'a [Option<InertTag>],
    pub dynamic_tags: &'a [InertTag],
}

impl<'a> ReifiedTagList<'a> {
    pub fn iter(self) -> impl Iterator<Item = &'a InertTag> + Clone + 'a {
        self.static_tags
            .iter()
            .filter_map(|v| v.as_ref())
            .chain(self.dynamic_tags.iter())
    }
}

#[derive(Debug, Copy, Clone)]
pub struct ComponentListSnapshot(DbComponentListRef);

impl ComponentListSnapshot {
    pub fn run_dtors(self, token: &'static MainThreadToken, target: InertEntity) {
        let len = self.0.direct_borrow().keys().len();

        for i in 0..len {
            let dtor = self.0.direct_borrow().keys()[i].dtor;

            // Analysis of destructors is essentially useless because we assume that all destructors
            // could run at any time. `.destroy()` is already dangerous because of UAFs so what's a
            // bit of extra danger to keep our users up at night?
            autoken::assume_no_alias(|| dtor(PhantomData, token, target));
        }
    }
}
