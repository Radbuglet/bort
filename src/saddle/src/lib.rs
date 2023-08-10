use std::{any::TypeId, marker::PhantomData};

// === Markers === //

pub trait Universe: Sized + 'static {}

pub trait Namespace: Sized + 'static {
    type Universe: Universe;
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
	)*}
}

// === Validator === //

#[doc(hidden)]
pub mod validator_internals {
    use crate::AccessReflectVec;

    use linkme::distributed_slice;
    use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

    pub use std::any::{type_name, TypeId};

    #[distributed_slice]
    pub static VALIDATOR_PARTS: [fn(&mut Validator)] = [..];

    #[derive(Debug, Default)]
    pub struct Validator {}

    impl Validator {
        pub fn push(
            &mut self,
            universe: TypeId,
            namespace: TypeId,
            namespace_name: &'static str,
            access: AccessReflectVec,
            calls: &[TypeId],
        ) {
            todo!()
        }
    }

    pub fn ensure_validated() {
        static HAS_VALIDATED: AtomicBool = AtomicBool::new(false);

        // This may run multiple times but the computation is pure so this does not cause issues and,
        // given that users typically acquire the token on a singular main thread, this should only
        // rarely cause redundant computations.
        if !HAS_VALIDATED.load(Relaxed) {
            let mut validator = Validator::default();

            for part in VALIDATOR_PARTS.iter() {
                part(&mut validator);
            }

            HAS_VALIDATED.store(true, Relaxed);
        }
    }
}

// === Access Tokens === //

// Traits
pub trait AccessMut<U: Universe, T: ?Sized>: AccessRef<U, T> {}

pub trait AccessRef<U: Universe, T: ?Sized> {}

pub type AccessReflectVec = Vec<(TypeId, &'static str, bool)>;

pub trait AccessReflect {
    type Universe: Universe;

    fn push_access_array(accesses: &mut AccessReflectVec);

    fn access_array() -> AccessReflectVec {
        let mut tmp = Vec::new();
        Self::push_access_array(&mut tmp);
        tmp
    }
}

pub trait BehaviorToken<N: Namespace> {}

// Macros
#[doc(hidden)]
pub mod cx_macro_internals {
    use crate::{AccessMut, AccessRef, AccessReflect, Universe};

    pub use std::any::{type_name, TypeId};

    pub fn ensure_universes_match<A, B>()
    where
        A: ?Sized + AccessReflect,
        B: ?Sized + AccessReflect<Universe = A::Universe>,
    {
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

		impl $crate::AccessReflect for dyn $name {
			type Universe = $universe;

			fn push_access_array(accesses: &mut $crate::AccessReflectVec) {
				$($(
					accesses.push((
						$crate::cx_macro_internals::TypeId::of::<$ty>(),
						$crate::cx_macro_internals::type_name::<$ty>(),
						$crate::cx!(@__parse_kw_is_mut $kw),
					));
				)*)?
				$($(
					$crate::cx_macro_internals::ensure_universes_match::<Self, dyn $inherits>();
					<dyn $inherits as $crate::AccessReflect>::push_access_array(accesses);
				)*)?
			}
		}
	)*};
	(@__parse_kw mut) => {
		$crate::cx_macro_internals::MutAccessMode
	};
	(@__parse_kw ref) => {
		$crate::cx_macro_internals::RefAccessMode
	};
	(@__parse_kw_is_mut mut) => {
		true
	};
	(@__parse_kw_is_mut ref) => {
		false
	};
}

// === Behavior === //

#[doc(hidden)]
pub mod behavior_macro_internals {
    use crate::{AccessMut, AccessRef, BehaviorToken, Namespace, Universe};

    pub use {linkme::distributed_slice, std::marker::Sized};

    pub trait BehaviorTokenExt<N: Namespace>: BehaviorToken<N> {
        fn __validate_behavior_token(&mut self) -> BehaviorTokenTyProof<'_>;
    }

    pub struct BehaviorTokenTyProof<'a> {
        _private: [&'a (); 0],
    }

    impl<N: Namespace, T: BehaviorToken<N>> BehaviorTokenExt<N> for T {
        fn __validate_behavior_token(&mut self) -> BehaviorTokenTyProof<'_> {
            BehaviorTokenTyProof { _private: [] }
        }
    }

    pub fn validate_behavior_token(bhv: BehaviorTokenTyProof<'_>) -> BehaviorTokenTyProof<'_> {
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
		in $namespace:ty,
		use authentication $in_bhv:expr,
		use tokens [
			$($comp_inherits:ty),*$(,)?
			$(; $($comp_kw:ident $comp_ty:ty),*$(,)?)*
		] as $token:ident,
		use behaviors [$($bhv_ty:ty),*$(,)?] as $bhv:ident,
		do {
			$($body:tt)*
		}
	) => {
		let (__input, __token) = {
			// Define a trait describing the set of components we're acquiring.
			$crate::cx! {
				trait __OurAccess(<$namespace as $crate::Namespace>::Universe)
					: $($comp_inherits),*
					$(= $($comp_kw $comp_ty),*)?;
			};

			// Register a validator method for this trait.
			#[$crate::behavior_macro_internals::distributed_slice($crate::validator_internals::VALIDATOR_PARTS)]
			fn __validate(validator: &mut $crate::validator_internals::Validator) {
				validator.push(
					$crate::validator_internals::TypeId::of::<<$namespace as $crate::Namespace>::Universe>(),
					$crate::validator_internals::TypeId::of::<$namespace>(),
					$crate::validator_internals::type_name::<$namespace>(),
					<dyn __OurAccess as $crate::AccessReflect>::access_array(),
					&[$($crate::validator_internals::TypeId::of::<$bhv_ty>())*],
				);
			}

			// Fetch a token
			fn get_token<'a>() -> impl __OurAccess {
				$crate::behavior_macro_internals::SuperDangerousGlobalToken
			}

			use $crate::behavior_macro_internals::BehaviorTokenExt as _;

			(
				// Validate the behavior token
				$crate::behavior_macro_internals::validate_behavior_token($in_bhv.__validate_behavior_token()),
				// Create a new component access token
				get_token(),
			)
		};

		let $token = &__token;

		let mut __bhv = {
			fn get_token() -> impl $($crate::BehaviorToken<$bhv_ty> +)* $crate::behavior_macro_internals::Sized {
				$crate::behavior_macro_internals::SuperDangerousGlobalToken
			}

			get_token()
		};
		let $bhv = &mut __bhv;

		$($body)*

		let _ = (__input, __token, __bhv);
	};
}

// === Entry === //

pub struct AnyBehaviorToken<U> {
    _private: PhantomData<fn() -> U>,
}

impl<U: Universe, N: Namespace<Universe = U>> BehaviorToken<N> for AnyBehaviorToken<U> {}

impl<U: Universe> AnyBehaviorToken<U> {
    pub fn acquire() -> Self {
        todo!();
    }
}
