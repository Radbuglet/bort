// === Core === //

pub trait Component: 'static + Sized + Send + Sync {}

impl<T: 'static + Send + Sync> Component for T {}

// === Access === //

pub trait AccessMut<T: Component>: AccessRef<T> {}

pub trait AccessRef<T: Component> {}

#[doc(hidden)]
pub mod cx_macro_internals {
    use crate::{AccessMut, AccessRef, Component};

    pub trait Dummy {}

    impl<T: ?Sized> Dummy for T {}

    pub struct MutAccessMode;
    pub struct RefAccessMode;

    pub trait Access<M, T: Component> {}

    impl<K: ?Sized + AccessMut<T>, T: Component> Access<MutAccessMode, T> for K {}
    impl<K: ?Sized + AccessRef<T>, T: Component> Access<RefAccessMode, T> for K {}
}

#[macro_export]
macro_rules! cx {
    ($($kw:ident $ty:ty),*$(,)?) => {
		impl $($crate::cx_macro_internals::Access<$crate::cx!(@__parse_kw $kw), $ty>+)* ?Sized
	};
	($(
		$vis:vis trait $name:ident$(: $($inherits:path)*)? $(= $($kw:ident $ty:ty)|+)?;
	)*) => {$(
		$vis trait $name: $crate::cx_macro_internals::Dummy
			$($(+ $inherits)*)?
			$($(+ $crate::cx_macro_internals::Access<$crate::cx!(@__parse_kw $kw), $ty>)*)?
		{
		}

		impl<T> $name for T
		where
			T: ?Sized $($(+ $inherits)*)?
			   $($(+ $crate::cx_macro_internals::Access<$crate::cx!(@__parse_kw $kw), $ty>)*)?
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

// === Behaviors === //

// Core
pub struct BehaviorRegistry {}

pub trait Delegate {}

pub trait ClosureForDelegate<D: Delegate> {}

// Macros
#[macro_export]
macro_rules! delegate {
    () => {};
}

#[macro_export]
macro_rules! behavior {
    ($(
		$(#[$bhv_meta:meta])*
		$bhv_vis:vis $bhv_name:ident $(: $($bhv_extends:path),*)? {$(
			$impl_bhv:ty => (
				$impl_bhv_para:ident: [$($impl_bhv_called:tt),*$(,)?],
				$impl_cx_para:ident: [
					$($impl_cx_include_ty:ty),*

					$(
						;
						$($impl_cx_single_kw:ident $impl_cx_single_ty:ty),*$(,)?
					)?
				]
				$(, $($para_name:ident $(: $para_ty:ty)?)*)?
			) {
				$($body:tt)*
			}
		)*}
	)*) => {$(
		$(#[$bhv_meta])*
		$bhv_vis fn $bhv_name(registry: &mut ()) {
			let _ = &registry;
		}
	)*};
}

behavior! {
    pub register {
        Foo => (bhv: [Bar, Baz, Maz], cx: [Cx; mut Foo, ref Bar]) {

        }
    }
}

// === Entities === //

// TODO

// === Events === //

// TODO

// === Tags === //

// TODO

// === Queries === //

// TODO
