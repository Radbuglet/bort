use std::{any::TypeId, fmt, marker::PhantomData, sync::Arc};

use derive_where::derive_where;

use crate::{
    core::{cell::OptRef, heap::Heap, token::MainThreadToken, token_cell::NMainCell},
    database::{
        get_global_tag, DbRoot, InertArchetypeId, InertEntity, InertTag, RecursiveQueryGuardTy,
        ReifiedTagList,
    },
    util::misc::NamedTypeId,
    Storage,
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

// === Query Traits === //

#[doc(hidden)]
pub mod query_internals {
    use std::{iter, ops::ControlFlow, sync::Arc};

    use autoken::{ImmutableBorrow, MutableBorrow};

    use crate::{
        core::{
            cell::{MultiOptRef, MultiOptRefMut, MultiRefCellIndex},
            heap::{array_chunks, heap_iter::RandomAccessHeapBlockIter, Heap, HeapSlotBlock, Slot},
            random_iter::{
                RandomAccessEnumerate, RandomAccessIter, RandomAccessRepeat, RandomAccessSliceRef,
                RandomAccessZip,
            },
            token::{MainThreadToken, Token},
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

    /// A heap over which a [`QueryPart`] will be iterating.
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

    impl<T: 'static> QueryHeap for Arc<Heap<T>> {
        type Storages = Storage<T>;
        type HeapIter = std::vec::IntoIter<Self>;

        #[rustfmt::skip]
        type BlockIter<'a, N> = RandomAccessHeapBlockIter<'a, T, N> where N: 'a + Token;

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

    impl QueryHeap for Arc<[NMainCell<InertEntity>]> {
        type Storages = ();
        type HeapIter = std::vec::IntoIter<Arc<[NMainCell<InertEntity>]>>;

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

    /// The group borrow guard living for lifetime `'a` which can be acquired from a given cell `B`
    /// and loaner token `L`.
    pub trait QueryGroupBorrow<'guard, B, L>: Sized + 'guard {
        #[rustfmt::skip]
        type Iter<'iter>: Iterator<Item = Self::IterItem<'iter>> where Self: 'iter;

        #[rustfmt::skip]
        type IterItem<'iter> where Self: 'iter;

        fn try_borrow_group(
            block: &'guard B,
            token: &'static MainThreadToken,
            loaner: &'guard mut L,
        ) -> Option<Self>;

        fn iter(&mut self) -> Self::Iter<'_>;
    }

    pub struct GroupBorrowSupported;

    impl<'guard, B, L> QueryGroupBorrow<'guard, B, L> for GroupBorrowSupported {
        type Iter<'iter> = iter::Repeat<()>;
        type IterItem<'iter> = ();

        fn try_borrow_group(
            _block: &'guard B,
            _token: &'static MainThreadToken,
            _loaner: &'guard mut L,
        ) -> Option<Self> {
            Some(GroupBorrowSupported)
        }

        fn iter(&mut self) -> Self::Iter<'_> {
            iter::repeat(())
        }
    }

    pub enum GroupBorrowUnsupported {}

    impl<'guard, B, L> QueryGroupBorrow<'guard, B, L> for GroupBorrowUnsupported {
        type Iter<'iter> = iter::Repeat<()>;
        type IterItem<'iter> = ();

        fn try_borrow_group(
            _block: &'guard B,
            _token: &'static MainThreadToken,
            _loaner: &'guard mut L,
        ) -> Option<Self> {
            None
        }

        fn iter(&mut self) -> Self::Iter<'_> {
            iter::repeat(())
        }
    }

    impl<'guard, 'block, T: 'static>
        QueryGroupBorrow<'guard, HeapSlotBlock<'block, T, MainThreadToken>, ImmutableBorrow<T>>
        for MultiOptRef<'guard, T>
    {
        #[rustfmt::skip]
        type Iter<'iter> = std::slice::Iter<'iter, T> where Self: 'iter;

        #[rustfmt::skip]
        type IterItem<'iter> = &'iter T where Self: 'iter;

        fn try_borrow_group(
            block: &'guard HeapSlotBlock<'block, T, MainThreadToken>,
            token: &'static MainThreadToken,
            loaner: &'guard mut ImmutableBorrow<T>,
        ) -> Option<Self> {
            block.values().try_borrow_all(token, loaner.downgrade_ref())
        }

        fn iter(&mut self) -> Self::Iter<'_> {
            (**self).iter()
        }
    }

    impl<'guard, 'block, T: 'static>
        QueryGroupBorrow<'guard, HeapSlotBlock<'block, T, MainThreadToken>, MutableBorrow<T>>
        for MultiOptRefMut<'guard, T>
    {
        #[rustfmt::skip]
        type Iter<'iter> = std::slice::IterMut<'iter, T> where Self: 'iter;

        #[rustfmt::skip]
        type IterItem<'iter> = &'iter mut T where Self: 'iter;

        fn try_borrow_group(
            block: &'guard HeapSlotBlock<'block, T, MainThreadToken>,
            token: &'static MainThreadToken,
            loaner: &'guard mut MutableBorrow<T>,
        ) -> Option<Self> {
            block
                .values()
                .try_borrow_all_mut(token, loaner.downgrade_mut())
        }

        fn iter(&mut self) -> Self::Iter<'_> {
            (**self).iter_mut()
        }
    }

    impl<'guard, 'slice>
        QueryGroupBorrow<'guard, &'slice [NMainCell<InertEntity>; MultiRefCellIndex::COUNT], ()>
        for &'guard [NMainCell<InertEntity>; MultiRefCellIndex::COUNT]
    {
        #[rustfmt::skip]
        type Iter<'iter> = std::slice::Iter<'iter, NMainCell<InertEntity>> where Self: 'iter;

        #[rustfmt::skip]
        type IterItem<'iter> = &'iter NMainCell<InertEntity> where Self: 'iter;

        fn try_borrow_group(
            block: &'guard &'guard [NMainCell<InertEntity>; MultiRefCellIndex::COUNT],
            _token: &'static MainThreadToken,
            _loaner: &'guard mut (),
        ) -> Option<Self> {
            Some(block)
        }

        fn iter(&mut self) -> Self::Iter<'_> {
            (**self).iter()
        }
    }

    impl<'guard, ItemA, ItemB, B1, L1, B2, L2> QueryGroupBorrow<'guard, (B1, B2), (L1, L2)>
        for (ItemA, ItemB)
    where
        ItemA: QueryGroupBorrow<'guard, B1, L1>,
        ItemB: QueryGroupBorrow<'guard, B2, L2>,
    {
        #[rustfmt::skip]
        type Iter<'iter> = iter::Zip<ItemA::Iter<'iter>, ItemB::Iter<'iter>> where Self: 'iter;

        #[rustfmt::skip]
        type IterItem<'iter> = (ItemA::IterItem<'iter>, ItemB::IterItem<'iter>) where Self: 'iter;

        fn try_borrow_group(
            block: &'guard (B1, B2),
            token: &'static MainThreadToken,
            loaner: &'guard mut (L1, L2),
        ) -> Option<Self> {
            Some((
                ItemA::try_borrow_group(&block.0, token, &mut loaner.0)?,
                ItemB::try_borrow_group(&block.1, token, &mut loaner.1)?,
            ))
        }

        fn iter(&mut self) -> Self::Iter<'_> {
            self.0.iter().zip(self.1.iter())
        }
    }

    // === QueryPart === //

    pub trait QueryPart: Sized {
        type Input<'a>;
        type TagIter: Iterator<Item = RawTag>;
        type AutokenLoan: Default;
        type Heap: QueryHeap;
        type GroupBorrow<'guard>: for<'block_iter> QueryGroupBorrow<
            'guard,
            <Self::Heap as QueryHeap>::Block<'block_iter, MainThreadToken>,
            Self::AutokenLoan,
        >;

        const NEEDS_ENTITIES: bool;

        fn tags(self) -> Self::TagIter;

        fn elem_from_block_item<'elem, 'guard>(
            token: &'static MainThreadToken,
            elem: &'elem mut <Self::GroupBorrow<'guard> as QueryGroupBorrow<
                'guard,
                <Self::Heap as QueryHeap>::Block<'_, MainThreadToken>,
                Self::AutokenLoan,
            >>::IterItem<'_>,
        ) -> Self::Input<'elem>;

        fn call_slow_borrow<B>(
            token: &'static MainThreadToken,
            block: &<Self::Heap as QueryHeap>::Block<'_, MainThreadToken>,
            index: MultiRefCellIndex,
            loaner: &mut Self::AutokenLoan,
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

            // Acquire a loan to all the components we're going to borrow.
            let mut loaner = <Self::AutokenLoan>::default();

            // For each archetype...
            for archetype in archetypes {
                // Fetch the component heaps associated with that archetype.
                let heaps = <Self::Heap>::heaps_for_archetype(&storages, &archetype);

                // For each of those heaps...
                for (heap_i, heap) in heaps.enumerate() {
                    // Determine the length of this heap
                    let heap_len_if_truncated = if heap_i == archetype.heap_count() - 1 {
                        archetype.last_heap_len()
                    } else {
                        usize::MAX
                    };

                    // Fetch the blocks comprising it.
                    let blocks = RandomAccessZip::new(RandomAccessEnumerate, heap.blocks(token));

                    // For each block...
                    'block_iter: for (block_i, block) in blocks.into_iter() {
                        // Attempt to run the fast-path...
                        if let Some(mut block) =
                            <Self::GroupBorrow<'_>>::try_borrow_group(&block, token, &mut loaner)
                        {
                            for mut elem in block.iter() {
                                f(Self::elem_from_block_item(token, &mut elem))?;
                            }

                            // N.B. we `continue` here rather than putting the slow path in an `else`
                            // block because the lifetime of the `loaner` borrow extends into both
                            // branches of the `if` expression.
                            continue;
                        }

                        // Otherwise, run the slow-path.
                        for index in MultiRefCellIndex::iter() {
                            if block_i * MultiRefCellIndex::COUNT + index as usize
                                >= heap_len_if_truncated
                            {
                                break 'block_iter;
                            }

                            Self::call_slow_borrow(token, &block, index, &mut loaner, &mut f);
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
        type AutokenLoan = ();
        type Heap = Arc<[NMainCell<InertEntity>]>;
        type GroupBorrow<'a> = &'a [NMainCell<InertEntity>; MultiRefCellIndex::COUNT];

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
            block: &<Self::Heap as QueryHeap>::Block<'_, MainThreadToken>,
            index: MultiRefCellIndex,
            loaner: &mut Self::AutokenLoan,
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
        type Input<'a> = Slot<T>;
        type TagIter = iter::Once<RawTag>;
        type AutokenLoan = ();
        type Heap = Arc<Heap<T>>;
        type GroupBorrow<'a> = GroupBorrowSupported;

        const NEEDS_ENTITIES: bool = false;

        fn tags(self) -> Self::TagIter {
            iter::once(self.0.raw())
        }

        fn elem_from_block_item<'elem>(
            token: &'static MainThreadToken,
            elem: &'elem mut (),
        ) -> Self::Input<'elem> {
            todo!()
        }

        fn call_slow_borrow<B>(
            token: &'static MainThreadToken,
            block: &<Self::Heap as QueryHeap>::Block<'_, MainThreadToken>,
            index: MultiRefCellIndex,
            loaner: &mut Self::AutokenLoan,
            f: impl FnOnce(Self::Input<'_>) -> ControlFlow<B>,
        ) -> ControlFlow<B> {
            todo!();
        }

        fn covariant_cast_input<'from: 'to, 'to>(src: Self::Input<'from>) -> Self::Input<'to> {
            src
        }
    }

    pub struct ObjQueryPart<T: 'static>(pub Tag<T>);

    impl<T: 'static> QueryPart for ObjQueryPart<T> {
        type Input<'a> = Obj<T>;
        type TagIter = iter::Once<RawTag>;
        type AutokenLoan = ();
        type Heap = Arc<Heap<T>>;
        type GroupBorrow<'a> = GroupBorrowSupported;

        const NEEDS_ENTITIES: bool = true;

        fn tags(self) -> Self::TagIter {
            iter::once(self.0.raw())
        }

        fn elem_from_block_item<'elem>(
            token: &'static MainThreadToken,
            elem: &'elem mut (),
        ) -> Self::Input<'elem> {
            todo!()
        }

        fn call_slow_borrow<B>(
            token: &'static MainThreadToken,
            block: &<Self::Heap as QueryHeap>::Block<'_, MainThreadToken>,
            index: MultiRefCellIndex,
            loaner: &mut Self::AutokenLoan,
            f: impl FnOnce(Self::Input<'_>) -> ControlFlow<B>,
        ) -> ControlFlow<B> {
            todo!();
        }

        fn covariant_cast_input<'from: 'to, 'to>(src: Self::Input<'from>) -> Self::Input<'to> {
            src
        }
    }

    pub struct ORefQueryPart<T: 'static>(pub Tag<T>);

    impl<T: 'static> QueryPart for ORefQueryPart<T> {
        type Input<'a> = CompRef<'a, T>;
        type TagIter = iter::Once<RawTag>;
        type AutokenLoan = ImmutableBorrow<T>;
        type Heap = Arc<Heap<T>>;
        type GroupBorrow<'a> = GroupBorrowUnsupported;

        const NEEDS_ENTITIES: bool = true;

        fn tags(self) -> Self::TagIter {
            iter::once(self.0.raw())
        }

        fn elem_from_block_item<'elem>(
            _token: &'static MainThreadToken,
            _elem: &'elem mut (),
        ) -> Self::Input<'elem> {
            unreachable!()
        }

        fn call_slow_borrow<B>(
            token: &'static MainThreadToken,
            block: &<Self::Heap as QueryHeap>::Block<'_, MainThreadToken>,
            index: MultiRefCellIndex,
            loaner: &mut Self::AutokenLoan,
            f: impl FnOnce(Self::Input<'_>) -> ControlFlow<B>,
        ) -> ControlFlow<B> {
            todo!();
        }

        fn covariant_cast_input<'from: 'to, 'to>(src: Self::Input<'from>) -> Self::Input<'to> {
            src
        }
    }

    pub struct OMutQueryPart<T: 'static>(pub Tag<T>);

    impl<T: 'static> QueryPart for OMutQueryPart<T> {
        type Input<'a> = CompMut<'a, T>;
        type TagIter = iter::Once<RawTag>;
        type AutokenLoan = MutableBorrow<T>;
        type Heap = Arc<Heap<T>>;
        type GroupBorrow<'a> = GroupBorrowUnsupported;

        const NEEDS_ENTITIES: bool = true;

        fn tags(self) -> Self::TagIter {
            iter::once(self.0.raw())
        }

        fn elem_from_block_item<'elem>(
            _token: &'static MainThreadToken,
            _elem: &'elem mut (),
        ) -> Self::Input<'elem> {
            unreachable!()
        }

        fn call_slow_borrow<B>(
            token: &'static MainThreadToken,
            block: &<Self::Heap as QueryHeap>::Block<'_, MainThreadToken>,
            index: MultiRefCellIndex,
            loaner: &mut Self::AutokenLoan,
            f: impl FnOnce(Self::Input<'_>) -> ControlFlow<B>,
        ) -> ControlFlow<B> {
            todo!();
        }

        fn covariant_cast_input<'from: 'to, 'to>(src: Self::Input<'from>) -> Self::Input<'to> {
            src
        }
    }

    pub struct RefQueryPart<T: 'static>(pub Tag<T>);

    impl<T: 'static> QueryPart for RefQueryPart<T> {
        type Input<'a> = &'a T;
        type TagIter = iter::Once<RawTag>;
        type AutokenLoan = ImmutableBorrow<T>;
        type Heap = Arc<Heap<T>>;
        type GroupBorrow<'a> = MultiOptRef<'a, T>;

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
            block: &<Self::Heap as QueryHeap>::Block<'_, MainThreadToken>,
            index: MultiRefCellIndex,
            loaner: &mut Self::AutokenLoan,
            f: impl FnOnce(Self::Input<'_>) -> ControlFlow<B>,
        ) -> ControlFlow<B> {
            f(&block.values().borrow_on_loan(token, index, &loaner))
        }

        fn covariant_cast_input<'from: 'to, 'to>(src: Self::Input<'from>) -> Self::Input<'to> {
            src
        }
    }

    pub struct MutQueryPart<T: 'static>(pub Tag<T>);

    impl<T: 'static> QueryPart for MutQueryPart<T> {
        type Input<'a> = &'a mut T;
        type TagIter = iter::Once<RawTag>;
        type AutokenLoan = MutableBorrow<T>;
        type Heap = Arc<Heap<T>>;
        type GroupBorrow<'a> = MultiOptRefMut<'a, T>;

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
            block: &<Self::Heap as QueryHeap>::Block<'_, MainThreadToken>,
            index: MultiRefCellIndex,
            loaner: &mut Self::AutokenLoan,
            f: impl FnOnce(Self::Input<'_>) -> ControlFlow<B>,
        ) -> ControlFlow<B> {
            f(&mut block.values().borrow_mut_on_loan(token, index, loaner))
        }

        fn covariant_cast_input<'from: 'to, 'to>(src: Self::Input<'from>) -> Self::Input<'to> {
            src
        }
    }

    impl<Left: QueryPart, Right: QueryPart> QueryPart for (Left, Right) {
        type Input<'a> = (Left::Input<'a>, Right::Input<'a>);
        type Heap = (Left::Heap, Right::Heap);
        type TagIter = iter::Chain<Left::TagIter, Right::TagIter>;
        type AutokenLoan = (Left::AutokenLoan, Right::AutokenLoan);
        type GroupBorrow<'a> = (Left::GroupBorrow<'a>, Right::GroupBorrow<'a>);

        const NEEDS_ENTITIES: bool = Left::NEEDS_ENTITIES || Right::NEEDS_ENTITIES;

        fn tags(self) -> Self::TagIter {
            self.0.tags().chain(self.1.tags())
        }

        fn elem_from_block_item<'elem, 'guard>(
            token: &'static MainThreadToken,
            elem: &'elem mut <Self::GroupBorrow<'guard> as QueryGroupBorrow<
                'guard,
                <Self::Heap as QueryHeap>::Block<'_, MainThreadToken>,
                Self::AutokenLoan,
            >>::IterItem<'_>,
        ) -> Self::Input<'elem> {
            (
                Left::elem_from_block_item(token, &mut elem.0),
                Right::elem_from_block_item(token, &mut elem.1),
            )
        }

        fn call_slow_borrow<B>(
            token: &'static MainThreadToken,
            block: &<Self::Heap as QueryHeap>::Block<'_, MainThreadToken>,
            index: MultiRefCellIndex,
            loaner: &mut Self::AutokenLoan,
            f: impl FnOnce(Self::Input<'_>) -> ControlFlow<B>,
        ) -> ControlFlow<B> {
            Left::call_slow_borrow(token, &block.0, index, &mut loaner.0, |a| {
                Right::call_slow_borrow(token, &block.1, index, &mut loaner.1, |b| {
                    f((
                        Left::covariant_cast_input(a),
                        Right::covariant_cast_input(b),
                    ))
                })
            })
        }

        fn covariant_cast_input<'from: 'to, 'to>(src: Self::Input<'from>) -> Self::Input<'to> {
            (
                Left::covariant_cast_input(src.0),
                Right::covariant_cast_input(src.1),
            )
        }
    }

    impl QueryPart for () {
        type Input<'a> = ();
        type TagIter = iter::Empty<RawTag>;
        type AutokenLoan = ();
        type Heap = ();
        type GroupBorrow<'a> = GroupBorrowSupported;

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
            _block: &<Self::Heap as QueryHeap>::Block<'_, MainThreadToken>,
            _index: MultiRefCellIndex,
            _loaner: &mut Self::AutokenLoan,
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
