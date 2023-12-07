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
    fn reify_tag_list<R>(
        tags: impl IntoIterator<Item = RawTag>,
        f: impl FnOnce(ReifiedTagList<'_>) -> R,
    ) -> R {
        let mut tags = tags.into_iter().map(|tag| tag.0);
        let static_tags: [_; 8] = std::array::from_fn(|_| tags.next());
        let dynamic_tags = tags.collect::<Vec<_>>();

        f(ReifiedTagList {
            static_tags: &static_tags,
            dynamic_tags: &dynamic_tags,
        })
    }

    pub fn in_intersection(
        tags: impl IntoIterator<Item = RawTag>,
        include_entities: bool,
    ) -> Vec<ArchetypeQueryInfo> {
        let token = MainThreadToken::acquire_fmt("enumerate archetypes in a tag intersection");

        let mut archetypes = Vec::new();
        Self::reify_tag_list(tags, |tags| {
            DbRoot::get(token).enumerate_tag_intersection(tags, |info| {
                archetypes.push(ArchetypeQueryInfo {
                    archetype: info.archetype.into_dangerous_archetype_id(),
                    heap_count: info.entities.len(),
                    last_heap_len: info.last_heap_len,
                    entities: include_entities.then(|| info.entities.clone()),
                });
            })
        });

        archetypes
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

#[derive_where(Default)]
pub struct QueryVersionMap<V> {
    versions: FxHashMap<ReifiedQueryKey, V>,
}

struct ReifiedQueryKey {
    hash: u64,
    value: Box<dyn Any + Send + Sync>,
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

pub trait QueryKey: 'static + Sized + Send + Sync + Hash + PartialEq {}

impl<T: 'static + Send + Sync + Hash + PartialEq> QueryKey for T {}

// === Query Drivers === //

pub trait QueryDriver {
    #[rustfmt::skip]
    type Item<'a> where Self: 'a;

    #[rustfmt::skip]
    type ArchetypeIterInfo<'a> where Self: 'a;

    #[rustfmt::skip]
    type HeapIterInfo<'a> where Self: 'a;

    #[rustfmt::skip]
    type BlockIterInfo<'a> where Self: 'a;

    fn drive_query<B>(
        &self,
        query_key: impl QueryKey,
        tags: impl IntoIterator<Item = RawTag>,
        include_entities: bool,
        process_arch: impl FnMut(&ArchetypeQueryInfo, Self::ArchetypeIterInfo<'_>) -> ControlFlow<B>,
        process_arbitrary: impl FnMut(Entity) -> ControlFlow<B>,
    ) -> ControlFlow<B>;

    fn foreach_heap<B>(
        &self,
        arch: &ArchetypeQueryInfo,
        arch_userdata: &mut Self::ArchetypeIterInfo<'_>,
        process: impl FnMut(usize, Self::HeapIterInfo<'_>) -> ControlFlow<B>,
    ) -> ControlFlow<B>;

    fn foreach_block<B>(
        &self,
        heap: usize,
        heap_userdata: &mut Self::HeapIterInfo<'_>,
        process: impl FnMut(usize, Self::BlockIterInfo<'_>) -> ControlFlow<B>,
    ) -> ControlFlow<B>;

    fn foreach_element_in_full_block<B>(
        &self,
        block: usize,
        block_userdata: &mut Self::BlockIterInfo<'_>,
        process: impl FnMut(MultiRefCellIndex, Self::Item<'_>) -> ControlFlow<B>,
    ) -> ControlFlow<B>;

    fn foreach_element_in_semi_block<B>(
        &self,
        block: usize,
        block_userdata: &mut Self::BlockIterInfo<'_>,
        process: impl FnMut(MultiRefCellIndex, Self::Item<'_>) -> ControlFlow<B>,
    ) -> ControlFlow<B>;
}

// === Query Macro === //

#[doc(hidden)]
pub mod query_internals {
    use std::{iter, ops::ControlFlow, sync::Arc};

    use autoken::{ImmutableBorrow, MutableBorrow};

    use crate::{
        core::{
            cell::{MultiOptRef, MultiOptRefMut, MultiRefCellIndex},
            heap::{
                array_chunks, heap_block_iter, heap_block_slot_iter, DirectSlot, Heap,
                HeapSlotBlock,
            },
            random_iter::{
                RandomAccessIter, RandomAccessRepeat, RandomAccessSliceMut, RandomAccessSliceRef,
                RandomAccessTake, RandomAccessZip,
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
        borrow_flush_guard, ArchetypeId, ArchetypeQueryInfo, HasGlobalManagedTag, RawTag, Tag,
    };

    pub use {
        cbit::cbit,
        std::{compile_error, concat, iter::Iterator, stringify},
    };

    // === QueryHeap === //

    /// A heap over which a [`QueryPart`] can iterate.
    pub trait QueryHeap: Sized + 'static {
        /// The set of storages from which the heap will be derived.
        type Storages;

        /// An iterator over instances of this heap.
        type HeapIter: Iterator<Item = Self>;

        /// An iterator over blocks in this heap, parameterized by the lifetime of the heap borrow
        /// and the token used.
        #[rustfmt::skip]
        type BlockIter<'a, N>: RandomAccessIter<Item = Self::Block<'a, N>> where N: 'a + Token;

        /// An iterator over blocks in this heap, parameterized by the lifetime of the heap borrow
        /// and the token used.
        #[rustfmt::skip]
        type Block<'a, N> where N: 'a + Token;

        /// Fetches the storages needed to fetch these heaps.
        fn storages() -> Self::Storages;

        /// Fetches the set of heaps associated with this archetype.
        fn heaps_for_archetype(
            storages: &Self::Storages,
            archetype: &ArchetypeQueryInfo,
        ) -> Self::HeapIter;

        /// Fetches the blocks present in this heap.
        fn blocks<'a, N: Token>(&'a self, token: &'a N) -> Self::BlockIter<'a, N>;
    }

    // No heap
    impl QueryHeap for () {
        type Storages = ();
        type HeapIter = iter::Repeat<()>;

        #[rustfmt::skip]
        type BlockIter<'a, N> = RandomAccessRepeat<()> where N: 'a + Token;

        #[rustfmt::skip]
        type Block<'a, N> = () where N: 'a + Token;

        fn storages() -> Self::Storages {}

        fn heaps_for_archetype(
            _storages: &Self::Storages,
            _archetype: &ArchetypeQueryInfo,
        ) -> Self::HeapIter {
            iter::repeat(())
        }

        fn blocks<'a, N: Token>(&'a self, _token: &'a N) -> Self::BlockIter<'a, N> {
            RandomAccessRepeat::new(())
        }
    }

    // Arc<Heap<T>>
    impl<T: 'static> QueryHeap for Arc<Heap<T>> {
        type Storages = Storage<T>;
        type HeapIter = std::vec::IntoIter<Self>;

        #[rustfmt::skip]
        type BlockIter<'a, N> = heap_block_iter::Iter<'a, T, N> where N: 'a + Token;

        #[rustfmt::skip]
        type Block<'a, N> = HeapSlotBlock<'a, T, N> where N: 'a + Token;

        fn storages() -> Self::Storages {
            storage::<T>()
        }

        fn heaps_for_archetype(
            storages: &Self::Storages,
            archetype: &ArchetypeQueryInfo,
        ) -> Self::HeapIter {
            archetype.heaps_for(storages).into_iter()
        }

        fn blocks<'a, N: Token>(&'a self, token: &'a N) -> Self::BlockIter<'a, N> {
            self.blocks_expose_random_access(token)
        }
    }

    // EntityQueryHeap
    type EntityQueryHeap = Arc<[NMainCell<InertEntity>]>;

    impl QueryHeap for EntityQueryHeap {
        type Storages = ();
        type HeapIter = std::vec::IntoIter<EntityQueryHeap>;

        type BlockIter<'a, N> = RandomAccessSliceRef<'a, [NMainCell<InertEntity>; MultiRefCellIndex::COUNT]>
        where N: 'a + Token;

        type Block<'a, N> = &'a [NMainCell<InertEntity>; MultiRefCellIndex::COUNT]
        where N: 'a + Token;

        fn storages() -> Self::Storages {}

        fn heaps_for_archetype(
            _storages: &Self::Storages,
            archetype: &ArchetypeQueryInfo,
        ) -> Self::HeapIter {
            archetype.entities.clone().unwrap().into_iter()
        }

        fn blocks<'a, N: Token>(&'a self, _token: &'a N) -> Self::BlockIter<'a, N> {
            RandomAccessSliceRef::new(array_chunks(self))
        }
    }

    // Joined heap
    impl<A: QueryHeap, B: QueryHeap> QueryHeap for (A, B) {
        type Storages = (A::Storages, B::Storages);
        type HeapIter = iter::Zip<A::HeapIter, B::HeapIter>;

        type BlockIter<'a, N> = RandomAccessZip<A::BlockIter<'a, N>, B::BlockIter<'a, N>>
        where
            N: 'a + Token;

        #[rustfmt::skip]
        type Block<'a, N> = (A::Block<'a, N>, B::Block<'a, N>) where N: 'a + Token;

        fn storages() -> Self::Storages {
            (A::storages(), B::storages())
        }

        fn heaps_for_archetype(
            storages: &Self::Storages,
            archetype: &ArchetypeQueryInfo,
        ) -> Self::HeapIter {
            A::heaps_for_archetype(&storages.0, archetype)
                .zip(B::heaps_for_archetype(&storages.1, archetype))
        }

        fn blocks<'a, N: Token>(&'a self, token: &'a N) -> Self::BlockIter<'a, N> {
            RandomAccessZip::new(self.0.blocks(token), self.1.blocks(token))
        }
    }

    // === QueryGroupBorrow === //

    pub trait QueryGroupBorrow<B, N, L: 'static>: 'static {
        #[rustfmt::skip]
        type Guard<'a> where B: 'a, N: 'a;

        #[rustfmt::skip]
        type Iter<'a>: RandomAccessIter<Item = Self::IterItem<'a>> where B: 'a, N: 'a;

        #[rustfmt::skip]
        type IterItem<'a> where B: 'a, N: 'a;

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
        #[rustfmt::skip]
        type Guard<'a> = () where B: 'a, N: 'a;

        #[rustfmt::skip]
        type Iter<'a> = RandomAccessRepeat<()> where B: 'a, N: 'a;

        #[rustfmt::skip]
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
        #[rustfmt::skip]
        type Guard<'a> = MultiOptRef<'a, T> where 'b: 'a, N: 'a;

        #[rustfmt::skip]
        type Iter<'a> = RandomAccessSliceRef<'a, T> where 'b: 'a, N: 'a;

        #[rustfmt::skip]
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
        #[rustfmt::skip]
        type Guard<'a> = MultiOptRefMut<'a, T> where 'b: 'a, N: 'a;

        #[rustfmt::skip]
        type Iter<'a> = RandomAccessSliceMut<'a, T> where 'b: 'a, N: 'a;

        #[rustfmt::skip]
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
        #[rustfmt::skip]
        type Guard<'a> = &'a [NMainCell<InertEntity>; MultiRefCellIndex::COUNT] where 'b: 'a, N: 'a;

        #[rustfmt::skip]
        type Iter<'a> = RandomAccessSliceRef<'a, NMainCell<InertEntity>> where 'b: 'a, N: 'a;

        #[rustfmt::skip]
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
        #[rustfmt::skip]
        type Guard<'a> = HeapSlotBlock<'a, T, N> where 'b: 'a, N: 'a;

        #[rustfmt::skip]
        type Iter<'a> = heap_block_slot_iter::Iter<'a, T, N> where 'b: 'a, N: 'a;

        #[rustfmt::skip]
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
            <Self::Heap as QueryHeap>::Block<'block, MainThreadToken>,
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
                ArchetypeId::in_intersection(self.tags().chain(extra_tags), Self::NEEDS_ENTITIES);

            // For each archetype...
            for archetype in archetypes {
                // Fetch the component heaps associated with that archetype.
                let heaps = <Self::Heap>::heaps_for_archetype(&storages, &archetype);

                // For each of those heaps...
                for (heap_i, heap) in heaps.enumerate() {
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
                    let mut blocks = heap.blocks(token);
                    let complete_blocks_if_truncated =
                        RandomAccessTake::new(&mut blocks, complete_heap_block_count_or_big);

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
        type Heap = EntityQueryHeap;
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

        fn covariant_cast_input<'from: 'to, 'to>(src: Self::Input<'from>) -> Self::Input<'to> {
            src
        }
    }

    pub struct SlotQueryPart<T: 'static>(pub Tag<T>);

    impl<T: 'static> QueryPart for SlotQueryPart<T> {
        type Input<'a> = DirectSlot<'a, T>;
        type TagIter = iter::Once<RawTag>;
        type Heap = Arc<Heap<T>>;
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
            *elem
        }

        fn call_slow_borrow<B>(
            _token: &'static MainThreadToken,
            block: &BlockForQueryPart<Self>,
            index: MultiRefCellIndex,
            f: impl FnOnce(Self::Input<'_>) -> ControlFlow<B>,
        ) -> ControlFlow<B> {
            f(block.slot(index))
        }

        fn covariant_cast_input<'from: 'to, 'to>(src: Self::Input<'from>) -> Self::Input<'to> {
            src
        }
    }

    pub struct ObjQueryPart<T: 'static>(pub Tag<T>);

    impl<T: 'static> QueryPart for ObjQueryPart<T> {
        type Input<'a> = Obj<T>;
        type TagIter = iter::Once<RawTag>;
        type Heap = (EntityQueryHeap, Arc<Heap<T>>);
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

        fn covariant_cast_input<'from: 'to, 'to>(src: Self::Input<'from>) -> Self::Input<'to> {
            src
        }
    }

    pub struct ORefQueryPart<T: 'static>(pub Tag<T>);

    impl<T: 'static> QueryPart for ORefQueryPart<T> {
        type Input<'a> = CompRef<'a, T>;
        type TagIter = iter::Once<RawTag>;
        type Heap = (EntityQueryHeap, Arc<Heap<T>>);
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

        fn covariant_cast_input<'from: 'to, 'to>(src: Self::Input<'from>) -> Self::Input<'to> {
            src
        }
    }

    pub struct OMutQueryPart<T: 'static>(pub Tag<T>);

    impl<T: 'static> QueryPart for OMutQueryPart<T> {
        type Input<'a> = CompMut<'a, T>;
        type TagIter = iter::Once<RawTag>;
        type Heap = (EntityQueryHeap, Arc<Heap<T>>);
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

        fn covariant_cast_input<'from: 'to, 'to>(src: Self::Input<'from>) -> Self::Input<'to> {
            src
        }
    }

    pub struct RefQueryPart<T: 'static>(pub Tag<T>);

    impl<T: 'static> QueryPart for RefQueryPart<T> {
        type Input<'a> = &'a T;
        type TagIter = iter::Once<RawTag>;
        type Heap = Arc<Heap<T>>;
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

        fn covariant_cast_input<'from: 'to, 'to>(src: Self::Input<'from>) -> Self::Input<'to> {
            src
        }
    }

    pub struct MutQueryPart<T: 'static>(pub Tag<T>);

    impl<T: 'static> QueryPart for MutQueryPart<T> {
        type Input<'a> = &'a mut T;
        type TagIter = iter::Once<RawTag>;
        type Heap = Arc<Heap<T>>;
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

        fn covariant_cast_input<'from: 'to, 'to>(src: Self::Input<'from>) -> Self::Input<'to> {
            src
        }
    }

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
}

#[macro_export]
macro_rules! query {
    (
        for ($($input:tt)*) {
            $($body:tt)*
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($input)*};
                built_parts = {()};
                built_extractor = {()};
                extra_tags = {$crate::query::query_internals::empty_tag_iter()};
                body = {$($body)*};
            }
        }
    };
    (
        @internal {
            remaining_input = {};
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

    // entity
    (
        @internal {
            remaining_input = {entity $name:pat $(, $($rest:tt)*)?};
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
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
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
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
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
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
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
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
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
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
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
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
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
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
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
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
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
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
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
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
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
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
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
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
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
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
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
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
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query! {
            @internal {
                remaining_input = {$($($rest)*)?};
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
            built_parts = {$parts:expr};
            built_extractor = {$extractor:pat};
            extra_tags = {$extra_tags:expr};
            body = {$($body:tt)*};
        }
    ) => {
        $crate::query::query_internals::compile_error!(
            $crate::query::query_internals::concat!(
                "expected `entity`, `slot`, `obj`, `ref`, `mut`, `oref`, `omut`, `tag`, or `tags`; got `",
                $crate::query::query_internals::stringify!($($anything)*),
                "`"
            ),
        );
    };
}

pub use query;
