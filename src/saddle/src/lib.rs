use std::{
    marker::PhantomData,
    sync::atomic::{AtomicBool, Ordering::Relaxed},
};

use self::internal_traits::{Access, MutAccessMode, RefAccessMode};
use self::validator::Validator;

// === Trait internals === //

// TODO: Deny external implementations of these traits

#[doc(hidden)]
pub mod internal_traits {
    use std::any::TypeId;

    use crate::{validator::Mutability, Universe};

    // Namespaces
    pub trait TrackDefinition {
        const LOCATION: &'static str;
    }

    // Access tokens
    pub trait Access<M, U: Universe, T: ?Sized>: Send + Sync {}

    pub struct MutAccessMode;
    pub struct RefAccessMode;

    impl<U, T, K> Access<RefAccessMode, U, T> for K
    where
        U: Universe,
        T: ?Sized,
        K: ?Sized + Access<MutAccessMode, U, T>,
    {
    }

    pub trait AccessAlias {
        type Universe: Universe;
        type AccessIter: Iterator<Item = (TypeId, &'static str, Mutability)>;

        fn iter_access() -> Self::AccessIter;
    }
}

// === Markers === //

pub trait Universe: 'static + Send + Sync {}

pub trait Namespace: 'static + Send + Sync + internal_traits::TrackDefinition {
    type Universe: Universe;
}

#[doc(hidden)]
pub mod marker_macro_internals {
    pub use {
        crate::internal_traits::TrackDefinition,
        std::{column, concat, file, line, stringify},
    };
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

		impl $crate::marker_macro_internals::TrackDefinition for $name {
			const LOCATION: &'static str = $crate::marker_macro_internals::concat!(
				$crate::marker_macro_internals::stringify!($name),
				" (at ",
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
pub trait AccessMut<U: Universe, T: ?Sized>: internal_traits::Access<MutAccessMode, U, T> {
    fn as_dyn_mut(&self) -> &dyn AccessMut<U, T> {
        &SuperDangerousGlobalToken
    }
}

pub trait AccessRef<U: Universe, T: ?Sized>: internal_traits::Access<RefAccessMode, U, T> {
    fn as_dyn(&self) -> &dyn AccessRef<U, T> {
        &SuperDangerousGlobalToken
    }
}

impl<U: Universe, T: ?Sized, K: ?Sized + Access<MutAccessMode, U, T>> AccessMut<U, T> for K {}
impl<U: Universe, T: ?Sized, K: ?Sized + Access<RefAccessMode, U, T>> AccessRef<U, T> for K {}

pub trait BehaviorToken<N: Namespace>: Send + Sync {}

// Global token
pub struct SuperDangerousGlobalToken;

impl<U: Universe, T: ?Sized> Access<MutAccessMode, U, T> for SuperDangerousGlobalToken {}

impl<N: Namespace> BehaviorToken<N> for SuperDangerousGlobalToken {}

// Macros
#[doc(hidden)]
pub mod cx_macro_internals {
    use crate::{validator::Mutability, Universe};

    pub use {
        crate::{
            internal_traits::{Access, AccessAlias, MutAccessMode, RefAccessMode},
            validator::Mutability::{Immutable, Mutable},
        },
        std::{
            any::{type_name, TypeId},
            iter::{IntoIterator, Iterator},
        },
    };

    // Common
    pub trait Dummy {}

    impl<T: ?Sized> Dummy for T {}

    // Trait definition
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

    pub fn bind_and_ensure_in_universe<U: Universe, T: ?Sized + AccessAlias<Universe = U>>() {}
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

		impl $crate::cx_macro_internals::AccessAlias for dyn $name {
			type Universe = $universe;
			type AccessIter =
				$($($crate::cx_macro_internals::TriChain<dyn $inherits, )*)?
				$crate::cx_macro_internals::ArrayIter<{
					$($($crate::cx_macro_internals::bind_and_return_one::<$ty>() +)*)? 0
				}>
				$($(, <dyn $inherits as $crate::cx_macro_internals::AccessAlias>::AccessIter> )*)?;

			fn iter_access() -> Self::AccessIter {
				// Construct the base iterator
				let iter = $crate::cx_macro_internals::IntoIterator::into_iter([$($((
					$crate::cx_macro_internals::TypeId::of::<$ty>(),
					$crate::cx_macro_internals::type_name::<$ty>(),
					$crate::cx!(@__parse_kw_expr $kw),
				)),*)?]);

				// Store the inherited accessors in a cons list in their original order...
				let iters = ();
				$($(let iters = (<dyn $inherits as $crate::cx_macro_internals::AccessAlias>::iter_access(), iters);)*)?

				// ...and pop them out and chain them in their opposite order.
				$($(
					$crate::cx_macro_internals::bind_and_ensure_in_universe::<$universe, dyn $inherits>();
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
    use crate::{BehaviorToken, Namespace};

    pub use {
        crate::{
            internal_traits::{AccessAlias, TrackDefinition},
            validator::Validator,
            SuperDangerousGlobalToken,
        },
        linkme::distributed_slice,
        partial_scope::partial_shadow,
        std::{any::TypeId, column, concat, file, line, marker::Sized},
    };

    #[distributed_slice]
    pub static BEHAVIORS: [fn(&mut Validator)] = [..];

    pub trait BehaviorTokenExt<N: Namespace>: BehaviorToken<N> {
        fn __validate_behavior_token(&mut self) -> BehaviorTokenTyProof<'_, N>;
    }

    pub struct BehaviorTokenTyProof<'a, N> {
        _private: ([&'a (); 0], [N; 0]),
    }

    impl<N: Namespace, T: ?Sized + BehaviorToken<N>> BehaviorTokenExt<N> for T {
        fn __validate_behavior_token(&mut self) -> BehaviorTokenTyProof<'_, N> {
            BehaviorTokenTyProof { _private: ([], []) }
        }
    }

    pub fn validate_behavior_token<N: Namespace>(
        bhv: BehaviorTokenTyProof<'_, N>,
    ) -> BehaviorTokenTyProof<'_, N> {
        bhv
    }
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

		// TODO: Re-enable shadowing once IDE support improves
		$(/*$crate::behavior_macro_internals::partial_shadow! {
			$cx_name, $bhv_name;*/

			let __token = {
				// Define a trait describing the set of components we're acquiring.
				$crate::cx! {
					trait BehaviorAccess(<$namespace as $crate::Namespace>::Universe)
						: $($comp_inherits),*
						$(= $($comp_kw $comp_ty),*)?;
				};

				// Define a registration method
				#[$crate::behavior_macro_internals::distributed_slice($crate::behavior_macro_internals::BEHAVIORS)]
				fn register(validator: &mut $crate::behavior_macro_internals::Validator) {
					validator.add_behavior(
						/* namespace: */ (
							$crate::behavior_macro_internals::TypeId::of::<$namespace>(),
							<$namespace as $crate::behavior_macro_internals::TrackDefinition>::LOCATION,
						),
						/* my_path: */ $crate::behavior_macro_internals::concat!(
							$crate::behavior_macro_internals::file!(),
							":",
							$crate::behavior_macro_internals::line!(),
							":",
							$crate::behavior_macro_internals::column!(),
						),
						/* borrows: */ <dyn BehaviorAccess as $crate::behavior_macro_internals::AccessAlias>::iter_access(),
						/* calls: */ [$((
							$crate::behavior_macro_internals::TypeId::of::<$bhv_ty>(),
							<$bhv_ty as $crate::behavior_macro_internals::TrackDefinition>::LOCATION,
						)),*],
					);
				}

				// Fetch a token
				fn get_token<'a>() -> impl BehaviorAccess {
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
		/*}*/)*

		let _ = __input;
	};
}

// === Validation === //

pub mod validator;

impl Validator {
    pub fn global() -> Self {
        let mut validator = Validator::default();

        for bhv in behavior_macro_internals::BEHAVIORS {
            bhv(&mut validator);
        }

        validator
    }
}

pub fn validate() -> Result<(), String> {
    static HAS_VALIDATED: AtomicBool = AtomicBool::new(false);

    if !HAS_VALIDATED.load(Relaxed) {
        Validator::global().validate()?;
        HAS_VALIDATED.store(true, Relaxed);
    }

    Ok(())
}

// === Entry === //

pub struct RootBehaviorToken<U: Universe> {
    _private: PhantomData<fn() -> U>,
}

impl<U: Universe, N: Namespace<Universe = U>> BehaviorToken<N> for RootBehaviorToken<U> {}

impl<U: Universe> RootBehaviorToken<U> {
    // TODO: Enforce singleton rules
    pub fn acquire() -> Self {
        if let Err(err) = validate() {
            panic!("{err}");
        }

        Self {
            _private: PhantomData,
        }
    }
}

impl<U: Universe> Drop for RootBehaviorToken<U> {
    fn drop(&mut self) {
        // (no-op for now, kept for forwards compatibility)
    }
}
