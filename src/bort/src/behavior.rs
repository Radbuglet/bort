use crate::{
    entity::Entity,
    event::ProcessableEventList,
    query::VirtualTag,
    util::{
        hash_map::{ConstSafeBuildHasherDefault, FxHashMap},
        misc::{MapFmt, NamedTypeId, RawFmt},
    },
    CompMut, CompRef,
};

use std::{
    any::Any,
    fmt,
    ops::{Deref, DerefMut},
};

// === Injectors === //

pub trait FuncMethodInjectorRef<T: ?Sized> {
    type Guard<'a>: Deref<Target = T>;
    type Injector;

    const INJECTOR: Self::Injector;
}

pub trait FuncMethodInjectorMut<T: ?Sized> {
    type Guard<'a>: DerefMut<Target = T>;
    type Injector;

    const INJECTOR: Self::Injector;
}

// === Dispatchable === //

pub trait Dispatchable<A> {
    type Output;

    fn dispatch(&self, args: A) -> Self::Output;
}

// === Delegate === //

#[doc(hidden)]
pub mod delegate_macro_internal {
    use std::ops::DerefMut;

    // === Re-exports === //

    pub use {
        super::{Dispatchable, FuncMethodInjectorMut, FuncMethodInjectorRef},
        std::{
            clone::Clone,
            convert::From,
            fmt,
            marker::{PhantomData, Send, Sync},
            ops::Deref,
            stringify,
            sync::Arc,
        },
    };

    // === Private helpers === //

    pub trait FuncMethodInjectorRefGetGuard<T: ?Sized> {
        type GuardHelper<'a>: Deref<Target = T>;
    }

    impl<G, T> FuncMethodInjectorRefGetGuard<T> for G
    where
        T: ?Sized,
        G: FuncMethodInjectorRef<T>,
    {
        type GuardHelper<'a> = G::Guard<'a>;
    }

    pub trait FuncMethodInjectorMutGetGuard<T: ?Sized> {
        type GuardHelper<'a>: DerefMut<Target = T>;
    }

    impl<G, T> FuncMethodInjectorMutGetGuard<T> for G
    where
        T: ?Sized,
        G: FuncMethodInjectorMut<T>,
    {
        type GuardHelper<'a> = G::Guard<'a>;
    }
}

#[macro_export]
macro_rules! delegate {
	// === With injector === //
	(
		$(#[$attr_meta:meta])*
		$vis:vis fn $name:ident
			$(
				<$($generic:ident),* $(,)?>
				$(<$($fn_lt:lifetime),* $(,)?>)?
			)?
			(
				&$inj_lt:lifetime self [$($inj_name:ident: $inj:ty),* $(,)?]
				$(, $para_name:ident: $para:ty)* $(,)?
			) $(-> $ret:ty)?
		$(as deriving $deriving:path $({ $($deriving_args:tt)* })? )*
		$(where $($where_token:tt)*)?
	) => {
		$crate::behavior::delegate! {
			$(#[$attr_meta])*
			$vis fn $name
				< $($($generic),*)? >
				< $inj_lt, $($($($fn_lt),*)?)? >
				(
					$($inj_name: $inj,)*
					$($para_name: $para,)*
				) $(-> $ret)?
			$(as deriving $deriving $({ $($deriving_args)* })? )*
			$(where $($where_token)*)?
		}

		impl$(<$($generic),*>)? $name $(<$($generic),*>)?
		$(where
			$($where_token)*
		)? {
			#[allow(unused)]
			pub fn new_method_ref<Injector, Receiver, Func>(_injector: Injector, handler: Func) -> Self
			where
				Injector: 'static + $crate::behavior::delegate_macro_internal::FuncMethodInjectorRefGetGuard<Receiver>,
				Injector: $crate::behavior::delegate_macro_internal::FuncMethodInjectorRef<
					Receiver,
					Injector = for<
						$inj_lt
						$($(
							$(,$fn_lt)*
						)?)?
					> fn(
						&$inj_lt (),
						$(&mut $inj),*
					) -> Injector::GuardHelper<$inj_lt>>,
				Receiver: ?Sized + 'static,
				Func: 'static
					+ $crate::behavior::delegate_macro_internal::Send
					+ $crate::behavior::delegate_macro_internal::Sync
					+ for<$inj_lt $($( $(,$fn_lt)* )?)?> Fn(
						&Receiver,
						$($inj,)*
						$($para,)*
					) $(-> $ret)?,
			{
				Self::new(move |$(mut $inj_name,)* $($para_name,)*| {
					let guard = Injector::INJECTOR(&(), $(&mut $inj_name,)*);

					handler(&*guard, $($inj_name,)* $($para_name,)*)
				})
			}

			#[allow(unused)]
			pub fn new_method_mut<Injector, Receiver, Func>(_injector: Injector, handler: Func) -> Self
			where
				Injector: 'static + $crate::behavior::delegate_macro_internal::FuncMethodInjectorMutGetGuard<Receiver>,
				Injector: $crate::behavior::delegate_macro_internal::FuncMethodInjectorMut<
					Receiver,
					Injector = for<
						$inj_lt
						$($(
							$(,$fn_lt)*
						)?)?
					> fn(
						&$inj_lt (),
						$(&mut $inj),*
					) -> Injector::GuardHelper<$inj_lt>>,
				Receiver: ?Sized + 'static,
				Func: 'static
					+ $crate::behavior::delegate_macro_internal::Send
					+ $crate::behavior::delegate_macro_internal::Sync
					+ for<$inj_lt $($( $(,$fn_lt)* )?)?> Fn(
						&mut Receiver,
						$($inj,)*
						$($para,)*
					) $(-> $ret)?,
			{
				Self::new(move |$(mut $inj_name,)* $($para_name,)*| {
					let mut guard = Injector::INJECTOR(&(), $(&mut $inj_name,)*);

					handler(&mut *guard, $($inj_name,)* $($para_name,)*)
				})
			}
		}
	};

	// === Without injector === //
	(
		$(#[$attr_meta:meta])*
		$vis:vis fn $name:ident
			$(
				<$($generic:ident),* $(,)?>
				$(<$($fn_lt:lifetime),* $(,)?>)?
			)?
			($($para_name:ident: $para:ty),* $(,)?) $(-> $ret:ty)?
		$(as deriving $deriving:path $({ $($deriving_args:tt)* })? )*
		$(where $($where_token:tt)*)?
	) => {
		$(#[$attr_meta])*
		$vis struct $name $(<$($generic),*>)?
		$(where
			$($where_token)*
		)? {
			_ty: ($($($crate::behavior::delegate_macro_internal::PhantomData<fn() -> $generic>,)*)?),
			handler: $crate::behavior::delegate_macro_internal::Arc<
				dyn
					$($(for<$($fn_lt),*>)?)?
					Fn($($para),*) $(-> $ret)? +
						$crate::behavior::delegate_macro_internal::Send +
						$crate::behavior::delegate_macro_internal::Sync
			>,
		}

		impl$(<$($generic),*>)? $name $(<$($generic),*>)?
		$(where
			$($where_token)*
		)? {
			#[allow(unused)]
			pub fn new<Func>(handler: Func) -> Self
			where
				Func: 'static +
					$crate::behavior::delegate_macro_internal::Send +
					$crate::behavior::delegate_macro_internal::Sync +
					$($(for<$($fn_lt),*>)?)?
						Fn($($para),*) $(-> $ret)?,
			{
				Self {
					_ty: ($($($crate::behavior::delegate_macro_internal::PhantomData::<fn() -> $generic>,)*)?),
					handler: $crate::behavior::delegate_macro_internal::Arc::new(handler),
				}
			}
		}

		impl<
			Func: 'static +
				$crate::behavior::delegate_macro_internal::Send +
				$crate::behavior::delegate_macro_internal::Sync +
				$($(for<$($fn_lt),*>)?)?
					Fn($($para),*) $(-> $ret)?
			$(, $($generic),*)?
		> $crate::behavior::delegate_macro_internal::From<Func> for $name $(<$($generic),*>)?
		$(where
			$($where_token)*
		)? {
			fn from(handler: Func) -> Self {
				Self::new(handler)
			}
		}

		impl$(<$($generic),*>)? $crate::behavior::delegate_macro_internal::Deref for $name $(<$($generic),*>)?
		$(where
			$($where_token)*
		)? {
			type Target = dyn $($(for<$($fn_lt),*>)?)? Fn($($para),*) $(-> $ret)? +
				$crate::behavior::delegate_macro_internal::Send +
				$crate::behavior::delegate_macro_internal::Sync;

			fn deref(&self) -> &Self::Target {
				&*self.handler
			}
		}

		impl<$($($($fn_lt,)*)? $($generic: 'static,)*)?> $crate::behavior::delegate_macro_internal::Dispatchable<($($para,)*)> for $name $(<$($generic),*>)?
		$(where
			$($where_token)*
		)? {
			type Output = $crate::behavior::delegate!(@__internal_or_unit $($ret)?);

			fn dispatch(&self, ($($para_name,)*): ($($para,)*)) -> Self::Output {
				self($($para_name,)*)
			}
		}

		impl$(<$($generic),*>)? $crate::behavior::delegate_macro_internal::fmt::Debug for $name $(<$($generic),*>)?
		$(where
			$($where_token)*
		)? {
			fn fmt(&self, fmt: &mut $crate::behavior::delegate_macro_internal::fmt::Formatter) -> $crate::behavior::delegate_macro_internal::fmt::Result {
				fmt.write_str("delegate::")?;
				fmt.write_str($crate::behavior::delegate_macro_internal::stringify!($name))?;
				fmt.write_str("(")?;
				$(
					fmt.write_str($crate::behavior::delegate_macro_internal::stringify!($para))?;
				)*
				fmt.write_str(")")?;

				Ok(())
			}
		}

		impl$(<$($generic),*>)? $crate::behavior::delegate_macro_internal::Clone for $name $(<$($generic),*>)?
		$(where
			$($where_token)*
		)? {
			fn clone(&self) -> Self {
				Self {
					_ty: ($($($crate::behavior::delegate_macro_internal::PhantomData::<fn() -> $generic>,)*)?),
					handler: $crate::behavior::delegate_macro_internal::Clone::clone(&self.handler),
				}
			}
		}

		$crate::behavior::delegate! {
			@__internal_forward_derives

			$(#[$attr_meta])*
			$vis fn $name
				$(
					<$($generic,)*>
					$(<$($fn_lt,)*>)?
				)?
				($($para_name: $para,)*) $(-> $ret)?
			$(as deriving $deriving $({ $($deriving_args)* })? )*
			$(where $($where_token)*)?
		}
	};

	// === Helpers === //
	(
		@__internal_forward_derives

		$(#[$attr_meta:meta])*
		$vis:vis fn $name:ident
			$(
				<$($generic:ident),* $(,)?>
				$(<$($fn_lt:lifetime),* $(,)?>)?
			)?
			($($para_name:ident: $para:ty),* $(,)?) $(-> $ret:ty)?
		as deriving $first_deriving:path $({ $($first_deriving_args:tt)* })?
		$(as deriving $next_deriving:path $({ $($next_deriving_args:tt)* })? )*
		$(where $($where_token:tt)*)?
	) => {
		$first_deriving! {
			args { $($($first_deriving_args)*)? }

			$(#[$attr_meta])*
			$vis fn $name
				$(
					<$($generic,)*>
					$(<$($fn_lt,)*>)?
				)?
				($($para_name: $para,)*) $(-> $ret)?
			$(where $($where_token)*)?
		}

		$crate::behavior::delegate! {
			@__internal_forward_derives

			$(#[$attr_meta])*
			$vis fn $name
				$(
					<$($generic,)*>
					$(<$($fn_lt,)*>)?
				)?
				($($para_name: $para,)*) $(-> $ret)?
			$(as deriving $next_deriving $({ $($next_deriving_args)* })?)*
			$(where $($where_token)*)?
		}
	};
	(
		@__internal_forward_derives

		$(#[$attr_meta:meta])*
		$vis:vis fn $name:ident
			$(
				<$($generic:ident),* $(,)?>
				$(<$($fn_lt:lifetime),* $(,)?>)?
			)?
			($($para_name:ident: $para:ty),* $(,)?) $(-> $ret:ty)?
		$(where $($where_token:tt)*)?
	) => { /* base case */};

	(@__internal_or_unit $ty:ty) => { $ty };
	(@__internal_or_unit) => { () };
}

pub use delegate;

// === Standard Injectors === //

#[derive(Debug, Copy, Clone, Default)]
pub struct ComponentInjector;

impl<T: 'static> FuncMethodInjectorRef<T> for ComponentInjector {
    type Guard<'a> = CompRef<'static, T>;
    type Injector = for<'a> fn(&'a (), &mut Entity) -> Self::Guard<'a>;

    const INJECTOR: Self::Injector = |_, me| me.get();
}

impl<T: 'static> FuncMethodInjectorMut<T> for ComponentInjector {
    type Guard<'a> = CompMut<'static, T>;
    type Injector = for<'a> fn(&'a (), &mut Entity) -> Self::Guard<'a>;

    const INJECTOR: Self::Injector = |_, me| me.get_mut();
}

// === Behavior === //

// Core traits
pub trait BehaviorKind: Sized + 'static {
    type Delegate: BehaviorDelegate;
}

pub trait BehaviorDelegate: Sized {
    type List: 'static + Send + Sync;
    type View<'a>;

    fn make_list() -> Self::List;

    fn extend_list(self, list: &mut Self::List);

    fn view<'a>(bhv: &'a BehaviorRegistry, list: Option<&'a Self::List>) -> Self::View<'a>;
}

// Macro
#[doc(hidden)]
pub mod behavior_kind_macro_internals {
    pub use super::{behavior_kind, BehaviorKind};
}

#[macro_export]
macro_rules! behavior_kind {
	($(
		$(#[$meta:meta])*
		$vis:vis $name:ident of $delegate:ty $(;)?
	)*) => {$(
		$(#[$meta])*
		$vis struct $name { _marker: () }

		$crate::behavior::behavior_kind_macro_internals::behavior_kind!(derive $name => $delegate);
	)*};
	($(
		derive $name:path => $delegate:ty  $(;)?
	)*) => {$(
		impl $crate::behavior::behavior_kind_macro_internals::BehaviorKind for $name {
			type Delegate = $name;
		}
	)*};
	(
		args {}

		$(#[$attr_meta:meta])*
		$vis:vis fn $name:ident
			$(
				<$($generic:ident),* $(,)?>
				$(<$($fn_lt:lifetime),* $(,)?>)?
			)?
			(
				$($para_name:ident: $para:ty),* $(,)?
			) $(-> $ret:ty)?
		$(where $($where_token:tt)*)?
	) => {
		$crate::behavior::behavior_kind_macro_internals::behavior_kind!(derive $name => $name);
	};
}

pub use behavior_kind;

// BehaviorRegistry
pub struct BehaviorRegistry {
    behaviors: FxHashMap<NamedTypeId, Box<dyn Any + Send + Sync>>,
}

impl BehaviorRegistry {
    pub const fn new() -> Self {
        Self {
            behaviors: FxHashMap::with_hasher(ConstSafeBuildHasherDefault::new()),
        }
    }

    pub fn register<B: BehaviorKind>(&mut self, delegate: B::Delegate) -> &mut Self {
        let own_registry = self
            .behaviors
            .entry(NamedTypeId::of::<B>())
            .or_insert_with(|| {
                Box::<<B::Delegate as BehaviorDelegate>::List>::new(
                    <B::Delegate as BehaviorDelegate>::make_list(),
                )
            })
            .downcast_mut::<<B::Delegate as BehaviorDelegate>::List>()
            .unwrap();

        delegate.extend_list(own_registry);

        self
    }

    pub fn register_combined<B: BehaviorKind<Delegate = B>>(&mut self, delegate: B) -> &mut Self {
        self.register::<B>(delegate)
    }

    pub fn register_many(&mut self, registrar: impl FnOnce(&mut Self)) -> &mut Self {
        registrar(self);
        self
    }

    pub fn with<B: BehaviorKind>(mut self, delegate: B::Delegate) -> Self {
        self.register::<B>(delegate);
        self
    }

    pub fn with_many(mut self, registrar: impl FnOnce(&mut Self)) -> Self {
        self.register_many(registrar);
        self
    }

    pub fn get_raw<B: BehaviorKind>(&self) -> Option<&<B::Delegate as BehaviorDelegate>::List> {
        self.behaviors
            .get(&NamedTypeId::of::<B>())
            .map(|list| list.downcast_ref().unwrap())
    }

    pub fn get<B: BehaviorKind>(&self) -> <B::Delegate as BehaviorDelegate>::View<'_> {
        <B::Delegate as BehaviorDelegate>::view(self, self.get_raw::<B>())
    }
}

impl Default for BehaviorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for BehaviorRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BehaviorRegistry")
            .field(
                "behaviors",
                &MapFmt(self.behaviors.iter().map(|(k, _v)| (k, RawFmt("...")))),
            )
            .finish()
    }
}

// === Behavior Delegates === //

#[doc(hidden)]
pub mod behavior_derive_macro_internal {
    pub use {
        super::{BehaviorDelegate, BehaviorRegistry},
        crate::event::ProcessableEventList,
        std::{boxed::Box, compile_error, concat, option::Option, stringify, vec::Vec},
    };

    pub trait FnPointeeInference {
        type Pointee: ?Sized;
    }

    impl<T: ?Sized> FnPointeeInference for fn(&mut T) {
        type Pointee = T;
    }
}

#[macro_export]
macro_rules! behavior_delegate {
	(
		args {}

		$(#[$attr_meta:meta])*
		$vis:vis fn $name:ident
			$(
				<$($generic:ident),* $(,)?>
				$(<$($fn_lt:lifetime),* $(,)?>)?
			)?
			(
				$bhv_name:ident: $bhv_ty:ty
				$(, $para_name:ident: $para:ty)* $(,)?
			) $(-> $ret:ty)?
		$(where $($where_token:tt)*)?
	) => {
		impl<$($($($fn_lt,)*)? $($generic: 'static,)*)?> $crate::behavior::behavior_derive_macro_internal::BehaviorDelegate for $name<$($($generic),*)?>
		$(where $($where_token)*)?
		{
			type List = $crate::behavior::behavior_derive_macro_internal::Vec<Self>;
			type View<'a> = $crate::behavior::behavior_derive_macro_internal::Box<dyn $(for<$($fn_lt,)*>)? Fn($($para),*) + 'a>;

			fn make_list() -> Self::List {
				$crate::behavior::behavior_derive_macro_internal::Vec::new()
			}

			fn extend_list(self, list: &mut Self::List) {
				list.push(self);
			}

			fn view<'a>(
				bhv: &'a $crate::behavior::behavior_derive_macro_internal::BehaviorRegistry,
				list: $crate::behavior::behavior_derive_macro_internal::Option<&'a Self::List>,
			) -> Self::View<'a> {
				$crate::behavior::behavior_derive_macro_internal::Box::new(move |$($para_name,)*| {
					for behavior in list.map_or(&[][..], |vec| vec.as_slice()) {
						behavior(bhv, $($para_name,)*);
					}
				})
			}
		}
	};
}

pub use behavior_delegate;

delegate! {
    pub fn ContextlessEventHandler<EL>(bhv: &BehaviorRegistry, events: &mut EL)
    as deriving behavior_delegate
    where
        EL: ProcessableEventList,
}

delegate! {
    pub fn ContextlessQueryHandler(bhv: &BehaviorRegistry)
    as deriving behavior_delegate
}

delegate! {
    pub fn NamespacedQueryHandler(bhv: &BehaviorRegistry, namespace: VirtualTag)
    as deriving behavior_delegate
}
