#[doc(hidden)]
pub mod macro_internals {
    use derive_where::derive_where;
    use std::marker::PhantomData;

    pub use {
        super::auto_reborrow,
        std::{ops::Drop, option::Option},
    };

    #[derive_where(Copy, Clone)]
    pub struct BoundTy<T>(PhantomData<fn(T) -> T>);

    impl<T> BoundTy<T> {
        pub fn new() -> Self {
            Self(PhantomData)
        }

        pub fn bind_fn_result<F, A>(self, _f: F)
        where
            F: Fn(A) -> T,
        {
        }

        pub fn bind_ref(self, value: &T) -> &T {
            value
        }

        pub fn bind_mut(self, value: &mut T) -> &mut T {
            value
        }

        pub fn make_none(self) -> Option<T> {
            None
        }
    }

    pub struct AutoReborrow<A, B, C> {
        pub cache: A,
        pub populators: B,
        pub clear: C,
    }
}

#[macro_export]
macro_rules! auto_reborrow {
	(
		$cx:ident $(: $($name:ident),* $(,)?)? => $body:expr
	) => {{
		let $crate::reborrow::macro_internals::AutoReborrow { cache, populators, clear } = &mut $cx;

		struct ClearGuard<'a, A, C: Fn(&mut A)> {
			cache: &'a mut A,
			clear: &'a mut C,
		}

		impl<'a, A, C: Fn(&mut A)> $crate::reborrow::macro_internals::Drop for ClearGuard<'a, A, C> {
			fn drop(&mut self) {
				(self.clear)(self.cache)
			}
		}

		let mut guard = ClearGuard { cache, clear };

		// Populate the cache.
		$($((populators.$name)(&mut guard.cache);)*)?

		// Inject dependencies
		$($( let $name = guard.cache.$name.as_mut().unwrap(); )*)?

		$body

	}};
	(
		$cx:ident
		$(: $($name:ident),* $(,)?)?
	) => {
		#[allow(unused_variables)]
		let $crate::reborrow::macro_internals::AutoReborrow {
			cache: __cache,
			populators: __populators,
			clear: __clear,
		} = &mut $cx;

		// Clear the cache
		__clear(__cache);

		// Populate the cache
		$($((__populators.$name)(&mut *__cache);)*)?

		// Inject dependencies
		$($( let $name = __cache.$name.as_mut().unwrap(); )*)?
	};
    (
		$(let $name:ident$(($($dep:ident),*$(,)?))? = $rb:expr;)*
	) => {{
		#[allow(non_camel_case_types)]
		struct Container<$($name),*> { $($name: $name),* }

		// Gather closures
		let (raw_populators, result_types) = {
			let result_types = Container { $($name: $crate::reborrow::macro_internals::BoundTy::new()),* };

			$(
				let $name = move |($($($dep,)*)?): ($($( $crate::reborrow::macro_internals::auto_reborrow!(@__bind_dep_and_give_mut_ref; $dep), )*)?)| {
					$($( let $dep = result_types.$dep.bind_mut($dep); )*)?
					$rb
				};
				result_types.$name.bind_fn_result(&$name);
			)*

			(Container { $($name),* }, result_types)
		};

		// Construct cache
		let cache = Container { $($name: result_types.$name.make_none()),* };
		let cache_ty = $crate::reborrow::macro_internals::BoundTy::new();
		cache_ty.bind_ref(&cache);

		// Construct cache populator
		let populators = {
			$(
				let $name = move |cache: &mut _| {
					let cache = cache_ty.bind_mut(cache);

					if cache.$name.is_none() {
						// Populate dependencies
						$($(
							$dep(&mut *cache);
						)*)?

						// Fetch dependencies
						$($(let $dep = cache.$dep.as_mut().unwrap();)*)?

						// Compute value
						let value = (raw_populators.$name)(($($($dep,)*)?));

						cache.$name = $crate::reborrow::macro_internals::Option::Some(value);
					}
				};
			)*

			Container { $($name),* }
		};

		// Create cache clearer
		let clear = move |cache: &mut _| {
			let cache = cache_ty.bind_mut(cache);
			$(cache.$name = $crate::reborrow::macro_internals::Option::None;)*
		};

		$crate::reborrow::macro_internals::AutoReborrow { cache, populators, clear }
	}};
	(@__bind_dep_and_give_mut_ref; $dep:ident) => { &mut _ };
}

pub use auto_reborrow;
