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
    ) -> Vec<AnonymousArchetypeQueryInfo> {
        let token = MainThreadToken::acquire_fmt("enumerate archetypes in a tag intersection");

        let mut archetypes = Vec::new();
        Self::reify_tag_list(tags, |tags| {
            DbRoot::get(token).enumerate_tag_intersection(tags, |info| {
                archetypes.push(AnonymousArchetypeQueryInfo {
                    archetype: info.archetype.into_dangerous_archetype_id(),
                    heap_count: info.entities.len(),
                    last_heap_len: info.last_heap_len,
                });
            })
        });

        archetypes
    }

    pub fn in_intersection_with_entities(
        tags: impl IntoIterator<Item = RawTag>,
    ) -> Vec<NamedArchetypeQueryInfo> {
        let token = MainThreadToken::acquire_fmt("enumerate archetypes in a tag intersection");

        let mut archetypes = Vec::new();
        Self::reify_tag_list(tags, |tags| {
            DbRoot::get(token).enumerate_tag_intersection(tags, |info| {
                archetypes.push(NamedArchetypeQueryInfo {
                    archetype: info.archetype.into_dangerous_archetype_id(),
                    last_heap_len: info.last_heap_len,
                    entities: info.entities.clone(),
                });
            })
        });

        archetypes
    }
}

#[derive(Debug, Clone)]
pub struct AnonymousArchetypeQueryInfo {
    archetype: ArchetypeId,
    heap_count: usize,
    last_heap_len: usize,
}

impl AnonymousArchetypeQueryInfo {
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
}

#[derive(Debug, Clone)]
pub struct NamedArchetypeQueryInfo {
    archetype: ArchetypeId,
    last_heap_len: usize,
    entities: Vec<Arc<[NMainCell<InertEntity>]>>,
}

impl NamedArchetypeQueryInfo {
    pub fn as_anonymous(&self) -> AnonymousArchetypeQueryInfo {
        AnonymousArchetypeQueryInfo {
            archetype: self.archetype,
            heap_count: self.entities.len(),
            last_heap_len: self.last_heap_len,
        }
    }

    pub fn archetype(&self) -> ArchetypeId {
        self.archetype
    }

    pub fn heap_count(&self) -> usize {
        self.entities.len()
    }

    pub fn last_heap_len(&self) -> usize {
        self.last_heap_len
    }

    pub fn heaps_for<T>(&self, storage: &Storage<T>) -> Vec<Arc<Heap<T>>> {
        DbRoot::heaps_from_archetype_aba(self.archetype.0, &storage.inner.borrow(&storage.token))
    }

    // TODO: Expose `entities`
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
            cell::{MultiOptRef, MultiOptRefMut},
            heap::{heap_iter::RandomAccessHeapBlockIter, Heap, HeapSlotBlock, Slot},
            random_iter::{
                RandomAccessEnumerate, RandomAccessIter, RandomAccessRepeat, RandomAccessZip,
            },
            token::{MainThreadToken, Token},
        },
        entity::{CompMut, CompRef, Entity},
        obj::Obj,
        storage, Storage,
    };

    use super::{
        borrow_flush_guard, AnonymousArchetypeQueryInfo, ArchetypeId, HasGlobalManagedTag, RawTag,
        Tag,
    };

    pub use {
        cbit::cbit,
        std::{compile_error, concat, iter::Iterator, stringify},
    };

    // === QueryHeap === //

    pub trait QueryHeap: Sized + 'static {
        type Storages;
        type HeapIter: Iterator<Item = Self>;

        #[rustfmt::skip]
        type BlockIter<'a, N>: RandomAccessIter<Item = Self::Block<'a, N>> where N: 'a + Token;

        #[rustfmt::skip]
        type Block<'a, N> where N: 'a + Token;

        fn storages() -> Self::Storages;

        fn heaps_for_archetype(
            storages: &Self::Storages,
            archetype: &AnonymousArchetypeQueryInfo,
        ) -> Self::HeapIter;

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
            _archetype: &AnonymousArchetypeQueryInfo,
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
            archetype: &AnonymousArchetypeQueryInfo,
        ) -> Self::HeapIter {
            archetype.heaps_for(storages).into_iter()
        }

        fn blocks<'a, N: Token>(&'a self, token: &'a N) -> Self::BlockIter<'a, N> {
            self.blocks_expose_random_access(token)
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
            archetype: &AnonymousArchetypeQueryInfo,
        ) -> Self::HeapIter {
            A::heaps_for_archetype(&storages.0, archetype)
                .zip(B::heaps_for_archetype(&storages.1, archetype))
        }

        fn blocks<'a, N: Token>(&'a self, token: &'a N) -> Self::BlockIter<'a, N> {
            RandomAccessZip::new(self.0.blocks(token), self.1.blocks(token))
        }
    }

    // === QueryGroupBorrow === //

    pub trait QueryGroupBorrow<'a, B, L>: Sized {
        fn try_borrow_group(
            block: &'a B,
            token: &'static MainThreadToken,
            loaner: &'a mut L,
        ) -> Option<Self>;
    }

    pub struct GroupBorrowSupported;

    impl<'a, B, L> QueryGroupBorrow<'a, B, L> for GroupBorrowSupported {
        fn try_borrow_group(
            _block: &'a B,
            _token: &'static MainThreadToken,
            _loaner: &'a mut L,
        ) -> Option<Self> {
            Some(GroupBorrowSupported)
        }
    }

    pub enum GroupBorrowUnsupported {}

    impl<'a, B, L> QueryGroupBorrow<'a, B, L> for GroupBorrowUnsupported {
        fn try_borrow_group(
            _block: &'a B,
            _token: &'static MainThreadToken,
            _loaner: &'a mut L,
        ) -> Option<Self> {
            None
        }
    }

    impl<'a, 'b, T: 'static>
        QueryGroupBorrow<'a, HeapSlotBlock<'b, T, MainThreadToken>, ImmutableBorrow<T>>
        for MultiOptRef<'a, T>
    {
        fn try_borrow_group(
            block: &'a HeapSlotBlock<'a, T, MainThreadToken>,
            token: &'static MainThreadToken,
            loaner: &'a mut ImmutableBorrow<T>,
        ) -> Option<Self> {
            block.values().try_borrow_all(token, loaner.downgrade_ref())
        }
    }

    impl<'a, 'b, T: 'static>
        QueryGroupBorrow<'a, HeapSlotBlock<'b, T, MainThreadToken>, MutableBorrow<T>>
        for MultiOptRefMut<'a, T>
    {
        fn try_borrow_group(
            block: &'a HeapSlotBlock<'a, T, MainThreadToken>,
            token: &'static MainThreadToken,
            loaner: &'a mut MutableBorrow<T>,
        ) -> Option<Self> {
            block
                .values()
                .try_borrow_all_mut(token, loaner.downgrade_mut())
        }
    }

    impl<'a, ItemA, ItemB, B1, L1, B2, L2> QueryGroupBorrow<'a, (B1, B2), (L1, L2)> for (ItemA, ItemB)
    where
        ItemA: QueryGroupBorrow<'a, B1, L1>,
        ItemB: QueryGroupBorrow<'a, B2, L2>,
    {
        fn try_borrow_group(
            block: &'a (B1, B2),
            token: &'static MainThreadToken,
            loaner: &'a mut (L1, L2),
        ) -> Option<Self> {
            todo!()
        }
    }

    // === QueryPart === //

    pub trait QueryPart: Sized {
        type Input<'a>;
        type Heap: QueryHeap;
        type TagIter: Iterator<Item = RawTag>;
        type AutokenLoan: Default;
        type GroupBorrow<'a>: for<'b> QueryGroupBorrow<
            'a,
            <Self::Heap as QueryHeap>::Block<'b, MainThreadToken>,
            Self::AutokenLoan,
        >;

        fn tags(self) -> Self::TagIter;

        fn query<B>(
            self,
            extra_tags: impl IntoIterator<Item = RawTag>,
            f: impl FnMut(Self::Input<'_>) -> ControlFlow<B>,
        ) -> ControlFlow<B> {
            let token = MainThreadToken::acquire_fmt("run a query");
            let _guard = borrow_flush_guard();
            let storages = <Self::Heap>::storages();
            let archetypes = ArchetypeId::in_intersection(self.tags().chain(extra_tags));

            let mut loaner = <Self::AutokenLoan>::default();

            for archetype in archetypes {
                let heaps = <Self::Heap>::heaps_for_archetype(&storages, &archetype);

                for (heap_i, heap) in heaps.enumerate() {
                    let blocks = RandomAccessZip::new(RandomAccessEnumerate, heap.blocks(token));

                    for (block_i, block) in blocks.into_iter() {
                        if let Some(block) =
                            <Self::GroupBorrow<'_>>::try_borrow_group(&block, token, &mut loaner)
                        {
                            // Fast-path
                        } else {
                            // Slow-path
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
        type Heap = ();
        type TagIter = iter::Empty<RawTag>;
        type AutokenLoan = ();
        type GroupBorrow<'a> = GroupBorrowSupported;

        fn tags(self) -> Self::TagIter {
            iter::empty()
        }
    }

    pub struct SlotQueryPart<T: 'static>(pub Tag<T>);

    impl<T: 'static> QueryPart for SlotQueryPart<T> {
        type Input<'a> = Slot<T>;
        type Heap = Arc<Heap<T>>;
        type TagIter = iter::Once<RawTag>;
        type AutokenLoan = ();
        type GroupBorrow<'a> = GroupBorrowSupported;

        fn tags(self) -> Self::TagIter {
            iter::once(self.0.raw())
        }
    }

    pub struct ObjQueryPart<T: 'static>(pub Tag<T>);

    impl<T: 'static> QueryPart for ObjQueryPart<T> {
        type Input<'a> = Obj<T>;
        type Heap = Arc<Heap<T>>;
        type TagIter = iter::Once<RawTag>;
        type AutokenLoan = ();
        type GroupBorrow<'a> = GroupBorrowSupported;

        fn tags(self) -> Self::TagIter {
            iter::once(self.0.raw())
        }
    }

    pub struct ORefQueryPart<T: 'static>(pub Tag<T>);

    impl<T: 'static> QueryPart for ORefQueryPart<T> {
        type Input<'a> = CompRef<'a, T>;
        type Heap = Arc<Heap<T>>;
        type TagIter = iter::Once<RawTag>;
        type AutokenLoan = ImmutableBorrow<T>;
        type GroupBorrow<'a> = GroupBorrowUnsupported;

        fn tags(self) -> Self::TagIter {
            iter::once(self.0.raw())
        }
    }

    pub struct OMutQueryPart<T: 'static>(pub Tag<T>);

    impl<T: 'static> QueryPart for OMutQueryPart<T> {
        type Input<'a> = CompMut<'a, T>;
        type Heap = Arc<Heap<T>>;
        type TagIter = iter::Once<RawTag>;
        type AutokenLoan = MutableBorrow<T>;
        type GroupBorrow<'a> = GroupBorrowUnsupported;

        fn tags(self) -> Self::TagIter {
            iter::once(self.0.raw())
        }
    }

    pub struct RefQueryPart<T: 'static>(pub Tag<T>);

    impl<T: 'static> QueryPart for RefQueryPart<T> {
        type Input<'a> = &'a T;
        type Heap = Arc<Heap<T>>;
        type TagIter = iter::Once<RawTag>;
        type AutokenLoan = ImmutableBorrow<T>;
        type GroupBorrow<'a> = MultiOptRef<'a, T>;

        fn tags(self) -> Self::TagIter {
            iter::once(self.0.raw())
        }
    }

    pub struct MutQueryPart<T: 'static>(pub Tag<T>);

    impl<T: 'static> QueryPart for MutQueryPart<T> {
        type Input<'a> = &'a mut T;
        type Heap = Arc<Heap<T>>;
        type TagIter = iter::Once<RawTag>;
        type AutokenLoan = MutableBorrow<T>;
        type GroupBorrow<'a> = MultiOptRefMut<'a, T>;

        fn tags(self) -> Self::TagIter {
            iter::once(self.0.raw())
        }
    }

    impl<A: QueryPart, B: QueryPart> QueryPart for (A, B) {
        type Input<'a> = (A::Input<'a>, B::Input<'a>);
        type Heap = (A::Heap, B::Heap);
        type TagIter = iter::Chain<A::TagIter, B::TagIter>;
        type AutokenLoan = (A::AutokenLoan, B::AutokenLoan);
        type GroupBorrow<'a> = (A::GroupBorrow<'a>, B::GroupBorrow<'a>);

        fn tags(self) -> Self::TagIter {
            self.0.tags().chain(self.1.tags())
        }
    }

    impl QueryPart for () {
        type Input<'a> = ();
        type Heap = ();
        type TagIter = iter::Empty<RawTag>;
        type AutokenLoan = ();
        type GroupBorrow<'a> = GroupBorrowSupported;

        fn tags(self) -> Self::TagIter {
            iter::empty()
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
