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
    use std::ops::ControlFlow;

    use crate::{
        core::heap::Slot,
        entity::{CompMut, CompRef, Entity},
        obj::Obj,
    };

    use super::{HasGlobalManagedTag, RawTag, Tag};

    pub use {cbit::cbit, std::iter::Iterator};

    pub trait QueryPart: Sized {
        type Input<'a>;

        fn query<B>(
            self,
            extra_tags: impl IntoIterator<Item = RawTag>,
            f: impl FnMut(Self::Input<'_>) -> ControlFlow<B>,
        ) -> ControlFlow<B> {
            ControlFlow::Continue(())
        }
    }

    pub struct EntityQueryPart;

    impl QueryPart for EntityQueryPart {
        type Input<'a> = Entity;
    }

    pub struct SlotQueryPart<T: 'static>(pub Tag<T>);

    impl<T: 'static> QueryPart for SlotQueryPart<T> {
        type Input<'a> = Slot<T>;
    }

    pub struct ObjQueryPart<T: 'static>(pub Tag<T>);

    impl<T: 'static> QueryPart for ObjQueryPart<T> {
        type Input<'a> = Obj<T>;
    }

    pub struct ORefQueryPart<T: 'static>(pub Tag<T>);

    impl<T: 'static> QueryPart for ORefQueryPart<T> {
        type Input<'a> = CompRef<'a, T>;
    }

    pub struct OMutQueryPart<T: 'static>(pub Tag<T>);

    impl<T: 'static> QueryPart for OMutQueryPart<T> {
        type Input<'a> = CompMut<'a, T>;
    }

    pub struct RefQueryPart<T: 'static>(pub Tag<T>);

    impl<T: 'static> QueryPart for RefQueryPart<T> {
        type Input<'a> = &'a T;
    }

    pub struct MutQueryPart<T: 'static>(pub Tag<T>);

    impl<T: 'static> QueryPart for MutQueryPart<T> {
        type Input<'a> = &'a mut T;
    }

    impl<A: QueryPart, B: QueryPart> QueryPart for (A, B) {
        type Input<'a> = (A::Input<'a>, B::Input<'a>);
    }

    impl QueryPart for () {
        type Input<'a> = ();
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
}

pub use query;
