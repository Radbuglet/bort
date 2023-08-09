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

// === Access Tokens === //

pub unsafe trait AccessMut<U: Universe, T: ?Sized>: AccessRef<U, T> {}

pub unsafe trait AccessRef<U: Universe, T: ?Sized>: Sized {}

pub unsafe trait BehaviorToken<N: Namespace> {}

#[doc(hidden)]
pub mod cx_macro_internals {
    use crate::{AccessMut, AccessRef, Universe};

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
		impl $($crate::cx_macro_internals::Access<$crate::cx!(@__parse_kw $kw), $universe, $ty>+)* $($($inherits+)*)? Sized
	};
	($(
		$vis:vis trait $name:ident($universe:ty) $(: $($inherits:path)*)? $(= $($kw:ident $ty:ty)|+)?;
	)*) => {$(
		$vis trait $name: $crate::cx_macro_internals::Dummy
			$($(+ $inherits)*)?
			$($(+ $crate::cx_macro_internals::Access<$universe, $crate::cx!(@__parse_kw $kw), $ty>)*)?
		{
		}

		impl<T> $name for T
		where
			T: ?Sized $($(+ $inherits)*)?
			   $($(+ $crate::cx_macro_internals::Access<$universe, $crate::cx!(@__parse_kw $kw), $ty>)*)?
		{
		}
	)*};
	(@__parse_kw mut) => {
		$crate::cx_macro_internals::MutAccessMode
	};
	(@__parse_kw ref) => {
		$crate::cx_macro_internals::RefAccessMode
	};
}

// === Behavior === //

#[doc(hidden)]
pub mod behavior_macro_internals {
    use crate::{AccessMut, AccessRef, BehaviorToken, Namespace, Universe};

    pub fn validate_behavior_token<N: Namespace>(bhv: &impl BehaviorToken<N>) {
        let _ = bhv;
    }

    pub struct SuperDangerousGlobalToken;

    unsafe impl<U: Universe, T: ?Sized> AccessMut<U, T> for SuperDangerousGlobalToken {}

    unsafe impl<U: Universe, T: ?Sized> AccessRef<U, T> for SuperDangerousGlobalToken {}

    unsafe impl<N: Namespace> BehaviorToken<N> for SuperDangerousGlobalToken {}
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
		// TODO: Register in global registry
		$crate::behavior_macro_internals::validate_behavior_token::<$namespace>($in_bhv);

		let __token = {
			fn get_token<'a>() -> $crate::cx![
				<$namespace as $crate::Namespace>::Universe;
				$($($comp_kw $comp_ty),*)? ; $($comp_inherits),*
			] {
				$crate::behavior_macro_internals::SuperDangerousGlobalToken
			}

			get_token()
		};
		let $token = &__token;

		let __bhv = {
			fn get_token() -> impl $($crate::BehaviorToken<$bhv_ty> +)* Sized {
				$crate::behavior_macro_internals::SuperDangerousGlobalToken
			}

			get_token()
		};
		let $bhv = &__bhv;

		$($body)*

		let _ = (__token, __bhv);
	};
}

// TODO: Provide an entry point

// TODO: What happens if someone forwards their behavior token to ours? (maybe namespace tokens by their originating token?)
