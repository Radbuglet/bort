use std::marker::PhantomData;

// === Trait internals === //

#[doc(hidden)]
pub mod trait_internals {
    use std::any::TypeId;

    use crate::validator::Mutability;

    pub trait TrackDefinition {
        const LOCATION: &'static str;
    }

    pub trait AccessAlias {
        type AccessIter: IntoIterator<Item = (TypeId, &'static str, Mutability)>;

        fn iter_access() -> Self::AccessIter;
    }
}

// === Markers === //

pub trait Universe: 'static + Send + Sync + trait_internals::TrackDefinition {}

pub trait Namespace: 'static + Send + Sync + trait_internals::TrackDefinition {
    type Universe: Universe;
}

#[doc(hidden)]
pub mod marker_macro_internals {
    pub use std::{column, concat, file, line, stringify};
}

#[macro_export]
macro_rules! universe {
    ($(
		$(#[$meta:meta])*
		$vis:vis $name:ident$(;)?
	)*) => {$(
		$(#[$meta])*
		$vis struct $name { _marker: () }

		impl $crate::Universe for $name {}

		impl $crate::trait_internals::TrackDefinition for $name {
			const LOCATION: &'static str = $crate::marker_macro_internals::concat!(
				$crate::marker_macro_internals::stringify!($name),
				"(at ",
				$crate::marker_macro_internals::file!(),
				":",
				$crate::marker_macro_internals::line!(),
				":",
				$crate::marker_macro_internals::column!(),
				")"
			);
		}
	)*}
}

#[macro_export]
macro_rules! namespace {
    ($(
		$(#[$meta:meta])*
		$vis:vis $name:ident in $universe:ty$(;)?
	)*) => {$(
		$(#[$meta])*
		$vis struct $name { _marker: () }

		impl $crate::Namespace for $name {
			type Universe = $universe;
		}

		impl $crate::trait_internals::TrackDefinition for $name {
			const LOCATION: &'static str = $crate::marker_macro_internals::concat!(
				$crate::marker_macro_internals::stringify!($name),
				"(at ",
				$crate::marker_macro_internals::file!(),
				":",
				$crate::marker_macro_internals::line!(),
				":",
				$crate::marker_macro_internals::column!(),
				")"
			);
		}
	)*}
}

// === Access Tokens === //

// Traits
pub trait AccessMut<U: Universe, T: ?Sized>: Send + Sync + AccessRef<U, T> {}

pub trait AccessRef<U: Universe, T: ?Sized>: Send + Sync {}

pub trait BehaviorToken<N: Namespace>: Send + Sync {}

// Macros
#[doc(hidden)]
pub mod cx_macro_internals {
    use crate::{validator::Mutability, AccessMut, AccessRef, Universe};

    pub use {
        crate::validator::Mutability::{Immutable, Mutable},
        std::{
            any::{type_name, TypeId},
            iter::{IntoIterator, Iterator},
        },
    };

    pub type TriChain<_Ignored, A, B> = <_Ignored as TriChainInner<A, B>>::Output;
    pub type ArrayIter<const N: usize> =
        std::array::IntoIter<(TypeId, &'static str, Mutability), N>;

    pub trait TriChainInner<A, B> {
        type Output;
    }

    impl<T: ?Sized, A, B> TriChainInner<A, B> for T {
        type Output = std::iter::Chain<A, B>;
    }

    pub const fn bind_and_return_one<T: ?Sized>() -> usize {
        1
    }

    pub trait Dummy {}

    impl<T: ?Sized> Dummy for T {}

    pub struct MutAccessMode;
    pub struct RefAccessMode;

    pub trait Access<M, U: Universe, T: ?Sized> {}

    impl<U: Universe, K: ?Sized + AccessMut<U, T>, T: ?Sized> Access<MutAccessMode, U, T> for K {}
    impl<U: Universe, K: ?Sized + AccessRef<U, T>, T: ?Sized> Access<RefAccessMode, U, T> for K {}
}

#[macro_export]
macro_rules! cx {
    ($universe:ty; $($kw:ident $ty:ty),*$(,)? $(; $($inherits:ty)*$(,)?)?) => {
		impl $($crate::cx_macro_internals::Access<$crate::cx!(@__parse_kw $kw), $universe, $ty>+)* $($($inherits+)*)? ?Sized
	};
	($(
		$vis:vis trait $name:ident($universe:ty) $(: $($inherits:path),*)? $(= $($kw:ident $ty:ty),+)?;
	)*) => {$(
		$vis trait $name: $crate::cx_macro_internals::Dummy
			$($(+ $inherits)*)?
			$($(+ $crate::cx_macro_internals::Access<$crate::cx!(@__parse_kw $kw), $universe, $ty>)*)?
		{
		}

		impl<T> $name for T
		where
			T: ?Sized $($(+ $inherits)*)?
			   $($(+ $crate::cx_macro_internals::Access<$crate::cx!(@__parse_kw $kw), $universe, $ty>)*)?
		{
		}

		impl $crate::trait_internals::AccessAlias for dyn $name {
			type AccessIter =
				$($($crate::cx_macro_internals::TriChain<dyn $inherits, )*)?
				$crate::cx_macro_internals::ArrayIter<{
					$($($crate::cx_macro_internals::bind_and_return_one::<$ty>() +)*)? 0
				}>
				$($(, <dyn $inherits as $crate::trait_internals::AccessAlias>::AccessIter> )*)?;

			fn iter_access() -> Self::AccessIter {
				// Construct the base iterator
				let iter = $crate::cx_macro_internals::IntoIterator::into_iter([$($((
					$crate::cx_macro_internals::TypeId::of::<$ty>(),
					$crate::cx_macro_internals::type_name::<$ty>(),
					$crate::cx!(@__parse_kw_expr $kw),
				)),*)?]);

				// Store the inherited accessors in a cons list in their original order...
				let iters = ();
				$($(let iters = (<dyn $inherits as $crate::trait_internals::AccessAlias>::iter_access(), iters);)*)?

				// ...and pop them out and chain them in their opposite order.
				$($(
					let _ = $crate::cx_macro_internals::bind_and_return_one::<dyn $inherits>();
					let (next_iter, iters) = iters;
					let iter = $crate::cx_macro_internals::Iterator::chain(iter, next_iter);
				)*)?

				iter
			}
		}
	)*};
	(@__parse_kw mut) => { $crate::cx_macro_internals::MutAccessMode };
	(@__parse_kw ref) => { $crate::cx_macro_internals::RefAccessMode };
	(@__parse_kw_expr mut) => { $crate::cx_macro_internals::Mutable };
	(@__parse_kw_expr ref) => { $crate::cx_macro_internals::Immutable };
}

// === Behavior === //

#[doc(hidden)]
pub mod behavior_macro_internals {
    use crate::{AccessMut, AccessRef, BehaviorToken, Namespace, Universe};

    pub use {linkme::distributed_slice, std::marker::Sized};

    pub trait BehaviorTokenExt<N: Namespace>: BehaviorToken<N> {
        fn __validate_behavior_token(&mut self) -> BehaviorTokenTyProof<'_, N>;
    }

    pub struct BehaviorTokenTyProof<'a, N> {
        _private: ([&'a (); 0], [N; 0]),
    }

    impl<N: Namespace, T: BehaviorToken<N>> BehaviorTokenExt<N> for T {
        fn __validate_behavior_token(&mut self) -> BehaviorTokenTyProof<'_, N> {
            BehaviorTokenTyProof { _private: ([], []) }
        }
    }

    pub fn validate_behavior_token<N: Namespace>(
        bhv: BehaviorTokenTyProof<'_, N>,
    ) -> BehaviorTokenTyProof<'_, N> {
        bhv
    }

    pub struct SuperDangerousGlobalToken;

    impl<U: Universe, T: ?Sized> AccessMut<U, T> for SuperDangerousGlobalToken {}

    impl<U: Universe, T: ?Sized> AccessRef<U, T> for SuperDangerousGlobalToken {}

    impl<N: Namespace> BehaviorToken<N> for SuperDangerousGlobalToken {}
}

#[macro_export]
macro_rules! behavior {
    (
		as $namespace:ty[$in_bhv:expr] do
		$(
			(
				$cx_name:ident: [
					$($comp_inherits:ty),*$(,)?
					$(; $($comp_kw:ident $comp_ty:ty),*$(,)?)*
				],
				$bhv_name:ident: [
					$($bhv_ty:ty),*$(,)?
				]
				$(,)?
			) {
				$($body:tt)*
			}
			$(,)?
		)*
	) => {
		let __input = {
			use $crate::behavior_macro_internals::BehaviorTokenExt as _;
			// Validate the behavior token
			$crate::behavior_macro_internals::validate_behavior_token::<$namespace>(
				$in_bhv.__validate_behavior_token()
			)
		};

		$(
			let __token = {
				// Define a trait describing the set of components we're acquiring.
				$crate::cx! {
					trait __OurAccess(<$namespace as $crate::Namespace>::Universe)
						: $($comp_inherits),*
						$(= $($comp_kw $comp_ty),*)?;
				};

				// Fetch a token
				fn get_token<'a>() -> impl __OurAccess {
					$crate::behavior_macro_internals::SuperDangerousGlobalToken
				}

				get_token()
			};

			let $cx_name = &__token;

			let mut __bhv = {
				fn get_token() -> impl $($crate::BehaviorToken<$bhv_ty> +)* $crate::behavior_macro_internals::Sized {
					$crate::behavior_macro_internals::SuperDangerousGlobalToken
				}

				get_token()
			};
			let $bhv_name = &mut __bhv;

			$($body)*

			let _ = (__token, __bhv);
		)*

		let _ = __input;
	};
}

// === Validation === //

pub mod validator;

// TODO: Expose graph-printing behavior

// === Entry === //

pub struct RootBehaviorToken<U> {
    _private: PhantomData<fn() -> U>,
}

impl<U: Universe, N: Namespace<Universe = U>> BehaviorToken<N> for RootBehaviorToken<U> {}

impl<U: Universe> RootBehaviorToken<U> {
    pub fn acquire() -> Self {
        todo!();
    }
}
