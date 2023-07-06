use std::{fmt, marker::PhantomData};

use derive_where::derive_where;

use crate::{
    core::token::MainThreadToken,
    database::{DbRoot, DbStorage, InertTag},
    util::misc::NamedTypeId,
};

// === Tag === //

pub(crate) struct VirtualTagMarker {
    _never: (),
}

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

    pub fn raw(self) -> RawTag {
        self.raw
    }
}

impl<T> Into<RawTag> for Tag<T> {
    fn into(self) -> RawTag {
        self.raw
    }
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct VirtualTag {
    raw: RawTag,
}

impl VirtualTag {
    pub fn new() -> Self {
        Self {
            raw: RawTag::new(NamedTypeId::of::<VirtualTagMarker>()),
        }
    }

    pub fn raw(self) -> RawTag {
        self.raw
    }
}

impl Into<RawTag> for VirtualTag {
    fn into(self) -> RawTag {
        self.raw
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
}

impl fmt::Debug for RawTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawTag")
            .field("id", &self.0.id())
            .field("ty", &self.0.ty())
            .finish()
    }
}

// === Flushing === //

pub fn flush() {
    let token = MainThreadToken::acquire_fmt("flush entity archetypes");
    DbRoot::get(token).flush_archetypes(token);
}

// === Queries === //

#[doc(hidden)]
pub mod query_internals {
    use super::*;

    // === Re-exports === //

    pub use {
        crate::{
            core::token::MainThreadToken,
            database::{DbRoot, InertTag},
        },
        std::{
            hint::unreachable_unchecked,
            iter::{empty, Iterator},
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
            extra.extend(self.into_iter().map(|v| v.into().0));
            None
        }
    }
}

#[macro_export]
macro_rules! query {
    (
		for ($(@$entity:ident;)? $($prefix:ident $name:ident in $tag:expr),+$(,)?) $(+ [$($vtag:expr),*$(,)?])? {$($body:tt)*}
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

        // Acquire a query guard to prevent flushing
        let _guard = db.borrow_query_guard(token);

        // Acquire a chunk iterator
        let chunks = $crate::query::query!(
			@__internal_switch;
			cond: {$(@$entity)?}
			true: {
				db.prepare_named_entity_query(&virtual_tags_static, &virtual_tags_dyn)
			}
			false: {
				db.prepare_anonymous_entity_query(&virtual_tags_static, &virtual_tags_dyn)
			}
		);

        // Drop the database to allow safe userland code involving Bort to run
		drop(db);

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
				while let (
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
					let mut did_complete = false;
					loop {
						$($body)*
						did_complete = true;
						break;
					}

					if !did_complete {
						break '__query;
					}
				}
			}
		}
    }};
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
	(@__internal_xform ref $name:ident $token:ident) => { let $name = $name.borrow($token); };
	(@__internal_xform mut $name:ident $token:ident) => { let mut $name = $name.borrow_mut($token); };
	(@__internal_xform oref $name:ident $token:ident) => { let $name = &*$name.borrow($token); };
	(@__internal_xform omut $name:ident $token:ident) => { let $name = &mut *$name.borrow_mut($token); };
	(@__internal_xform slot $name:ident $token:ident) => {};
}

pub use query;
