use std::{any::TypeId, fmt, marker::PhantomData};

use derive_where::derive_where;

use crate::{
    core::token::MainThreadToken,
    database::{get_global_tag, DbRoot, DbStorage, InertTag},
    entity::Storage,
    util::misc::NamedTypeId,
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

    pub fn global() -> Self
    where
        T: ManagedGlobalTag,
    {
        Self {
            _ty: PhantomData,
            raw: get_global_tag(NamedTypeId::of::<T>(), NamedTypeId::of::<T::Component>()),
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

    pub fn global<T: VirtualGlobalTag>() -> Self {
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

// === Static Tags === //

pub trait ManagedGlobalTag: Sized + 'static {
    type Component;
}

pub trait VirtualGlobalTag: Sized + 'static {}

// === Flushing === //

#[must_use]
pub fn try_flush() -> bool {
    let token = MainThreadToken::acquire_fmt("flush entity archetypes");
    DbRoot::get(token).flush_archetypes(token).is_ok()
}

pub fn flush() {
    assert!(
        try_flush(),
        "attempted to flush the entity database while a query was active"
    );
}

// === Queries === //

#[doc(hidden)]
pub mod query_internals {
    use super::*;

    // === Re-exports === //

    pub use {
        crate::{
            core::token::MainThreadToken,
            database::{DbRoot, InertTag, ReifiedTagList},
            event::QueryableEventList,
            query::try_flush,
        },
        std::{
            assert,
            hint::unreachable_unchecked,
            iter::{empty, Iterator},
            mem::drop,
            ops::ControlFlow,
            option::Option,
            vec::Vec,
        },
    };

    // === Helpers === //

    pub fn get_tag<T: 'static>(tag: impl Into<Tag<T>>) -> (Tag<T>, InertTag) {
        let tag = tag.into();
        (tag, tag.raw.0)
    }

    pub fn get_storage<T: 'static>(
        db: &mut DbRoot,
        _infer: (Tag<T>, InertTag),
    ) -> &'static DbStorage<T> {
        db.get_storage::<T>()
    }

    pub fn inner_storage_to_api_storage<T: 'static>(
        token: &'static MainThreadToken,
        inner: &'static DbStorage<T>,
    ) -> Storage<T> {
        Storage::from_database(token, inner)
    }

    pub trait ExtraTagConverter {
        fn into_single(self, extra: &mut Vec<InertTag>) -> Option<InertTag>;
    }

    impl ExtraTagConverter for VirtualTag {
        fn into_single(self, _extra: &mut Vec<InertTag>) -> Option<InertTag> {
            Some(self.raw.0)
        }
    }

    impl<T: 'static> ExtraTagConverter for Tag<T> {
        fn into_single(self, _extra: &mut Vec<InertTag>) -> Option<InertTag> {
            Some(self.raw.0)
        }
    }

    impl ExtraTagConverter for RawTag {
        fn into_single(self, _extra: &mut Vec<InertTag>) -> Option<InertTag> {
            Some(self.0)
        }
    }

    impl<I: IntoIterator> ExtraTagConverter for I
    where
        I::Item: Into<RawTag>,
    {
        fn into_single(self, extra: &mut Vec<InertTag>) -> Option<InertTag> {
            let mut iter = self.into_iter();

            if let Some(first) = iter.next() {
                let first = first.into().0;
                extra.extend(iter.map(|v| v.into().0));

                Some(first)
            } else {
                None
            }
        }
    }
}

#[macro_export]
macro_rules! query {
	// === Event query === //
	(
		for (
			$event_name:ident in $event_src:expr
			;
			$(@$entity:ident $(,)?)?
			$($prefix:ident $name:ident in $tag:expr),*
			$(,)?
		)
		$(+ [$($vtag:expr),*$(,)?])?
		{
			$($body:tt)*
		}
	) => {'__query: {
		#[derive(Hash, Eq, PartialEq)]
		struct QueryDiscriminator;

		// Evaluate our tag expressions
		$( let $name = $crate::query::query_internals::get_tag($tag); )*

		// Determine tag list
		let mut virtual_tags_dyn = Vec::<$crate::query::query_internals::InertTag>::new();
		let virtual_tags_static = [
			$($crate::query::query_internals::Option::Some($name.1),)*
			$($($crate::query::query_internals::ExtraTagConverter::into_single($vtag, &mut virtual_tags_dyn),)*)?
		];

		let tag_list = $crate::query::query_internals::ReifiedTagList {
			static_tags: &virtual_tags_static,
			dynamic_tags: &virtual_tags_dyn,
		};

		// Acquire storages for all tags we care about
        let token = $crate::query::query_internals::MainThreadToken::acquire_fmt("query entities");
        let mut db = $crate::query::query_internals::DbRoot::get(token);

		$(
			let $name = $crate::query::query_internals::get_storage(&mut db, $name);
			let $name = $crate::query::query_internals::inner_storage_to_api_storage(token, $name);
		)*
		$crate::query::query_internals::drop(db);

		// Run the event handler
		$crate::query::query_internals::QueryableEventList::query_raw(
			&$event_src,
			QueryDiscriminator,
			$crate::query::query_internals::Iterator::map(
				tag_list.iter(),
				|inert_tag| inert_tag.into_dangerous_tag()
			),
			|entity, $event_name| {
				// Inject all the necessary context
				$(
					let $name = $name.get_slot(entity);
					$crate::query::query!(@__internal_xform $prefix $name token);
				)*
				$( let $entity = entity; )?

				// Handle breaks
				// TODO: Can we also handle `return`s?
				let mut did_run = false;
				loop {
					if did_run {
						// The user must have used `continue`.
						return $crate::query::query_internals::ControlFlow::Continue(());
					}
					did_run = true;

					let _: () = {
						$($body)*
					};

					// The user completed the loop.
					#[allow(unreachable_code)]
					{
						return $crate::query::query_internals::ControlFlow::Continue(());
					}
				}

				// The user broke out of the loop.
				#[allow(unreachable_code)]
				{
					$crate::query::query_internals::ControlFlow::Continue(())
				}
			}
		)
	}};

	// === Global query === //
	(
		for (
			$(@$entity:ident $(,)?)?
			$($prefix:ident $name:ident in $tag:expr),*
			$(,)?
		)
		$(+ [$($vtag:expr),*$(,)?])?
		{
			$($body:tt)*
		}
	) => {{
		$crate::query::query_internals::assert!(
			$crate::query::query_internals::try_flush(),
			"Attempted to run a query inside another query, which is forbidden by default. \
				If this behavior is intended, use the `rec for` syntax instead of the `for` syntax."
		);

		$crate::query! {
			rec for (
				$(@$entity)?
				$($prefix $name in $tag,)*
			) $(+ [$($vtag,)*])?
			{
				$($body)*
			}
		}
	}};
	(
		rec for (
			$(@$entity:ident $(,)?)?
			$($prefix:ident $name:ident in $tag:expr),*
			$(,)?
		)
		$(+ [$($vtag:expr),*$(,)?])?
		{
			$($body:tt)*
		}
	) => {{
		let _ = $crate::query::query_internals::try_flush();

		$crate::query! {
			noflush for (
				$(@$entity)?
				$($prefix $name in $tag,)*
			) $(+ [$($vtag,)*])?
			{
				$($body)*
			}
		}
	}};
    (
		noflush for (
			$(@$entity:ident $(,)?)?
			$($prefix:ident $name:ident in $tag:expr),*
			$(,)?
		)
		$(+ [$($vtag:expr),*$(,)?])?
		{
			$($body:tt)*
		}
	) => {'__query: {
		// Evaluate our tag expressions
		$( let $name = $crate::query::query_internals::get_tag($tag); )*

		// Determine tag list
		let mut virtual_tags_dyn = Vec::<$crate::query::query_internals::InertTag>::new();
		let virtual_tags_static = [
			$($crate::query::query_internals::Option::Some($name.1),)*
			$($($crate::query::query_internals::ExtraTagConverter::into_single($vtag, &mut virtual_tags_dyn),)*)?
		];

        // Acquire the main thread token used for our query
        let token = $crate::query::query_internals::MainThreadToken::acquire_fmt("query entities");

        // Acquire the database
        let mut db = $crate::query::query_internals::DbRoot::get(token);

        // Collect the necessary storages and tags
        $( let $name = $crate::query::query_internals::get_storage(&mut db, $name); )*

        // Acquire a chunk iterator
        let chunks = $crate::query::query!(
			@__internal_switch;
			cond: {$(@$entity)?}
			true: {
				db.prepare_named_entity_query($crate::query::query_internals::ReifiedTagList {
					static_tags: &virtual_tags_static,
					dynamic_tags: &virtual_tags_dyn,
				})
			}
			false: {
				db.prepare_anonymous_entity_query($crate::query::query_internals::ReifiedTagList {
					static_tags: &virtual_tags_static,
					dynamic_tags: &virtual_tags_dyn,
				})
			}
		);

		// Acquire a query guard to prevent flushing
        let _guard = db.borrow_query_guard(token);

        // Drop the database to allow safe userland code involving Bort to run
		$crate::query::query_internals::drop(db);

		// For each chunk...
		for chunk in chunks {
			// Fetch the entity iter if it was requested
			$(
				let (chunk, $entity) = chunk.split();
				let mut $entity = $entity.into_iter();
			)?

			// Collect the heaps for each storage
			$( let mut $name = chunk.heaps(&$name.borrow(token)).into_iter(); )*

			// Handle all our heaps
			let mut i = chunk.heap_count();

			while let (
				$($crate::query::query_internals::Option::Some($name),)*
				$($crate::query::query_internals::Option::Some($entity),)?
			) = (
				$($crate::query::query_internals::Iterator::next(&mut $name),)*
				$($crate::query::query_internals::Iterator::next(&mut $entity),)?
			)
			{
				// Determine whether we're the last heap of the chunk
				i -= 1;
				let is_last = i == 0;

				// Construct iterators
				$(
					let mut $name = $name.slots(token)
					.take(if is_last { chunk.last_heap_len() } else { $name.len() });
				)*

				$(
					let mut $entity = if is_last {
						&$entity[..chunk.last_heap_len()]
					} else {
						&$entity
					}
					.iter();
				)?

				// Iterate through every element in this heap
				'__query_ent: while let (
					$($crate::query::query_internals::Option::Some($name),)*
					$($crate::query::query_internals::Option::Some($entity),)?
				) = (
					$($crate::query::query_internals::Iterator::next(&mut $name),)*
					$($crate::query::query_internals::Iterator::next(&mut $entity),)?
				) {
					// Convert the residuals to their target form
					$( $crate::query::query!(@__internal_xform $prefix $name token); )*
					$( let $entity = $entity.get(token).into_dangerous_entity(); )?

					// Run userland code, absorbing their attempt at an early return.
					let mut did_run = false;
					loop {
						if did_run {
							// The user must have used `continue`.
							continue '__query_ent;
						}
						did_run = true;

						let _: () = {
							$($body)*
						};

						// The user completed the loop.
						#[allow(unreachable_code)]
						{
							continue '__query_ent;
						}
					}

					// The user broke out of the loop.
					#[allow(unreachable_code)]
					{
						break '__query;
					}
				}
			}
		}
    }};

	// === Helpers === //
	(
		@__internal_switch;
		cond: {}
		true: {$($true:tt)*}
		false: {$($false:tt)*}
	) => {
		$($false)*
	};
	(
		@__internal_switch;
		cond: {$($there:tt)+}
		true: {$($true:tt)*}
		false: {$($false:tt)*}
	) => {
		$($true)*
	};

	// N.B. these work on both `Slot`s and `DirectSlot`s
	(@__internal_xform ref $name:ident $token:ident) => { let $name = &*$name.borrow($token); };
	(@__internal_xform mut $name:ident $token:ident) => { let $name = &mut *$name.borrow_mut($token); };
	(@__internal_xform oref $name:ident $token:ident) => { let $name = $name.borrow($token); };
	(@__internal_xform omut $name:ident $token:ident) => { let mut $name =  $name.borrow_mut($token); };
	(@__internal_xform slot $name:ident $token:ident) => {};
}

pub use query;
