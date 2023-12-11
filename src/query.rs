use std::{
    any::{Any, TypeId},
    fmt,
    hash::Hash,
    marker::PhantomData,
    ops::ControlFlow,
    sync::Arc,
};

use derive_where::derive_where;

use crate::{
    core::{
        cell::{MultiRefCellIndex, OptRef},
        heap::Heap,
        token::MainThreadToken,
        token_cell::NMainCell,
    },
    database::{
        get_global_tag, DbRoot, InertArchetypeId, InertEntity, InertTag, RecursiveQueryGuardTy,
        ReifiedTagList,
    },
    entity::Storage,
    util::{
        hash_map::{ConstSafeBuildHasherDefault, FxHashMap},
        iter::hash_one,
        misc::NamedTypeId,
    },
    Entity,
};

// === Tag === //

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

    pub fn global<G: HasGlobalManagedTag<Component = T>>() -> Self {
        Self {
            _ty: PhantomData,
            raw: get_global_tag(NamedTypeId::of::<G>(), NamedTypeId::of::<T>()),
        }
    }

    pub fn raw(self) -> RawTag {
        self.raw
    }
}

impl<T> From<Tag<T>> for RawTag {
    fn from(value: Tag<T>) -> Self {
        value.raw
    }
}

impl<T> Default for Tag<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct VirtualTag {
    raw: RawTag,
}

impl VirtualTag {
    pub fn new() -> Self {
        Self {
            raw: RawTag::new(InertTag::inert_ty_id()),
        }
    }

    pub fn global<T: HasGlobalVirtualTag>() -> Self {
        Self {
            raw: get_global_tag(NamedTypeId::of::<T>(), InertTag::inert_ty_id()),
        }
    }

    pub fn raw(self) -> RawTag {
        self.raw
    }
}

impl From<VirtualTag> for RawTag {
    fn from(value: VirtualTag) -> Self {
        value.raw
    }
}

impl Default for VirtualTag {
    fn default() -> Self {
        Self::new()
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

    pub fn ty(self) -> TypeId {
        self.0.ty().raw()
    }

    pub fn unerase<T: 'static>(self) -> Option<Tag<T>> {
        (self.0.ty() == NamedTypeId::of::<T>()).then_some(Tag {
            _ty: PhantomData,
            raw: self,
        })
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

// === Global Tags === //

// Traits
pub trait HasGlobalManagedTag: Sized + 'static {
    type Component: 'static;
}

pub trait HasGlobalVirtualTag: Sized + 'static {}

// Aliases
mod tag_alias_sealed {
    use std::marker::PhantomData;

    use derive_where::derive_where;

    #[derive(Debug, Copy, Clone)]
    pub enum Never {}

    #[derive_where(Debug, Copy, Clone)]
    pub enum GlobalTag<T> {
        _Phantom(Never, PhantomData<fn() -> T>),
        GlobalTag,
    }

    #[derive_where(Debug, Copy, Clone)]
    pub enum GlobalVirtualTag<T> {
        _Phantom(Never, PhantomData<fn() -> T>),
        GlobalVirtualTag,
    }

    pub mod globs {
        pub use super::{GlobalTag::GlobalTag, GlobalVirtualTag::GlobalVirtualTag};
    }
}

pub use tag_alias_sealed::{globs::*, GlobalTag, GlobalVirtualTag};

impl<T: HasGlobalManagedTag> From<GlobalTag<T>> for Tag<T::Component> {
    fn from(_: GlobalTag<T>) -> Self {
        Tag::global::<T>()
    }
}

impl<T: HasGlobalManagedTag> From<GlobalTag<T>> for RawTag {
    fn from(value: GlobalTag<T>) -> Self {
        Tag::from(value).into()
    }
}

impl<T: HasGlobalVirtualTag> From<GlobalVirtualTag<T>> for VirtualTag {
    fn from(_: GlobalVirtualTag<T>) -> Self {
        VirtualTag::global::<T>()
    }
}

impl<T: HasGlobalVirtualTag> From<GlobalVirtualTag<T>> for RawTag {
    fn from(value: GlobalVirtualTag<T>) -> Self {
        VirtualTag::from(value).into()
    }
}

// === ArchetypeIds === //

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub struct ArchetypeId(pub(crate) InertArchetypeId);

impl ArchetypeId {
    pub fn in_intersection(
        tags: impl IntoIterator<Item = RawTag>,
        include_entities: bool,
    ) -> Option<Vec<ArchetypeQueryInfo>> {
        let token = MainThreadToken::acquire_fmt("enumerate archetypes in a tag intersection");

        let mut archetypes = Vec::new();
        let is_non_empty = ReifiedTagList::reify(tags, |tags| {
            if tags.is_non_empty() {
                DbRoot::get(token).enumerate_tag_intersection(tags, |info| {
                    archetypes.push(ArchetypeQueryInfo {
                        archetype: info.archetype.into_dangerous_archetype_id(),
                        heap_count: info.entities.len(),
                        last_heap_len: info.last_heap_len,
                        entities: include_entities.then(|| info.entities.clone()),
                    });
                });
                true
            } else {
                false
            }
        });

        is_non_empty.then_some(archetypes)
    }
}

#[derive(Debug, Clone)]
pub struct ArchetypeQueryInfo {
    archetype: ArchetypeId,
    heap_count: usize,
    last_heap_len: usize,
    entities: Option<Vec<Arc<[NMainCell<InertEntity>]>>>,
}

impl ArchetypeQueryInfo {
    pub fn archetype(&self) -> ArchetypeId {
        self.archetype
    }

    pub fn heap_count(&self) -> usize {
        self.heap_count
    }

    pub fn last_heap_len(&self) -> usize {
        self.last_heap_len
    }

    pub fn heaps_for<T>(&self, storage: &Storage<T>) -> Vec<Arc<Heap<T>>> {
        DbRoot::heaps_from_archetype_aba(self.archetype.0, &storage.inner.borrow(&storage.token))
    }

    // TODO: Expose entities
}

// === Flushing === //

#[must_use]
pub fn try_flush() -> bool {
    let token = MainThreadToken::acquire_fmt("flush entity archetypes");
    DbRoot::get(token).flush_archetypes(token).is_ok()
}

fn flush_with_custom_msg(msg: &'static str) {
    autoken::assert_mutably_borrowable::<RecursiveQueryGuardTy>();
    assert!(try_flush(), "{msg}");
}

pub fn flush() {
    flush_with_custom_msg("attempted to flush the entity database while a query was active");
}

pub fn total_flush_count() -> u64 {
    DbRoot::get(MainThreadToken::acquire_fmt("query total flush count")).total_flush_count()
}

#[derive(Debug)]
pub struct FlushGuard(OptRef<'static, RecursiveQueryGuardTy>);

impl Clone for FlushGuard {
    fn clone(&self) -> Self {
        Self(OptRef::clone(&self.0))
    }
}

pub fn borrow_flush_guard() -> FlushGuard {
    let token = MainThreadToken::acquire_fmt("borrow the flush guard");

    FlushGuard(DbRoot::get(token).borrow_query_guard(token))
}

// === Query Version Tracking === //

pub trait QueryKey: 'static + Sized + Send + Sync + Clone + Hash + PartialEq {}

impl<T: 'static + Send + Sync + Clone + Hash + PartialEq> QueryKey for T {}

#[derive_where(Default)]
#[derive(Clone)]
pub struct QueryVersionMap<V> {
    versions: FxHashMap<ReifiedQueryKey, V>,
}

impl<V> fmt::Debug for QueryVersionMap<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QueryVersionMap").finish_non_exhaustive()
    }
}

impl<V> QueryVersionMap<V> {
    pub const fn new() -> Self {
        Self {
            versions: FxHashMap::with_hasher(ConstSafeBuildHasherDefault::new()),
        }
    }

    pub fn clear(&mut self) {
        self.versions.clear();
    }

    pub fn entry<K>(&mut self, key: K, version_ctor: impl FnOnce() -> V) -> &mut V
    where
        K: QueryKey,
    {
        let hash = hash_one(self.versions.hasher(), &key);
        let entry = self.versions.raw_entry_mut().from_hash(hash, |entry| {
            hash == entry.hash
                && entry
                    .value
                    .as_any()
                    .downcast_ref::<K>()
                    .is_some_and(|candidate| candidate == &key)
        });

        match entry {
            hashbrown::hash_map::RawEntryMut::Occupied(occupied) => occupied.into_mut(),
            hashbrown::hash_map::RawEntryMut::Vacant(vacant) => {
                vacant
                    .insert_with_hasher(
                        hash,
                        ReifiedQueryKey {
                            hash,
                            value: Box::new(key),
                        },
                        version_ctor(),
                        |entry| entry.hash,
                    )
                    .1
            }
        }
    }
}

struct ReifiedQueryKey {
    hash: u64,
    value: Box<dyn AnyAndClone>,
}

impl Clone for ReifiedQueryKey {
    fn clone(&self) -> Self {
        Self {
            hash: self.hash,
            value: self.value.clone_box(),
        }
    }
}

trait AnyAndClone: Any + Send + Sync {
    fn clone_box(&self) -> Box<dyn AnyAndClone>;

    fn as_any(&self) -> &dyn Any;

    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: 'static + Clone + Send + Sync> AnyAndClone for T {
    fn clone_box(&self) -> Box<dyn AnyAndClone> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// === MultiQueryDriver === //

pub type MultiDriverItem<'a, M> = <M as MultiQueryDriverTypes<'a>>::Item;

pub trait MultiQueryDriverTypes<'a, WhereACannotOutliveSelf = &'a Self> {
    type Item;
}

pub trait MultiQueryDriver: for<'a> MultiQueryDriverTypes<'a> {
    fn drive_multi_query<T: QueryDriverTarget, B>(
        &self,
        target: &mut T,
        f: impl FnMut((T::Input<'_>, MultiDriverItem<'_, Self>)) -> ControlFlow<B>,
    ) -> ControlFlow<B>;

    fn chain<O: MultiQueryDriver>(&self, other: O) -> QueryChainAdapter<&'_ Self, O> {
        QueryChainAdapter(self, other)
    }

    fn map<F>(&self, map: F) -> QueryMapAdapter<&'_ Self, F>
    where
        F: for<'a> QueryMapper<MultiDriverItem<'a, Self>>,
    {
        QueryMapAdapter(self, map)
    }
}

mod query_driver_target_sealed {
    pub trait DriverTargetSealed {}
}

use query_driver_target_sealed::DriverTargetSealed;

pub trait QueryDriverTarget: DriverTargetSealed {
    type Input<'a>;

    fn handle_driver<D: QueryDriver, B>(
        &mut self,
        driver: &D,
        f: impl FnMut((Self::Input<'_>, DriverItem<'_, D>)) -> ControlFlow<B>,
    ) -> ControlFlow<B>;
}

// Derivations
impl<'a, D: MultiQueryDriverTypes<'a>> MultiQueryDriverTypes<'a> for &'_ D {
    type Item = D::Item;
}

impl<D: MultiQueryDriver> MultiQueryDriver for &'_ D {
    fn drive_multi_query<T: QueryDriverTarget, B>(
        &self,
        target: &mut T,
        f: impl FnMut((T::Input<'_>, MultiDriverItem<'_, Self>)) -> ControlFlow<B>,
    ) -> ControlFlow<B> {
        (*self).drive_multi_query(target, f)
    }
}

// === QueryDriver === //

pub type DriverItem<'a, D> = <D as QueryDriverTypes<'a>>::Item;
pub type DriverArchIterInfo<'a, D> = <D as QueryDriverTypes<'a>>::ArchIterInfo;
pub type DriverHeapIterInfo<'a, D> = <D as QueryDriverTypes<'a>>::HeapIterInfo;
pub type DriverBlockIterInfo<'a, D> = <D as QueryDriverTypes<'a>>::BlockIterInfo;

pub trait QueryDriverTypes<'a, WhereACannotOutliveSelf = &'a Self> {
    type Item;
    type ArchIterInfo;
    type HeapIterInfo;
    type BlockIterInfo;
}

pub trait QueryDriver: Sized + for<'a> QueryDriverTypes<'a> {
    fn drive_query<B>(
        &self,
        query_key: impl QueryKey,
        tags: impl IntoIterator<Item = RawTag>,
        include_entities: bool,
        handler: impl QueryDriverEntryHandler<Self, B>,
    ) -> ControlFlow<B>;

    fn foreach_heap<B>(
        &self,
        arch: &ArchetypeQueryInfo,
        arch_userdata: &mut DriverArchIterInfo<'_, Self>,
        handler: impl QueryHeapHandler<Self, B>,
    ) -> ControlFlow<B>;

    fn foreach_block<B>(
        &self,
        heap_idx: usize,
        heap_len: usize,
        heap_userdata: &mut DriverHeapIterInfo<'_, Self>,
        handler: impl QueryBlockHandler<Self, B>,
    ) -> ControlFlow<B>;

    fn foreach_element_in_full_block<B>(
        &self,
        block: usize,
        block_userdata: &mut DriverBlockIterInfo<'_, Self>,
        handler: impl QueryBlockElementHandler<Self, B>,
    ) -> ControlFlow<B>;

    fn foreach_element_in_semi_block<B>(
        &self,
        block: usize,
        block_userdata: &mut DriverBlockIterInfo<'_, Self>,
        handler: impl QueryBlockElementHandler<Self, B>,
    ) -> ControlFlow<B>;
}

mod query_handler_sealed {
    use super::QueryDriver;

    pub trait QueryHandlerSealed<D: QueryDriver, B> {}
}

use query_handler_sealed::QueryHandlerSealed;

pub trait QueryDriverEntryHandler<D: QueryDriver, B>: QueryHandlerSealed<D, B> {
    fn process_arch(
        &mut self,
        info: &ArchetypeQueryInfo,
        userdata: DriverArchIterInfo<'_, D>,
    ) -> ControlFlow<B>;

    fn process_arbitrary(&mut self, entity: Entity, item: DriverItem<'_, D>) -> ControlFlow<B>;
}

pub trait QueryHeapHandler<D: QueryDriver, B>: QueryHandlerSealed<D, B> {
    fn process_heap(&mut self, index: usize, userdata: DriverHeapIterInfo<'_, D>)
        -> ControlFlow<B>;
}

pub trait QueryBlockHandler<D: QueryDriver, B>: QueryHandlerSealed<D, B> {
    fn process_block(
        &mut self,
        index: usize,
        userdata: DriverBlockIterInfo<'_, D>,
    ) -> ControlFlow<B>;
}

pub trait QueryBlockElementHandler<D: QueryDriver, B>: QueryHandlerSealed<D, B> {
    fn process_element(
        &mut self,
        index: MultiRefCellIndex,
        item: DriverItem<'_, D>,
    ) -> ControlFlow<B>;
}

// === Query Adapters === //

// QueryChainAdapter
pub struct QueryChainAdapter<M1, M2>(pub M1, pub M2);

impl<'a, M1, M2> MultiQueryDriverTypes<'a> for QueryChainAdapter<M1, M2>
where
    M1: MultiQueryDriverTypes<'a>,
{
    type Item = M1::Item;
}

impl<M1, M2> MultiQueryDriver for QueryChainAdapter<M1, M2>
where
    M1: MultiQueryDriver,
    M2: MultiQueryDriver + for<'a> MultiQueryDriverTypes<'a, Item = MultiDriverItem<'a, M1>>,
{
    fn drive_multi_query<T: QueryDriverTarget, B>(
        &self,
        target: &mut T,
        mut f: impl FnMut((T::Input<'_>, MultiDriverItem<'_, Self>)) -> ControlFlow<B>,
    ) -> ControlFlow<B> {
        self.0.drive_multi_query(target, &mut f)?;
        self.1.drive_multi_query(target, &mut f)?;
        ControlFlow::Continue(())
    }
}

// QueryMapAdapter
pub struct QueryMapAdapter<M, F>(pub M, pub F);

pub trait QueryMapper<I> {
    type Output;

    fn map(&self, input: I) -> Self::Output;
}

impl<I, O, F: Fn(I) -> O> QueryMapper<I> for F {
    type Output = O;

    fn map(&self, input: I) -> Self::Output {
        self(input)
    }
}

impl<'a, M, F> MultiQueryDriverTypes<'a> for QueryMapAdapter<M, F>
where
    M: MultiQueryDriverTypes<'a>,
    F: QueryMapper<M::Item>,
{
    type Item = F::Output;
}

impl<M, F> MultiQueryDriver for QueryMapAdapter<M, F>
where
    M: MultiQueryDriver,
    F: for<'a> QueryMapper<MultiDriverItem<'a, M>>,
{
    fn drive_multi_query<T: QueryDriverTarget, B>(
        &self,
        target: &mut T,
        mut f: impl FnMut((T::Input<'_>, MultiDriverItem<'_, Self>)) -> ControlFlow<B>,
    ) -> ControlFlow<B> {
        self.0
            .drive_multi_query(target, |(input, item)| f((input, self.1.map(item))))
    }
}

// === Query Macro === //

#[doc(hidden)]
pub mod query_internals {
    use std::{iter, marker::PhantomData, ops::ControlFlow, sync::Arc};

    use autoken::{ImmutableBorrow, MutableBorrow};

    use crate::{
        core::{
            cell::{MultiOptRef, MultiOptRefMut, MultiRefCellIndex},
            heap::{
                array_chunks, heap_block_iter, heap_block_slot_iter, DirectSlot, Heap,
                HeapSlotBlock, Slot,
            },
            random_iter::{
                RandomAccessEnumerate, RandomAccessIter, RandomAccessRepeat, RandomAccessSliceMut,
                RandomAccessSliceRef, RandomAccessTake, RandomAccessVec, RandomAccessZip,
                UnivRandomAccessIter, UntiedRandomAccessIter,
            },
            token::{BorrowMutToken, BorrowToken, MainThreadToken, Token},
            token_cell::NMainCell,
        },
        database::InertEntity,
        entity::{CompMut, CompRef, Entity},
        obj::Obj,
        storage, Storage,
    };

    use super::{
        borrow_flush_guard, query_driver_target_sealed::DriverTargetSealed,
        query_handler_sealed::QueryHandlerSealed, ArchetypeId, ArchetypeQueryInfo,
        DriverArchIterInfo, DriverBlockIterInfo, DriverHeapIterInfo, DriverItem,
        HasGlobalManagedTag, MultiDriverItem, MultiQueryDriver, QueryBlockElementHandler,
        QueryBlockHandler, QueryDriver, QueryDriverEntryHandler, QueryDriverTarget,
        QueryHeapHandler, QueryKey, RawTag, Tag,
    };

    pub use {
        cbit::cbit,
        std::{compile_error, concat, iter::Iterator, stringify},
    };

    // === QueryXxHandler === //

    // QueryDriveEntryHandlerInstance
    pub struct QueryDriveEntryHandlerInstance<D, B, U, F1, F2>(PhantomData<fn() -> D>, U, F1, F2)
    where
        D: QueryDriver,
        F1: FnMut(&mut U, &ArchetypeQueryInfo, DriverArchIterInfo<'_, D>) -> ControlFlow<B>,
        F2: FnMut(&mut U, Entity, DriverItem<'_, D>) -> ControlFlow<B>;

    impl<D, B, U, F1, F2> QueryHandlerSealed<D, B> for QueryDriveEntryHandlerInstance<D, B, U, F1, F2>
    where
        D: QueryDriver,
        F1: FnMut(&mut U, &ArchetypeQueryInfo, DriverArchIterInfo<'_, D>) -> ControlFlow<B>,
        F2: FnMut(&mut U, Entity, DriverItem<'_, D>) -> ControlFlow<B>,
    {
    }

    impl<D, B, U, F1, F2> QueryDriverEntryHandler<D, B>
        for QueryDriveEntryHandlerInstance<D, B, U, F1, F2>
    where
        D: QueryDriver,
        F1: FnMut(&mut U, &ArchetypeQueryInfo, DriverArchIterInfo<'_, D>) -> ControlFlow<B>,
        F2: FnMut(&mut U, Entity, DriverItem<'_, D>) -> ControlFlow<B>,
    {
        fn process_arch(
            &mut self,
            info: &ArchetypeQueryInfo,
            userdata: DriverArchIterInfo<'_, D>,
        ) -> ControlFlow<B> {
            self.2(&mut self.1, info, userdata)
        }

        fn process_arbitrary(&mut self, entity: Entity, item: DriverItem<'_, D>) -> ControlFlow<B> {
            self.3(&mut self.1, entity, item)
        }
    }

    // QueryHeapHandlerInstance
    pub struct QueryHeapHandlerInstance<D, B, F>(PhantomData<fn() -> D>, F)
    where
        D: QueryDriver,
        F: FnMut(usize, DriverHeapIterInfo<'_, D>) -> ControlFlow<B>;

    impl<D, B, F> QueryHandlerSealed<D, B> for QueryHeapHandlerInstance<D, B, F>
    where
        D: QueryDriver,
        F: FnMut(usize, DriverHeapIterInfo<'_, D>) -> ControlFlow<B>,
    {
    }

    impl<D, B, F> QueryHeapHandler<D, B> for QueryHeapHandlerInstance<D, B, F>
    where
        D: QueryDriver,
        F: FnMut(usize, DriverHeapIterInfo<'_, D>) -> ControlFlow<B>,
    {
        fn process_heap(
            &mut self,
            index: usize,
            userdata: DriverHeapIterInfo<'_, D>,
        ) -> ControlFlow<B> {
            self.1(index, userdata)
        }
    }

    // QueryBlockHandlerInstance
    pub struct QueryBlockHandlerInstance<D, B, F>(PhantomData<fn() -> D>, F)
    where
        D: QueryDriver,
        F: FnMut(usize, DriverBlockIterInfo<'_, D>) -> ControlFlow<B>;

    impl<D, B, F> QueryHandlerSealed<D, B> for QueryBlockHandlerInstance<D, B, F>
    where
        D: QueryDriver,
        F: FnMut(usize, DriverBlockIterInfo<'_, D>) -> ControlFlow<B>,
    {
    }

    impl<D, B, F> QueryBlockHandler<D, B> for QueryBlockHandlerInstance<D, B, F>
    where
        D: QueryDriver,
        F: FnMut(usize, DriverBlockIterInfo<'_, D>) -> ControlFlow<B>,
    {
        fn process_block(
            &mut self,
            index: usize,
            userdata: DriverBlockIterInfo<'_, D>,
        ) -> ControlFlow<B> {
            self.1(index, userdata)
        }
    }

    // QueryBlockElementHandlerInstance
    pub struct QueryBlockElementHandlerInstance<D, B, F>(PhantomData<fn() -> D>, F)
    where
        D: QueryDriver,
        F: FnMut(MultiRefCellIndex, DriverItem<'_, D>) -> ControlFlow<B>;

    impl<D, B, F> QueryHandlerSealed<D, B> for QueryBlockElementHandlerInstance<D, B, F>
    where
        D: QueryDriver,
        F: FnMut(MultiRefCellIndex, DriverItem<'_, D>) -> ControlFlow<B>,
    {
    }

    impl<D, B, F> QueryBlockElementHandler<D, B> for QueryBlockElementHandlerInstance<D, B, F>
    where
        D: QueryDriver,
        F: FnMut(MultiRefCellIndex, DriverItem<'_, D>) -> ControlFlow<B>,
    {
        fn process_element(
            &mut self,
            index: MultiRefCellIndex,
            item: DriverItem<'_, D>,
        ) -> ControlFlow<B> {
            self.1(index, item)
        }
    }

    // === QueryHeap === //

    /// A heap over which a [`QueryPart`] can iterate.
    pub trait QueryHeap: Sized + 'static {
        type Storages;

        type HeapIter: for<'a> RandomAccessIter<'a, Item = Self::Heap<'a>>;
        type Heap<'a>;

        type BlockIter<'a, N>: UntiedRandomAccessIter<UntiedItem = Self::Block<'a, N>>
        where
            N: 'a + Token;

        type Block<'a, N>
        where
            N: 'a + Token;

        /// Fetches the storages needed to fetch these heaps.
        fn storages() -> Self::Storages;

        /// Fetches the set of heaps associated with this archetype.
        fn heaps_for_archetype(
            storages: &Self::Storages,
            archetype: &ArchetypeQueryInfo,
        ) -> Self::HeapIter;

        /// Fetches the blocks present in this heap.
        fn blocks<'a, N: Token>(heap: Self::Heap<'a>, token: &'a N) -> Self::BlockIter<'a, N>;
    }

    // No heap
    impl QueryHeap for () {
        type Storages = ();
        type HeapIter = RandomAccessRepeat<()>;
        type Heap<'a> = ();

        type BlockIter<'a, N> = RandomAccessRepeat<()> where N: 'a + Token;

        type Block<'a, N> = () where N: 'a + Token;

        fn storages() -> Self::Storages {}

        fn heaps_for_archetype(
            _storages: &Self::Storages,
            _archetype: &ArchetypeQueryInfo,
        ) -> Self::HeapIter {
            RandomAccessRepeat::new(())
        }

        fn blocks<'a, N: Token>(_heap: Self::Heap<'a>, _token: &'a N) -> Self::BlockIter<'a, N> {
            RandomAccessRepeat::new(())
        }
    }

    // FetchHeap
    pub struct FetchHeap<T: 'static>(PhantomData<fn(T) -> T>);

    impl<T: 'static> QueryHeap for FetchHeap<T> {
        type Storages = Storage<T>;
        type HeapIter = RandomAccessVec<Arc<Heap<T>>>;
        type Heap<'a> = &'a mut Arc<Heap<T>>;

        type BlockIter<'a, N> = heap_block_iter::Iter<'a, T, N> where N: 'a + Token;
        type Block<'a, N> = HeapSlotBlock<'a, T, N> where N: 'a + Token;

        fn storages() -> Self::Storages {
            storage::<T>()
        }

        fn heaps_for_archetype(
            storages: &Self::Storages,
            archetype: &ArchetypeQueryInfo,
        ) -> Self::HeapIter {
            RandomAccessVec::new(archetype.heaps_for(storages))
        }

        fn blocks<'a, N: Token>(heap: Self::Heap<'a>, token: &'a N) -> Self::BlockIter<'a, N> {
            heap.blocks_expose_random_access(token)
        }
    }

    // FetchEntity
    pub struct FetchEntity;

    impl QueryHeap for FetchEntity {
        type Storages = ();
        type HeapIter = RandomAccessVec<Arc<[NMainCell<InertEntity>]>>;
        type Heap<'a> = &'a mut Arc<[NMainCell<InertEntity>]>;

        type BlockIter<'a, N> = RandomAccessSliceRef<'a, [NMainCell<InertEntity>; MultiRefCellIndex::COUNT]>
            where N: 'a + Token;

        type Block<'a, N> = &'a [NMainCell<InertEntity>; MultiRefCellIndex::COUNT]
            where N: 'a + Token;

        fn storages() -> Self::Storages {}

        fn heaps_for_archetype(
            _storages: &Self::Storages,
            archetype: &ArchetypeQueryInfo,
        ) -> Self::HeapIter {
            RandomAccessVec::new(archetype.entities.clone().unwrap())
        }

        fn blocks<'a, N: Token>(heap: Self::Heap<'a>, _token: &'a N) -> Self::BlockIter<'a, N> {
            RandomAccessSliceRef::new(array_chunks(heap))
        }
    }

    // Joined heap
    impl<A: QueryHeap, B: QueryHeap> QueryHeap for (A, B) {
        type Storages = (A::Storages, B::Storages);
        type HeapIter = RandomAccessZip<A::HeapIter, B::HeapIter>;
        type Heap<'a> = (A::Heap<'a>, B::Heap<'a>);

        type BlockIter<'a, N> = RandomAccessZip<A::BlockIter<'a, N>, B::BlockIter<'a, N>>
        where
            N: 'a + Token;

        type Block<'a, N> = (A::Block<'a, N>, B::Block<'a, N>) where N: 'a + Token;

        fn storages() -> Self::Storages {
            (A::storages(), B::storages())
        }

        fn heaps_for_archetype(
            storages: &Self::Storages,
            archetype: &ArchetypeQueryInfo,
        ) -> Self::HeapIter {
            RandomAccessZip::new(
                A::heaps_for_archetype(&storages.0, archetype),
                B::heaps_for_archetype(&storages.1, archetype),
            )
        }

        fn blocks<'a, N: Token>((a, b): Self::Heap<'a>, token: &'a N) -> Self::BlockIter<'a, N> {
            RandomAccessZip::new(A::blocks(a, token), B::blocks(b, token))
        }
    }

    // === QueryGroupBorrow === //

    pub trait QueryGroupBorrow<B, N, L: 'static>: Sized + 'static {
        type Guard<'a>
        where
            B: 'a,
            N: 'a;

        type Iter<'a>: UntiedRandomAccessIter<UntiedItem = Self::IterItem<'a>>
        where
            B: 'a,
            N: 'a;

        type IterItem<'a>
        where
            B: 'a,
            N: 'a;

        fn try_borrow_group<'a>(
            block: &'a B,
            token: &'a N,
            loaner: &'a mut L,
        ) -> Option<Self::Guard<'a>>;

        fn iter<'a, 'g: 'a>(guard: &'a mut Self::Guard<'g>) -> Self::Iter<'a>
        where
            B: 'g,
            N: 'g;
    }

    // SupportedQueryGroupBorrow
    pub struct SupportedQueryGroupBorrow;

    impl<B, N: Token, L: 'static> QueryGroupBorrow<B, N, L> for SupportedQueryGroupBorrow {
        type Guard<'a> = () where B: 'a, N: 'a;

        type Iter<'a> = RandomAccessRepeat<()> where B: 'a, N: 'a;
        type IterItem<'a> = () where B: 'a, N: 'a;

        fn try_borrow_group<'a>(
            _block: &'a B,
            _token: &'a N,
            _loaner: &'a mut L,
        ) -> Option<Self::Guard<'a>> {
            Some(())
        }

        fn iter<'a, 'g: 'a>(_guard: &'a mut Self::Guard<'g>) -> Self::Iter<'a>
        where
            B: 'g,
            N: 'g,
        {
            RandomAccessRepeat::new(())
        }
    }

    // CompRefQueryGroupBorrow
    pub struct CompRefQueryGroupBorrow;

    impl<'b, N: BorrowToken<T>, T: 'static>
        QueryGroupBorrow<HeapSlotBlock<'b, T, N>, N, ImmutableBorrow<T>>
        for CompRefQueryGroupBorrow
    {
        type Guard<'a> = MultiOptRef<'a, T> where 'b: 'a, N: 'a;

        type Iter<'a> = RandomAccessSliceRef<'a, T> where 'b: 'a, N: 'a;

        type IterItem<'a> = &'a T where 'b: 'a, N: 'a;

        fn try_borrow_group<'a>(
            block: &'a HeapSlotBlock<'b, T, N>,
            token: &'a N,
            loaner: &'a mut ImmutableBorrow<T>,
        ) -> Option<Self::Guard<'a>> {
            block.values().try_borrow_all(token, loaner.downgrade_ref())
        }

        fn iter<'a, 'g: 'a>(guard: &'a mut Self::Guard<'g>) -> Self::Iter<'a>
        where
            'b: 'g,
            N: 'g,
        {
            RandomAccessSliceRef::new(&**guard)
        }
    }

    // CompMutQueryGroupBorrow
    pub struct CompMutQueryGroupBorrow;

    impl<'b, N: BorrowMutToken<T>, T: 'static>
        QueryGroupBorrow<HeapSlotBlock<'b, T, N>, N, MutableBorrow<T>> for CompMutQueryGroupBorrow
    {
        type Guard<'a> = MultiOptRefMut<'a, T> where 'b: 'a, N: 'a;

        type Iter<'a> = RandomAccessSliceMut<'a, T> where 'b: 'a, N: 'a;

        type IterItem<'a> = &'a mut T where 'b: 'a, N: 'a;

        fn try_borrow_group<'a>(
            block: &'a HeapSlotBlock<'b, T, N>,
            token: &'a N,
            loaner: &'a mut MutableBorrow<T>,
        ) -> Option<Self::Guard<'a>> {
            block
                .values()
                .try_borrow_all_mut(token, loaner.downgrade_mut())
        }

        fn iter<'a, 'g: 'a>(guard: &'a mut Self::Guard<'g>) -> Self::Iter<'a>
        where
            'b: 'g,
            N: 'g,
        {
            RandomAccessSliceMut::new(&mut **guard)
        }
    }

    // EntityQueryGroupBorrow
    pub struct EntityQueryGroupBorrow;

    impl<'b, N: Token>
        QueryGroupBorrow<&'b [NMainCell<InertEntity>; MultiRefCellIndex::COUNT], N, ()>
        for EntityQueryGroupBorrow
    {
        type Guard<'a> = &'a [NMainCell<InertEntity>; MultiRefCellIndex::COUNT] where 'b: 'a, N: 'a;

        type Iter<'a> = RandomAccessSliceRef<'a, NMainCell<InertEntity>> where 'b: 'a, N: 'a;

        type IterItem<'a> = &'a NMainCell<InertEntity> where 'b: 'a, N: 'a;

        fn try_borrow_group<'a>(
            block: &'a &'b [NMainCell<InertEntity>; MultiRefCellIndex::COUNT],
            _token: &'a N,
            _loaner: &'a mut (),
        ) -> Option<Self::Guard<'a>> {
            Some(block)
        }

        fn iter<'a, 'g: 'a>(block: &'a mut Self::Guard<'g>) -> Self::Iter<'a>
        where
            'b: 'g,
            N: 'g,
        {
            RandomAccessSliceRef::new(&**block)
        }
    }

    // SlotQueryGroupBorrow
    pub struct SlotQueryGroupBorrow;

    impl<'b, T: 'static, N: Token> QueryGroupBorrow<HeapSlotBlock<'b, T, N>, N, ()>
        for SlotQueryGroupBorrow
    {
        type Guard<'a> = HeapSlotBlock<'a, T, N> where 'b: 'a, N: 'a;

        type Iter<'a> = heap_block_slot_iter::Iter<'a, T, N> where 'b: 'a, N: 'a;

        type IterItem<'a> = DirectSlot<'a, T> where 'b: 'a, N: 'a;

        fn try_borrow_group<'a>(
            block: &'a HeapSlotBlock<'b, T, N>,
            _token: &'a N,
            _loaner: &'a mut (),
        ) -> Option<Self::Guard<'a>> {
            Some(*block)
        }

        fn iter<'a, 'g: 'a>(guard: &'a mut Self::Guard<'g>) -> Self::Iter<'a>
        where
            'b: 'g,
            N: 'g,
        {
            guard.slots_expose_random_access()
        }
    }

    // ChainBorrow
    impl<ItemA, ItemB, BA, BB, LA, LB, N> QueryGroupBorrow<(BA, BB), N, (LA, LB)> for (ItemA, ItemB)
    where
        ItemA: QueryGroupBorrow<BA, N, LA>,
        ItemB: QueryGroupBorrow<BB, N, LB>,
        LA: 'static,
        LB: 'static,
    {
        type Guard<'a> = (ItemA::Guard<'a>, ItemB::Guard<'a>)
            where
                (BA, BB): 'a,
                N: 'a;

        type Iter<'a> = RandomAccessZip<ItemA::Iter<'a>, ItemB::Iter<'a>>
            where
                (BA, BB): 'a,
                N: 'a;

        type IterItem<'a> = (ItemA::IterItem<'a>, ItemB::IterItem<'a>)
            where
                (BA, BB): 'a,
                N: 'a;

        fn try_borrow_group<'a>(
            block: &'a (BA, BB),
            token: &'a N,
            loaner: &'a mut (LA, LB),
        ) -> Option<Self::Guard<'a>> {
            Some((
                ItemA::try_borrow_group(&block.0, token, &mut loaner.0)?,
                ItemB::try_borrow_group(&block.1, token, &mut loaner.1)?,
            ))
        }

        fn iter<'a, 'g: 'a>(guard: &'a mut Self::Guard<'g>) -> Self::Iter<'a>
        where
            (BA, BB): 'g,
            N: 'g,
        {
            RandomAccessZip::new(ItemA::iter(&mut guard.0), ItemB::iter(&mut guard.1))
        }
    }

    // === QueryPart === //

    type BlockForQueryPart<'a, Q> =
        <<Q as QueryPart>::Heap as QueryHeap>::Block<'a, MainThreadToken>;

    type IterItemForQueryPart<'a, 'b, Q> = <<Q as QueryPart>::GroupBorrow as QueryGroupBorrow<
        BlockForQueryPart<'a, Q>,
        MainThreadToken,
        <Q as QueryPart>::GroupAutokenLoan,
    >>::IterItem<'b>;

    pub trait QueryPart: Sized {
        type Input<'a>;
        type TagIter: Iterator<Item = RawTag>;

        type Heap: QueryHeap;

        type GroupAutokenLoan: Default + 'static;
        type GroupBorrow: for<'block> QueryGroupBorrow<
            BlockForQueryPart<'block, Self>,
            MainThreadToken,
            Self::GroupAutokenLoan,
        >;

        const NEEDS_ENTITIES: bool;

        fn tags(self) -> Self::TagIter;

        fn elem_from_block_item<'elem>(
            token: &'static MainThreadToken,
            elem: &'elem mut IterItemForQueryPart<Self>,
        ) -> Self::Input<'elem>;

        fn call_slow_borrow<B>(
            token: &'static MainThreadToken,
            block: &BlockForQueryPart<Self>,
            index: MultiRefCellIndex,
            f: impl FnOnce(Self::Input<'_>) -> ControlFlow<B>,
        ) -> ControlFlow<B>;

        fn call_super_slow_borrow<B>(
            storages: &<Self::Heap as QueryHeap>::Storages,
            entity: Entity,
            f: impl FnOnce(Self::Input<'_>) -> ControlFlow<B>,
        ) -> ControlFlow<B>;

        fn covariant_cast_input<'from: 'to, 'to>(src: Self::Input<'from>) -> Self::Input<'to>;

        fn query<B>(
            self,
            extra_tags: impl IntoIterator<Item = RawTag>,
            mut f: impl FnMut(Self::Input<'_>) -> ControlFlow<B>,
        ) -> ControlFlow<B> {
            // Ensure that we're running on the main thread.
            let token = MainThreadToken::acquire_fmt("run a query");

            // Ensure that users cannot flush the database while we're running a query.
            let _guard = borrow_flush_guard();

            // Fetch the storages used by this query.
            let storages = <Self::Heap>::storages();

            // Fetch the archetypes containing our desired intersection of tags.
            let archetypes =
                ArchetypeId::in_intersection(self.tags().chain(extra_tags), Self::NEEDS_ENTITIES)
                    .unwrap_or_default();

            // For each archetype...
            for archetype in archetypes {
                // Fetch the component heaps associated with that archetype.
                let heaps = <Self::Heap>::heaps_for_archetype(&storages, &archetype);
                let mut heaps = RandomAccessZip::new(RandomAccessEnumerate, heaps);

                // For each of those heaps...
                for (heap_i, heap) in heaps.iter() {
                    // Determine the length of this heap. If this is not the last heap in the
                    // archetype, we know that it is entirely full and can set its length to something
                    // really high.
                    let is_last_heap = heap_i == archetype.heap_count() - 1;
                    let heap_len_or_big = if is_last_heap {
                        archetype.last_heap_len()
                    } else {
                        usize::MAX
                    };

                    let complete_heap_block_count_or_big =
                        heap_len_or_big / MultiRefCellIndex::COUNT;

                    // Fetch the blocks comprising it.
                    let mut blocks = <Self::Heap>::blocks(heap, token);
                    let complete_blocks_if_truncated =
                        RandomAccessTake::new(blocks.by_mut(), complete_heap_block_count_or_big);

                    // For each complete block...
                    for block in complete_blocks_if_truncated.into_iter() {
                        // Attempt to run the fast-path...
                        let mut loaner = <Self::GroupAutokenLoan>::default();

                        if let Some(mut block) =
                            <Self::GroupBorrow>::try_borrow_group(&block, token, &mut loaner)
                        {
                            for mut elem in <Self::GroupBorrow>::iter(&mut block).into_iter() {
                                f(Self::elem_from_block_item(token, &mut elem))?;
                            }

                            // N.B. we `continue` here rather than putting the slow path in an `else`
                            // block because the lifetime of the `loaner` borrow extends into both
                            // branches of the `if` expression.
                            continue;
                        }

                        drop(loaner);

                        // Otherwise, run the slow-path.
                        for index in MultiRefCellIndex::iter() {
                            Self::call_slow_borrow(token, &block, index, &mut f);
                        }
                    }

                    // For the last incomplete block...
                    let leftover = heap_len_or_big
                        - complete_heap_block_count_or_big * MultiRefCellIndex::COUNT;

                    if is_last_heap && leftover > 0 {
                        let block = blocks.get(complete_heap_block_count_or_big).unwrap();

                        for index in MultiRefCellIndex::iter().take(leftover) {
                            Self::call_slow_borrow(token, &block, index, &mut f);
                        }
                    }
                }
            }

            ControlFlow::Continue(())
        }
    }

    pub struct EntityQueryPart;

    impl QueryPart for EntityQueryPart {
        type Input<'a> = Entity;
        type TagIter = iter::Empty<RawTag>;
        type Heap = FetchEntity;
        type GroupAutokenLoan = ();
        type GroupBorrow = EntityQueryGroupBorrow;

        const NEEDS_ENTITIES: bool = true;

        fn tags(self) -> Self::TagIter {
            iter::empty()
        }

        fn elem_from_block_item<'elem>(
            token: &'static MainThreadToken,
            elem: &'elem mut &NMainCell<InertEntity>,
        ) -> Self::Input<'elem> {
            elem.get(token).into_dangerous_entity()
        }

        fn call_slow_borrow<B>(
            token: &'static MainThreadToken,
            block: &BlockForQueryPart<Self>,
            index: MultiRefCellIndex,
            f: impl FnOnce(Self::Input<'_>) -> ControlFlow<B>,
        ) -> ControlFlow<B> {
            f(block[index as usize].get(token).into_dangerous_entity())
        }

        fn call_super_slow_borrow<B>(
            _storages: &<Self::Heap as QueryHeap>::Storages,
            entity: Entity,
            f: impl FnOnce(Self::Input<'_>) -> ControlFlow<B>,
        ) -> ControlFlow<B> {
            f(entity)
        }

        fn covariant_cast_input<'from: 'to, 'to>(src: Self::Input<'from>) -> Self::Input<'to> {
            src
        }
    }

    pub struct SlotQueryPart<T: 'static>(pub Tag<T>);

    impl<T: 'static> QueryPart for SlotQueryPart<T> {
        type Input<'a> = Slot<T>;
        type TagIter = iter::Once<RawTag>;
        type Heap = FetchHeap<T>;
        type GroupAutokenLoan = ();
        type GroupBorrow = SlotQueryGroupBorrow;

        const NEEDS_ENTITIES: bool = false;

        fn tags(self) -> Self::TagIter {
            iter::once(self.0.raw())
        }

        fn elem_from_block_item<'elem>(
            _token: &'static MainThreadToken,
            elem: &'elem mut DirectSlot<'_, T>,
        ) -> Self::Input<'elem> {
            elem.slot()
        }

        fn call_slow_borrow<B>(
            _token: &'static MainThreadToken,
            block: &BlockForQueryPart<Self>,
            index: MultiRefCellIndex,
            f: impl FnOnce(Self::Input<'_>) -> ControlFlow<B>,
        ) -> ControlFlow<B> {
            f(block.slot(index).slot())
        }

        fn call_super_slow_borrow<B>(
            storages: &<Self::Heap as QueryHeap>::Storages,
            entity: Entity,
            f: impl FnOnce(Self::Input<'_>) -> ControlFlow<B>,
        ) -> ControlFlow<B> {
            f(storages.get_slot(entity))
        }

        fn covariant_cast_input<'from: 'to, 'to>(src: Self::Input<'from>) -> Self::Input<'to> {
            src
        }
    }

    pub struct ObjQueryPart<T: 'static>(pub Tag<T>);

    impl<T: 'static> QueryPart for ObjQueryPart<T> {
        type Input<'a> = Obj<T>;
        type TagIter = iter::Once<RawTag>;
        type Heap = (FetchEntity, FetchHeap<T>);
        type GroupAutokenLoan = ((), ());
        type GroupBorrow = (EntityQueryGroupBorrow, SlotQueryGroupBorrow);

        const NEEDS_ENTITIES: bool = true;

        fn tags(self) -> Self::TagIter {
            iter::once(self.0.raw())
        }

        fn elem_from_block_item<'elem>(
            token: &'static MainThreadToken,
            (entity, slot): &'elem mut (&NMainCell<InertEntity>, DirectSlot<'_, T>),
        ) -> Self::Input<'elem> {
            Obj::from_raw_parts(entity.get(token).into_dangerous_entity(), slot.slot())
        }

        fn call_slow_borrow<B>(
            token: &'static MainThreadToken,
            (entities, slots): &BlockForQueryPart<Self>,
            index: MultiRefCellIndex,
            f: impl FnOnce(Self::Input<'_>) -> ControlFlow<B>,
        ) -> ControlFlow<B> {
            let entity = &entities[index as usize];
            let slot = slots.slot(index);

            f(Obj::from_raw_parts(
                entity.get(token).into_dangerous_entity(),
                slot.slot(),
            ))
        }

        fn call_super_slow_borrow<B>(
            storages: &<Self::Heap as QueryHeap>::Storages,
            entity: Entity,
            f: impl FnOnce(Self::Input<'_>) -> ControlFlow<B>,
        ) -> ControlFlow<B> {
            f(Obj::from_raw_parts(entity, storages.1.get_slot(entity)))
        }

        fn covariant_cast_input<'from: 'to, 'to>(src: Self::Input<'from>) -> Self::Input<'to> {
            src
        }
    }

    pub struct ORefQueryPart<T: 'static>(pub Tag<T>);

    impl<T: 'static> QueryPart for ORefQueryPart<T> {
        type Input<'a> = CompRef<'a, T>;
        type TagIter = iter::Once<RawTag>;
        type Heap = (FetchEntity, FetchHeap<T>);
        type GroupAutokenLoan = ((), ());
        type GroupBorrow = (EntityQueryGroupBorrow, SlotQueryGroupBorrow);

        const NEEDS_ENTITIES: bool = true;

        fn tags(self) -> Self::TagIter {
            iter::once(self.0.raw())
        }

        fn elem_from_block_item<'elem>(
            token: &'static MainThreadToken,
            (entity, slot): &'elem mut (&NMainCell<InertEntity>, DirectSlot<'_, T>),
        ) -> Self::Input<'elem> {
            CompRef::new(
                Obj::from_raw_parts(entity.get(token).into_dangerous_entity(), slot.slot()),
                slot.borrow(token),
            )
        }

        fn call_slow_borrow<B>(
            token: &'static MainThreadToken,
            (entities, slots): &BlockForQueryPart<Self>,
            index: MultiRefCellIndex,
            f: impl FnOnce(Self::Input<'_>) -> ControlFlow<B>,
        ) -> ControlFlow<B> {
            let entity = &entities[index as usize];
            let slot = slots.slot(index);

            f(CompRef::new(
                Obj::from_raw_parts(entity.get(token).into_dangerous_entity(), slot.slot()),
                slot.borrow(token),
            ))
        }

        fn call_super_slow_borrow<B>(
            storages: &<Self::Heap as QueryHeap>::Storages,
            entity: Entity,
            f: impl FnOnce(Self::Input<'_>) -> ControlFlow<B>,
        ) -> ControlFlow<B> {
            f(storages.1.get(entity))
        }

        fn covariant_cast_input<'from: 'to, 'to>(src: Self::Input<'from>) -> Self::Input<'to> {
            src
        }
    }

    pub struct OMutQueryPart<T: 'static>(pub Tag<T>);

    impl<T: 'static> QueryPart for OMutQueryPart<T> {
        type Input<'a> = CompMut<'a, T>;
        type TagIter = iter::Once<RawTag>;
        type Heap = (FetchEntity, FetchHeap<T>);
        type GroupAutokenLoan = ((), ());
        type GroupBorrow = (EntityQueryGroupBorrow, SlotQueryGroupBorrow);

        const NEEDS_ENTITIES: bool = true;

        fn tags(self) -> Self::TagIter {
            iter::once(self.0.raw())
        }

        fn elem_from_block_item<'elem>(
            token: &'static MainThreadToken,
            (entity, slot): &'elem mut (&NMainCell<InertEntity>, DirectSlot<'_, T>),
        ) -> Self::Input<'elem> {
            CompMut::new(
                Obj::from_raw_parts(entity.get(token).into_dangerous_entity(), slot.slot()),
                slot.borrow_mut(token),
            )
        }

        fn call_slow_borrow<B>(
            token: &'static MainThreadToken,
            (entities, slots): &BlockForQueryPart<Self>,
            index: MultiRefCellIndex,
            f: impl FnOnce(Self::Input<'_>) -> ControlFlow<B>,
        ) -> ControlFlow<B> {
            let entity = &entities[index as usize];
            let slot = slots.slot(index);

            f(CompMut::new(
                Obj::from_raw_parts(entity.get(token).into_dangerous_entity(), slot.slot()),
                slot.borrow_mut(token),
            ))
        }

        fn call_super_slow_borrow<B>(
            storages: &<Self::Heap as QueryHeap>::Storages,
            entity: Entity,
            f: impl FnOnce(Self::Input<'_>) -> ControlFlow<B>,
        ) -> ControlFlow<B> {
            f(storages.1.get_mut(entity))
        }

        fn covariant_cast_input<'from: 'to, 'to>(src: Self::Input<'from>) -> Self::Input<'to> {
            src
        }
    }

    pub struct RefQueryPart<T: 'static>(pub Tag<T>);

    impl<T: 'static> QueryPart for RefQueryPart<T> {
        type Input<'a> = &'a T;
        type TagIter = iter::Once<RawTag>;
        type Heap = FetchHeap<T>;
        type GroupAutokenLoan = ImmutableBorrow<T>;
        type GroupBorrow = CompRefQueryGroupBorrow;

        const NEEDS_ENTITIES: bool = false;

        fn tags(self) -> Self::TagIter {
            iter::once(self.0.raw())
        }

        fn elem_from_block_item<'elem>(
            _token: &'static MainThreadToken,
            elem: &'elem mut &T,
        ) -> Self::Input<'elem> {
            elem
        }

        fn call_slow_borrow<B>(
            token: &'static MainThreadToken,
            block: &BlockForQueryPart<Self>,
            index: MultiRefCellIndex,
            f: impl FnOnce(Self::Input<'_>) -> ControlFlow<B>,
        ) -> ControlFlow<B> {
            f(&block.values().borrow(token, index))
        }

        fn call_super_slow_borrow<B>(
            storages: &<Self::Heap as QueryHeap>::Storages,
            entity: Entity,
            f: impl FnOnce(Self::Input<'_>) -> ControlFlow<B>,
        ) -> ControlFlow<B> {
            f(&storages.get(entity))
        }

        fn covariant_cast_input<'from: 'to, 'to>(src: Self::Input<'from>) -> Self::Input<'to> {
            src
        }
    }

    pub struct MutQueryPart<T: 'static>(pub Tag<T>);

    impl<T: 'static> QueryPart for MutQueryPart<T> {
        type Input<'a> = &'a mut T;
        type TagIter = iter::Once<RawTag>;
        type Heap = FetchHeap<T>;
        type GroupAutokenLoan = MutableBorrow<T>;
        type GroupBorrow = CompMutQueryGroupBorrow;

        const NEEDS_ENTITIES: bool = false;

        fn tags(self) -> Self::TagIter {
            iter::once(self.0.raw())
        }

        fn elem_from_block_item<'elem>(
            _token: &'static MainThreadToken,
            elem: &'elem mut &mut T,
        ) -> Self::Input<'elem> {
            elem
        }

        fn call_slow_borrow<B>(
            token: &'static MainThreadToken,
            block: &BlockForQueryPart<Self>,
            index: MultiRefCellIndex,
            f: impl FnOnce(Self::Input<'_>) -> ControlFlow<B>,
        ) -> ControlFlow<B> {
            f(&mut block.values().borrow_mut(token, index))
        }

        fn call_super_slow_borrow<B>(
            storages: &<Self::Heap as QueryHeap>::Storages,
            entity: Entity,
            f: impl FnOnce(Self::Input<'_>) -> ControlFlow<B>,
        ) -> ControlFlow<B> {
            f(&mut storages.get_mut(entity))
        }

        fn covariant_cast_input<'from: 'to, 'to>(src: Self::Input<'from>) -> Self::Input<'to> {
            src
        }
    }

    impl<A: QueryPart, B: QueryPart> QueryPart for (A, B) {
        type Input<'a> = (A::Input<'a>, B::Input<'a>);
        type Heap = (A::Heap, B::Heap);
        type TagIter = iter::Chain<A::TagIter, B::TagIter>;
        type GroupAutokenLoan = (A::GroupAutokenLoan, B::GroupAutokenLoan);
        type GroupBorrow = (A::GroupBorrow, B::GroupBorrow);

        const NEEDS_ENTITIES: bool = A::NEEDS_ENTITIES || B::NEEDS_ENTITIES;

        fn tags(self) -> Self::TagIter {
            self.0.tags().chain(self.1.tags())
        }

        fn elem_from_block_item<'elem, 'guard>(
            token: &'static MainThreadToken,
            elem: &'elem mut IterItemForQueryPart<Self>,
        ) -> Self::Input<'elem> {
            (
                A::elem_from_block_item(token, &mut elem.0),
                B::elem_from_block_item(token, &mut elem.1),
            )
        }

        fn call_slow_borrow<Br>(
            token: &'static MainThreadToken,
            block: &BlockForQueryPart<Self>,
            index: MultiRefCellIndex,
            f: impl FnOnce(Self::Input<'_>) -> ControlFlow<Br>,
        ) -> ControlFlow<Br> {
            A::call_slow_borrow(token, &block.0, index, |a| {
                B::call_slow_borrow(token, &block.1, index, |b| {
                    f((A::covariant_cast_input(a), B::covariant_cast_input(b)))
                })
            })
        }

        fn call_super_slow_borrow<Br>(
            storages: &<Self::Heap as QueryHeap>::Storages,
            entity: Entity,
            f: impl FnOnce(Self::Input<'_>) -> ControlFlow<Br>,
        ) -> ControlFlow<Br> {
            A::call_super_slow_borrow(&storages.0, entity, |a| {
                B::call_super_slow_borrow(&storages.1, entity, |b| {
                    f((A::covariant_cast_input(a), B::covariant_cast_input(b)))
                })
            })
        }

        fn covariant_cast_input<'from: 'to, 'to>(src: Self::Input<'from>) -> Self::Input<'to> {
            (
                A::covariant_cast_input(src.0),
                B::covariant_cast_input(src.1),
            )
        }
    }

    impl QueryPart for () {
        type Input<'a> = ();
        type TagIter = iter::Empty<RawTag>;
        type GroupAutokenLoan = ();
        type Heap = ();
        type GroupBorrow = SupportedQueryGroupBorrow;

        const NEEDS_ENTITIES: bool = false;

        fn tags(self) -> Self::TagIter {
            iter::empty()
        }

        fn elem_from_block_item<'elem>(
            _token: &'static MainThreadToken,
            _elem: &'elem mut (),
        ) -> Self::Input<'elem> {
        }

        fn call_slow_borrow<B>(
            _token: &'static MainThreadToken,
            _block: &BlockForQueryPart<Self>,
            _index: MultiRefCellIndex,
            f: impl FnOnce(Self::Input<'_>) -> ControlFlow<B>,
        ) -> ControlFlow<B> {
            f(())
        }

        fn call_super_slow_borrow<Br>(
            _storages: &<Self::Heap as QueryHeap>::Storages,
            _entity: Entity,
            f: impl FnOnce(Self::Input<'_>) -> ControlFlow<Br>,
        ) -> ControlFlow<Br> {
            f(())
        }

        fn covariant_cast_input<'from: 'to, 'to>(src: Self::Input<'from>) -> Self::Input<'to> {
            src
        }
    }

    // === Driven Queries === //

    pub fn run_driven_query<K: QueryKey, P: QueryPart, M: MultiQueryDriver, B>(
        key: K,
        part: P,
        extra_tags: impl IntoIterator<Item = RawTag>,
        driver: &M,
        f: impl FnMut((P::Input<'_>, MultiDriverItem<'_, M>)) -> ControlFlow<B>,
    ) -> ControlFlow<B> {
        driver.drive_multi_query(
            &mut QueryDriverTargetInstance::<K, P> {
                _ty: PhantomData,
                key,
                tags: part.tags().chain(extra_tags).collect(),
            },
            f,
        )
    }

    struct QueryDriverTargetInstance<K: QueryKey, P: QueryPart> {
        _ty: PhantomData<fn() -> P>,
        key: K,
        tags: Vec<RawTag>,
    }

    impl<K: QueryKey, P: QueryPart> DriverTargetSealed for QueryDriverTargetInstance<K, P> {}

    impl<K: QueryKey, P: QueryPart> QueryDriverTarget for QueryDriverTargetInstance<K, P> {
        type Input<'a> = P::Input<'a>;

        fn handle_driver<D: QueryDriver, B>(
            &mut self,
            driver: &D,
            mut f: impl FnMut((Self::Input<'_>, DriverItem<'_, D>)) -> ControlFlow<B>,
        ) -> ControlFlow<B> {
            // Ensure that we're running on the main thread.
            let token = MainThreadToken::acquire_fmt("run a query");

            // Ensure that users cannot flush the database while we're running a query.
            let _guard = borrow_flush_guard();

            // Fetch the storages used by this query.
            let storages = <P::Heap>::storages();

            driver.drive_query(
                self.key.clone(),
                self.tags.clone(),
                P::NEEDS_ENTITIES,
                QueryDriveEntryHandlerInstance(
                    PhantomData,
                    &mut f,
                    |f, archetype, mut userdata| {
                        // Fetch the component heaps associated with that archetype.
                        let mut heaps = <P::Heap>::heaps_for_archetype(&storages, archetype);

                        // For each of those heaps...
                        driver.foreach_heap(
                            archetype,
                            &mut userdata,
                            QueryHeapHandlerInstance(PhantomData, |heap_i, mut userdata| {
                                let heap = heaps.get(heap_i).unwrap();

                                // Fetch the blocks comprising it.
                                let mut blocks = <P::Heap>::blocks(heap, token);

                                // For each block...
                                #[rustfmt::skip]
                                driver.foreach_block(
                                    heap_i,
                                    blocks.len(),
                                    &mut userdata,
                                    QueryBlockHandlerInstance(PhantomData, |block_i, mut userdata| 'pb: {
                                        let block = blocks.get(heap_i).unwrap();

                                        // Attempt to run the fast-path...
                                        let mut loaner = <P::GroupAutokenLoan>::default();

                                        if let Some(mut block) =
                                            <P::GroupBorrow>::try_borrow_group(
                                                &block,
                                                token,
                                                &mut loaner,
                                            )
                                        {
                                            let mut block = <P::GroupBorrow>::iter(&mut block);

                                            driver.foreach_element_in_full_block(
                                                block_i,
                                                &mut userdata,
                                                QueryBlockElementHandlerInstance(PhantomData, |index, item| {
                                                    f((
                                                        P::elem_from_block_item(
                                                            token,
                                                            &mut block
                                                                .get(index as usize)
                                                                .unwrap(),
                                                        ),
                                                        item,
                                                    ))
                                                }),
                                            )?;

                                            // N.B. we `break` here rather than putting the slow path in an `else`
                                            // block because the lifetime of the `loaner` borrow extends into both
                                            // branches of the `if` expression.
                                            break 'pb ControlFlow::Continue(());
                                        }

                                        // Otherwise, run the slow-path.
                                        driver.foreach_element_in_semi_block(
                                            block_i,
                                            &mut userdata,
                                            QueryBlockElementHandlerInstance(PhantomData, |index, item| {
                                                P::call_slow_borrow(
                                                    token,
                                                    &block,
                                                    index,
                                                    |args| f((args, item)),
                                                )
                                            }),
                                        )
                                    }),
                                )?;

                                ControlFlow::Continue(())
                            }),
                        )
                    },
                    |f, entity, item| {
                        P::call_super_slow_borrow(&storages, entity, |args| f((args, item)))
                    },
                ),
            )
        }
    }

    // === Helpers === //

    pub fn get_tag<T: 'static + HasGlobalManagedTag>() -> Tag<T::Component> {
        Tag::global::<T>()
    }

    pub fn from_tag<T: 'static>(tag: impl Into<Tag<T>>) -> Tag<T> {
        tag.into()
    }

    pub fn from_tag_virtual(tag: impl Into<RawTag>) -> RawTag {
        tag.into()
    }

    pub fn empty_tag_iter() -> impl Iterator<Item = RawTag> {
        [].into_iter()
    }

    pub trait ExtractRefOfMultiQueryDriver: MultiQueryDriver {
        fn __extract_ref_of_multi_query_driver(&self) -> &Self;
    }

    impl<T: ?Sized + MultiQueryDriver> ExtractRefOfMultiQueryDriver for T {
        fn __extract_ref_of_multi_query_driver(&self) -> &Self {
            self
        }
    }
}

#[macro_export]
macro_rules! query {
    // Entrypoints
    (
        for ($($input:tt)*) {
            $($body:tt)*
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($input)*};
                bound_event = {};
                built_parts = {()};
                built_extractor = {()};
                extra_tags = {$crate::query::query_internals::empty_tag_iter()};
                body = {$($body)*};
            }
        }
    };

    // Recursion base cases
    (
        @internal {
            remaining_input = {};
            bound_event = {};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query_internals::cbit!(
            for $extractor in $crate::query::query_internals::QueryPart::query($parts, $extra_tags) {
                $($body)*
            }
        )
    };
    (
        @internal {
            remaining_input = {};
            bound_event = {$name:pat in $driver:expr};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {{
        #[allow(unused_import)]
        use $crate::query::query_internals::ExtractRefOfMultiQueryDriver;

        $crate::query::query_internals::cbit!(
            for ($extractor, $name) in $crate::query::query_internals::run_driven_query(
                {
                    #[derive(Copy, Clone, Hash, Eq, PartialEq)]
                    struct MyQueryKey;
                    MyQueryKey
                },
                $parts,
                $extra_tags,
                $driver.__extract_ref_of_multi_query_driver(),
            ) {
                $($body)*
            }
        )
    }};
    (
        @internal {
            remaining_input = {};
            bound_event = {$($invalid:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query_internals::compile_error!($crate::query::query_internals::concat!(
            "internal error: invalid `bound_event` (tokens: `",
            $crate::query::query_internals::stringify!($($invalid)*),
            "`)"
        ));
    };

    // event
    (
        @internal {
            remaining_input = {event $name:pat in $driver:expr $(, $($rest:tt)*)?};
            bound_event = {};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
                bound_event = {$name in $driver};
                built_parts = {$parts};
                built_extractor = {$extractor};
                extra_tags = {$extra_tags};
                body = {$($body)*};
            }
        }
    };
    (
        @internal {
            remaining_input = {event $name:pat in $driver:expr $(, $($rest:tt)*)?};
            bound_event = {$($anything:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query_internals::compile_error!("`query!` can have at most one event driver");
    };
    (
        @internal {
            remaining_input = {event $($anything:tt)*};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query_internals::compile_error!(
            $crate::query::query_internals::concat!(
                "expected `$name in $driver` after `event`; got `",
                $crate::query::query_internals::stringify!($($anything)*),
                "`"
            ),
        );
    };

    // entity
    (
        @internal {
            remaining_input = {entity $name:pat $(, $($rest:tt)*)?};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
                bound_event = {$($bound_event)*};
                built_parts = {($parts, $crate::query::query_internals::EntityQueryPart)};
                built_extractor = {($extractor, $name)};
                extra_tags = {$extra_tags};
                body = {$($body)*};
            }
        }
    };
    (
        @internal {
            remaining_input = {entity $($anything:tt)*};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query_internals::compile_error!(
            $crate::query::query_internals::concat!(
                "expected a pattern after `entity`; got `",
                $crate::query::query_internals::stringify!($($anything)*),
                "`"
            ),
        );
    };

    // `slot`
    (
        @internal {
            remaining_input = {slot $name:ident : $ty:ty $(, $($rest:tt)*)?};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
                bound_event = {$($bound_event)*};
                built_parts = {($parts, $crate::query::query_internals::SlotQueryPart(
                    $crate::query::query_internals::get_tag::<$ty>(),
                ))};
                built_extractor = {($extractor, $name)};
                extra_tags = {$extra_tags};
                body = {$($body)*};
            }
        }
    };
    (
        @internal {
            remaining_input = {slot $name:ident in $tag:expr $(, $($rest:tt)*)?};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
                bound_event = {$($bound_event)*};
                built_parts = {($parts, $crate::query::query_internals::SlotQueryPart(
                    $crate::query::query_internals::from_tag($tag),
                ))};
                built_extractor = {($extractor, $name)};
                extra_tags = {$extra_tags};
                body = {$($body)*};
            }
        }
    };

    // `slot` error handling
    (
        @internal {
            remaining_input = {slot $name:ident $($anything:tt)*};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query_internals::compile_error!(
            $crate::query::query_internals::concat!(
                "expected a global type tag in the form `slot ",
                $crate::query::query_internals::stringify!($name),
                ": <type>` or a tag expression in the form `slot ",
                $crate::query::query_internals::stringify!($name),
                " in <expr>` but instead got `",
                $crate::query::query_internals::stringify!($($anything)*),
                "`"
            ),
        );
    };
    (
        @internal {
            remaining_input = {slot $($anything:tt)*};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query_internals::compile_error!(
            $crate::query::query_internals::concat!(
                "expected an identifier after `slot`; got `",
                $crate::query::query_internals::stringify!($($anything)*),
                "`"
            ),
        );
    };

    // `obj`
    (
        @internal {
            remaining_input = {obj $name:ident : $ty:ty $(, $($rest:tt)*)?};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
                bound_event = {$($bound_event)*};
                built_parts = {($parts, $crate::query::query_internals::ObjQueryPart(
                    $crate::query::query_internals::get_tag::<$ty>(),
                ))};
                built_extractor = {($extractor, $name)};
                extra_tags = {$extra_tags};
                body = {$($body)*};
            }
        }
    };
    (
        @internal {
            remaining_input = {obj $name:ident in $tag:expr $(, $($rest:tt)*)?};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
                bound_event = {$($bound_event)*};
                built_parts = {($parts, $crate::query::query_internals::ObjQueryPart(
                    $crate::query::query_internals::from_tag($tag),
                ))};
                built_extractor = {($extractor, $name)};
                extra_tags = {$extra_tags};
                body = {$($body)*};
            }
        }
    };

    // `obj` error handling
    (
        @internal {
            remaining_input = {obj $name:ident $($anything:tt)*};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query_internals::compile_error!(
            $crate::query::query_internals::concat!(
                "expected a global type tag in the form `obj ",
                $crate::query::query_internals::stringify!($name),
                ": <type>` or a tag expression in the form `obj ",
                $crate::query::query_internals::stringify!($name),
                " in <expr>` but instead got `",
                $crate::query::query_internals::stringify!($($anything)*),
                "`"
            ),
        );
    };
    (
        @internal {
            remaining_input = {obj $($anything:tt)*};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query_internals::compile_error!(
            $crate::query::query_internals::concat!(
                "expected an identifier after `obj`; got `",
                $crate::query::query_internals::stringify!($($anything)*),
                "`"
            ),
        );
    };

    // `ref`
    (
        @internal {
            remaining_input = {ref $name:ident : $ty:ty $(, $($rest:tt)*)?};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
                bound_event = {$($bound_event)*};
                built_parts = {($parts, $crate::query::query_internals::RefQueryPart(
                    $crate::query::query_internals::get_tag::<$ty>(),
                ))};
                built_extractor = {($extractor, $name)};
                extra_tags = {$extra_tags};
                body = {$($body)*};
            }
        }
    };
    (
        @internal {
            remaining_input = {ref $name:ident in $tag:expr $(, $($rest:tt)*)?};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
                bound_event = {$($bound_event)*};
                built_parts = {($parts, $crate::query::query_internals::RefQueryPart(
                    $crate::query::query_internals::from_tag($tag),
                ))};
                built_extractor = {($extractor, $name)};
                extra_tags = {$extra_tags};
                body = {$($body)*};
            }
        }
    };

    // `ref` error handling
    (
        @internal {
            remaining_input = {ref $name:ident $($anything:tt)*};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query_internals::compile_error!(
            $crate::query::query_internals::concat!(
                "expected a global type tag in the form `ref ",
                $crate::query::query_internals::stringify!($name),
                ": <type>` or a tag expression in the form `ref ",
                $crate::query::query_internals::stringify!($name),
                " in <expr>` but instead got `",
                $crate::query::query_internals::stringify!($($anything)*),
                "`"
            ),
        );
    };
    (
        @internal {
            remaining_input = {ref $($anything:tt)*};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query_internals::compile_error!(
            $crate::query::query_internals::concat!(
                "expected an identifier after `ref`; got `",
                $crate::query::query_internals::stringify!($($anything)*),
                "`"
            ),
        );
    };

    // `mut`
    (
        @internal {
            remaining_input = {mut $name:ident : $ty:ty $(, $($rest:tt)*)?};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
                bound_event = {$($bound_event)*};
                built_parts = {($parts, $crate::query::query_internals::MutQueryPart(
                    $crate::query::query_internals::get_tag::<$ty>(),
                ))};
                built_extractor = {($extractor, $name)};
                extra_tags = {$extra_tags};
                body = {$($body)*};
            }
        }
    };
    (
        @internal {
            remaining_input = {mut $name:ident in $tag:expr $(, $($rest:tt)*)?};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
                bound_event = {$($bound_event)*};
                built_parts = {($parts, $crate::query::query_internals::MutQueryPart(
                    $crate::query::query_internals::from_tag($tag),
                ))};
                built_extractor = {($extractor, $name)};
                extra_tags = {$extra_tags};
                body = {$($body)*};
            }
        }
    };

    // `mut` error handling
    (
        @internal {
            remaining_input = {mut $name:ident $($anything:tt)*};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query_internals::compile_error!(
            $crate::query::query_internals::concat!(
                "expected a global type tag in the form `mut ",
                $crate::query::query_internals::stringify!($name),
                ": <type>` or a tag expression in the form `mut ",
                $crate::query::query_internals::stringify!($name),
                " in <expr>` but instead got `",
                $crate::query::query_internals::stringify!($($anything)*),
                "`"
            ),
        );
    };
    (
        @internal {
            remaining_input = {mut $($anything:tt)*};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query_internals::compile_error!(
            $crate::query::query_internals::concat!(
                "expected an identifier after `mut`; got `",
                $crate::query::query_internals::stringify!($($anything)*),
                "`"
            ),
        );
    };

    // `oref`
    (
        @internal {
            remaining_input = {oref $name:ident : $ty:ty $(, $($rest:tt)*)?};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
                bound_event = {$($bound_event)*};
                built_parts = {($parts, $crate::query::query_internals::ORefQueryPart(
                    $crate::query::query_internals::get_tag::<$ty>(),
                ))};
                built_extractor = {($extractor, $name)};
                extra_tags = {$extra_tags};
                body = {$($body)*};
            }
        }
    };
    (
        @internal {
            remaining_input = {oref $name:ident in $tag:expr $(, $($rest:tt)*)?};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
                bound_event = {$($bound_event)*};
                built_parts = {($parts, $crate::query::query_internals::ORefQueryPart(
                    $crate::query::query_internals::from_tag($tag),
                ))};
                built_extractor = {($extractor, $name)};
                extra_tags = {$extra_tags};
                body = {$($body)*};
            }
        }
    };

    // `oref` error handling
    (
        @internal {
            remaining_input = {oref $name:ident $($anything:tt)*};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query_internals::compile_error!(
            $crate::query::query_internals::concat!(
                "expected a global type tag in the form `oref ",
                $crate::query::query_internals::stringify!($name),
                ": <type>` or a tag expression in the form `oref ",
                $crate::query::query_internals::stringify!($name),
                " in <expr>` but instead got `",
                $crate::query::query_internals::stringify!($($anything)*),
                "`"
            ),
        );
    };
    (
        @internal {
            remaining_input = {oref $($anything:tt)*};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query_internals::compile_error!(
            $crate::query::query_internals::concat!(
                "expected an identifier after `oref`; got `",
                $crate::query::query_internals::stringify!($($anything)*),
                "`"
            ),
        );
    };

    // `omut`
    (
        @internal {
            remaining_input = {omut $name:ident : $ty:ty $(, $($rest:tt)*)?};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
                bound_event = {$($bound_event)*};
                built_parts = {($parts, $crate::query::query_internals::OMutQueryPart(
                    $crate::query::query_internals::get_tag::<$ty>(),
                ))};
                built_extractor = {($extractor, mut $name)};
                extra_tags = {$extra_tags};
                body = {$($body)*};
            }
        }
    };
    (
        @internal {
            remaining_input = {omut $name:ident in $tag:expr $(, $($rest:tt)*)?};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
                bound_event = {$($bound_event)*};
                built_parts = {($parts, $crate::query::query_internals::OMutQueryPart(
                    $crate::query::query_internals::from_tag($tag),
                ))};
                built_extractor = {($extractor, mut $name)};
                extra_tags = {$extra_tags};
                body = {$($body)*};
            }
        }
    };

    // `omut` error handling
    (
        @internal {
            remaining_input = {omut $name:ident $($anything:tt)*};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query_internals::compile_error!(
            $crate::query::query_internals::concat!(
                "expected a global type tag in the form `omut ",
                $crate::query::query_internals::stringify!($name),
                ": <type>` or a tag expression in the form `omut ",
                $crate::query::query_internals::stringify!($name),
                " in <expr>` but instead got `",
                $crate::query::query_internals::stringify!($($anything)*),
                "`"
            ),
        );
    };
    (
        @internal {
            remaining_input = {omut $($anything:tt)*};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query_internals::compile_error!(
            $crate::query::query_internals::concat!(
                "expected an identifier after `omut`; got `",
                $crate::query::query_internals::stringify!($($anything)*),
                "`"
            ),
        );
    };

    // Tags
    (
        @internal {
            remaining_input = {tags $tag:expr $(, $($rest:tt)*)?};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
                bound_event = {$($bound_event)*};
                built_parts = {$parts};
                built_extractor = {$extractor};
                extra_tags = {$crate::query::query_internals::Iterator::join(
                    $extra_tags,
                    $tag,
                )};
                body = {$($body)*};
            }
        }
    };
    (
        @internal {
            remaining_input = {tag $tag:expr $(, $($rest:tt)*)?};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
                bound_event = {$($bound_event)*};
                built_parts = {$parts};
                built_extractor = {$extractor};
                extra_tags = {$crate::query::query_internals::Iterator::chain(
                    $extra_tags,
                    [$crate::query::query_internals::from_tag_virtual($tag)],
                )};
                body = {$($body)*};
            }
        }
    };

    // Tags error handling
    (
        @internal {
            remaining_input = {tags $($anything:tt)*};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query_internals::compile_error!(
            $crate::query::query_internals::concat!(
                "expected an expression after `tags`; got `",
                $crate::query::query_internals::stringify!($($anything)*),
                "`"
            ),
        );
    };
    (
        @internal {
            remaining_input = {tag $($anything:tt)*};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query_internals::compile_error!(
            $crate::query::query_internals::concat!(
                "expected an expression after `tag`; got `",
                $crate::query::query_internals::stringify!($($anything)*),
                "`"
            ),
        );
    };

    // General error handling
    (
        @internal {
            remaining_input = {$($anything:tt)*};
            bound_event = {$($bound_event:tt)*};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query_internals::compile_error!(
            $crate::query::query_internals::concat!(
                "expected `event`, `entity`, `slot`, `obj`, `ref`, `mut`, `oref`, `omut`, `tag`, or \
                 `tags`; got `",
                $crate::query::query_internals::stringify!($($anything)*),
                "`"
            ),
        );
    };
}

pub use query;
