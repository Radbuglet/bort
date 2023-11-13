use std::{
    any::{Any, TypeId},
    fmt, hash,
    ops::{Deref, DerefMut},
    sync::OnceLock,
};

use derive_where::derive_where;

use crate::{
    entity::{CompMut, CompRef, Entity},
    util::{
        hash_map::{ConstSafeBuildHasherDefault, FxHashMap, FxHashSet},
        misc::{IsUnit, MapFmt, NamedTypeId, Truthy},
    },
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

// === Delegate Traits === //

pub trait Delegate: fmt::Debug + Clone + Send + Sync {}

// === Delegate === //

#[doc(hidden)]
pub mod delegate_macro_internal {
    use std::{mem::MaybeUninit, ops::DerefMut};

    // === Re-exports === //

    pub use {
        super::{Delegate, FuncMethodInjectorMut, FuncMethodInjectorRef},
        std::{
            clone::Clone,
            convert::From,
            fmt,
            marker::{PhantomData, Send, Sync},
            ops::{Deref, Fn},
            panic::Location,
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

    // N.B. this function is not marked as unsafe because `#[forbid(dead_code)]` may be used in
    // userland crates.
    #[allow(unsafe_code)] // TODO: Move to `core`
    pub fn uber_dangerous_transmute_this_is_unsound<A, B>(a: A) -> B {
        unsafe {
            let mut a = MaybeUninit::<A>::new(a);
            a.as_mut_ptr().cast::<B>().read()
        }
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
			#[cfg_attr(debug_assertions, track_caller)]
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
			#[cfg_attr(debug_assertions, track_caller)]
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
			#[cfg(debug_assertions)]
			defined: &'static $crate::behavior::delegate_macro_internal::Location<'static>,
            handler: $crate::behavior::delegate_macro_internal::Arc<
                dyn
                    $($(for<$($fn_lt),*>)?)?
                    $crate::behavior::delegate_macro_internal::Fn($crate::behavior::delegate_macro_internal::PhantomData<Self> $(,$para)*) $(-> $ret)? +
                        $crate::behavior::delegate_macro_internal::Send +
                        $crate::behavior::delegate_macro_internal::Sync
            >,
        }

		#[allow(unused)]
        impl$(<$($generic),*>)? $name $(<$($generic),*>)?
        $(where
            $($where_token)*
        )? {
			#[cfg_attr(debug_assertions, track_caller)]
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
					#[cfg(debug_assertions)]
					defined: $crate::behavior::delegate_macro_internal::Location::caller(),
                    handler: $crate::behavior::delegate_macro_internal::Arc::new(move |_marker $(,$para_name)*| handler($($para_name),*)),
                }
            }

			#[allow(non_camel_case_types)]
			pub fn call<$($($($fn_lt,)*)?)? $($para_name,)* __Out>(&self $(,$para_name: $para_name)*) -> __Out
			where
				$($(for<$($fn_lt,)*>)?)? fn($($para,)*) $(-> $ret)?: $crate::behavior::delegate_macro_internal::Fn($($para_name,)*) -> __Out,
			{
				$crate::behavior::delegate_macro_internal::uber_dangerous_transmute_this_is_unsound(
					(self.handler)(
						$crate::behavior::delegate_macro_internal::PhantomData,
						$($crate::behavior::delegate_macro_internal::uber_dangerous_transmute_this_is_unsound($para_name),)*
					)
				)
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
			#[cfg_attr(debug_assertions, track_caller)]
            fn from(handler: Func) -> Self {
                Self::new(handler)
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

				#[cfg(debug_assertions)]
				{
					fmt.write_str(" @ ")?;
					fmt.write_str(self.defined.file())?;
					fmt.write_str(":")?;
					$crate::behavior::delegate_macro_internal::fmt::Debug::fmt(&self.defined.line(), fmt)?;
					fmt.write_str(":")?;
					$crate::behavior::delegate_macro_internal::fmt::Debug::fmt(&self.defined.column(), fmt)?;
				}

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
					#[cfg(debug_assertions)]
					defined: self.defined,
                    handler: $crate::behavior::delegate_macro_internal::Clone::clone(&self.handler),
                }
            }
        }

		impl$(<$($generic),*>)? $crate::behavior::delegate_macro_internal::Delegate for $name $(<$($generic),*>)?
        $(where
            $($where_token)*
        )?
		{
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
    type Guard<'a> = CompRef<'static, T, T>;
    type Injector = for<'a> fn(&'a (), &mut Entity) -> Self::Guard<'a>;

    const INJECTOR: Self::Injector = |_, me| me.get();
}

impl<T: 'static> FuncMethodInjectorMut<T> for ComponentInjector {
    type Guard<'a> = CompMut<'static, T, T>;
    type Injector = for<'a> fn(&'a (), &mut Entity) -> Self::Guard<'a>;

    const INJECTOR: Self::Injector = |_, me| me.get_mut();
}

// === BehaviorRegistry === //

pub struct BehaviorRegistry {
    behaviors: FxHashMap<NamedTypeId, Box<dyn DynBehaviorList>>,
}

impl BehaviorRegistry {
    pub const fn new() -> Self {
        Self {
            behaviors: FxHashMap::with_hasher(ConstSafeBuildHasherDefault::new()),
        }
    }

    pub fn from_fn(registrar: impl FnOnce(&mut Self)) -> Self {
        let mut bhv = Self::new();
        registrar(&mut bhv);
        bhv
    }

    pub fn register_cx<B: Behavior, M>(&mut self, meta: M, delegate: B) -> &mut Self
    where
        B::List: ExtendableBehaviorList<M>,
    {
        let own_registry = self
            .behaviors
            .entry(NamedTypeId::of::<B>())
            .or_insert_with(|| Box::<B::List>::default())
            .as_any_mut()
            .downcast_mut::<B::List>()
            .unwrap();

        own_registry.push_cx(delegate, meta);

        self
    }

    pub fn register<B: Behavior>(&mut self, delegate: B) -> &mut Self
    where
        B::List: ExtendableBehaviorList,
    {
        self.register_cx((), delegate)
    }

    pub fn register_many(&mut self, registrar: impl FnOnce(&mut Self)) -> &mut Self {
        registrar(self);
        self
    }

    pub fn register_from(&mut self, registry: &BehaviorRegistry) {
        for (key, list) in self.behaviors.iter_mut() {
            if let Some(other) = registry.behaviors.get(key) {
                list.extend_dyn(&**other);
            }
        }
    }

    pub fn with_cx<B: Behavior, M>(mut self, meta: M, delegate: B) -> Self
    where
        B::List: ExtendableBehaviorList<M>,
    {
        self.register_cx(meta, delegate);
        self
    }

    pub fn with<B: Behavior>(self, delegate: B) -> Self
    where
        B::List: ExtendableBehaviorList<()>,
    {
        self.with_cx((), delegate)
    }

    pub fn with_many(mut self, registrar: impl FnOnce(&mut Self)) -> Self {
        self.register_many(registrar);
        self
    }

    pub fn get_list_untyped(&self, behavior_id: TypeId) -> Option<&(dyn DynBehaviorList)> {
        self.behaviors.get(&behavior_id).map(|v| &**v)
    }

    pub fn get_list<B: Behavior>(&self) -> Option<&B::List> {
        self.get_list_untyped(TypeId::of::<B>())
            .map(|v| v.as_any().downcast_ref().unwrap())
    }

    pub fn get_list_mut_untyped(
        &mut self,
        behavior_id: TypeId,
    ) -> Option<&mut (dyn DynBehaviorList)> {
        self.behaviors.get_mut(&behavior_id).map(|v| &mut **v)
    }

    pub fn get_list_mut<B: Behavior>(&mut self) -> Option<&mut B::List> {
        self.get_list_mut_untyped(TypeId::of::<B>())
            .map(|v| v.as_any_mut().downcast_mut().unwrap())
    }

    pub fn get<B: Behavior>(&self) -> <B::List as BehaviorList>::View<'_> {
        <B::List as BehaviorList>::opt_view(self.get_list::<B>())
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
                &MapFmt(self.behaviors.iter().map(|(k, v)| (k, v))),
            )
            .finish()
    }
}

impl Clone for BehaviorRegistry {
    fn clone(&self) -> Self {
        Self {
            behaviors: self
                .behaviors
                .iter()
                .map(|(k, v)| (*k, v.clone_box()))
                .collect(),
        }
    }
}

// === Behavior === //

pub type BehaviorListFor<T> = <T as Behavior>::List;

pub trait Behavior: Sized + 'static {
    type List: BehaviorList<Delegate = Self>;
}

pub trait DynBehaviorList: 'static + Send + Sync + fmt::Debug {
    fn as_any(&self) -> &(dyn Any + Send + Sync);

    fn as_any_mut(&mut self) -> &mut (dyn Any + Send + Sync);

    fn clone_box(&self) -> Box<dyn DynBehaviorList>;

    fn extend_dyn(&mut self, other: &dyn DynBehaviorList);
}

impl<T: BehaviorList> DynBehaviorList for T {
    fn as_any(&self) -> &(dyn Any + Send + Sync) {
        self
    }

    fn as_any_mut(&mut self) -> &mut (dyn Any + Send + Sync) {
        self
    }

    fn clone_box(&self) -> Box<dyn DynBehaviorList> {
        Box::new(self.clone())
    }

    fn extend_dyn(&mut self, other: &dyn DynBehaviorList) {
        self.extend_ref(other.as_any().downcast_ref().unwrap())
    }
}

pub trait BehaviorList: BehaviorSafe + Default + fmt::Debug {
    type View<'a>;
    type Delegate: 'static;

    fn extend(&mut self, other: Self);

    fn extend_ref(&mut self, other: &Self);

    fn opt_view(me: Option<&Self>) -> Self::View<'_>;

    fn view(&self) -> Self::View<'_> {
        Self::opt_view(Some(self))
    }
}

pub trait ExtendableBehaviorList<M = ()>: BehaviorList {
    fn push_cx(&mut self, delegate: Self::Delegate, meta: M);

    fn push(&mut self, delegate: Self::Delegate)
    where
        IsUnit<M>: Truthy<Unit = M>,
    {
        self.push_cx(delegate, IsUnit::<M>::make_unit())
    }
}

pub trait BehaviorSafe: 'static + Sized + Send + Sync + Clone + fmt::Debug {}

impl<T: 'static + Send + Sync + Clone + fmt::Debug> BehaviorSafe for T {}

// === Multiplexable === //

pub trait Multiplexable: Delegate {
    type Multiplexer<'a, D>
    where
        Self: 'a,
        D: MultiplexDriver<Item = Self> + 'a;

    fn make_multiplexer<'a, D>(driver: D) -> Self::Multiplexer<'a, D>
    where
        D: MultiplexDriver<Item = Self> + 'a;
}

pub trait MultiplexDriver: Sized {
    type Item: ?Sized;

    fn drive<'a>(&'a self, target: impl FnMut(&'a Self::Item));
}

impl<I: MultiplexDriver> MultiplexDriver for &'_ I {
    type Item = I::Item;

    fn drive<'a>(&'a self, target: impl FnMut(&'a Self::Item)) {
        (*self).drive(target);
    }
}

impl<I: MultiplexDriver> MultiplexDriver for Option<I> {
    type Item = I::Item;

    fn drive<'a>(&'a self, target: impl FnMut(&'a Self::Item)) {
        if let Some(inner) = self {
            inner.drive(target);
        }
    }
}

#[doc(hidden)]
pub mod multiplexed_macro_internals {
    pub use {
        super::{
            behavior, delegate, Behavior, BehaviorRegistry, MultiplexDriver, Multiplexable,
            SimpleBehaviorList,
        },
        std::{boxed::Box, clone::Clone, iter::IntoIterator, ops::Fn},
    };
}

#[macro_export]
macro_rules! behavior {
	(
        $(#[$attr_meta:meta])*
        $vis:vis fn $name:ident
            $(
                <$($generic:ident),* $(,)?>
                $(<$($fn_lt:lifetime),* $(,)?>)?
            )?
            ($($para_name:ident: $para:ty),* $(,)?) $(-> $ret:ty)?
		$(as list $list:ty)?
        $(as deriving $deriving:path $({ $($deriving_args:tt)* })? )*
        $(where $($where_token:tt)*)?
    ) => {
        $crate::behavior::multiplexed_macro_internals::delegate!(
            $(#[$attr_meta])*
            $vis fn $name
                $(
                    <$($generic),*>
                    $(<$($fn_lt),*>)?
                )?
                (
                    bhv: &'_ $crate::behavior::multiplexed_macro_internals::BehaviorRegistry,
                    $($para_name: $para),*
                ) $(-> $ret)?
            as deriving $crate::behavior::multiplexed_macro_internals::behavior { $($list)? }
            $(as deriving $deriving $({ $($deriving_args)* })? )*
            $(where $($where_token)*)?
        );
    };
    (
        args { just_multiplex }

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
		impl<$($generic),*> $crate::behavior::multiplexed_macro_internals::Multiplexable for $name<$($generic),*>
		$(where $($where_token)*)?
		{
			type Multiplexer<'a, D> = $crate::behavior::multiplexed_macro_internals::Box<
				dyn $(for<$($fn_lt),*>)? $crate::behavior::multiplexed_macro_internals::Fn($($para),*) + 'a
			>
			where
				Self: 'a,
				D: 'a + $crate::behavior::multiplexed_macro_internals::MultiplexDriver<Item = Self>;

			fn make_multiplexer<'a, D>(driver: D) -> Self::Multiplexer<'a, D>
			where
				D: 'a + $crate::behavior::multiplexed_macro_internals::MultiplexDriver<Item = Self>,
			{
				$crate::behavior::multiplexed_macro_internals::Box::new(move |$($para_name),*| {
					driver.drive(|item| item.call($($para_name),*));
				})
			}
		}
	};
	(
        args { just_derive $ty:ty }

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
		impl<$($generic),*> $crate::behavior::multiplexed_macro_internals::Behavior for $name<$($generic),*>
		$(where $($where_token)*)?
		{
			type List = $ty;
		}
	};
	(
        args { $ty:ty }

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
		$crate::behavior::multiplexed_macro_internals::behavior! {
			args { just_multiplex }

			$(#[$attr_meta])*
			$vis fn $name
				$(
					<$($generic),*>
					$(<$($fn_lt),*>)?
				)?
				(
					$($para_name: $para),*
				) $(-> $ret)?
			$(where $($where_token)*)?
		}

		$crate::behavior::multiplexed_macro_internals::behavior! {
			args { just_derive $ty }

			$(#[$attr_meta])*
			$vis fn $name
				$(
					<$($generic),*>
					$(<$($fn_lt),*>)?
				)?
				(
					$($para_name: $para),*
				) $(-> $ret)?
			$(where $($where_token)*)?
		}
	};
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
		$crate::behavior::multiplexed_macro_internals::behavior! {
			args { $crate::behavior::multiplexed_macro_internals::SimpleBehaviorList<Self> }

			$(#[$attr_meta])*
			$vis fn $name
				$(
					<$($generic),*>
					$(<$($fn_lt),*>)?
				)?
				(
					$($para_name: $para),*
				) $(-> $ret)?
			$(where $($where_token)*)?
		}
	};
}

pub use behavior;

// === SimpleBehaviorList === //

#[derive(Debug, Clone)]
#[derive_where(Default)]
pub struct SimpleBehaviorList<B> {
    pub behaviors: Vec<B>,
}

impl<B: BehaviorSafe + Multiplexable> BehaviorList for SimpleBehaviorList<B> {
    type View<'a> = B::Multiplexer<'a, Option<&'a SimpleBehaviorList<B>>>;
    type Delegate = B;

    fn extend(&mut self, mut other: Self) {
        self.behaviors.append(&mut other.behaviors);
    }

    fn extend_ref(&mut self, other: &Self) {
        self.behaviors.extend(other.behaviors.iter().cloned());
    }

    fn opt_view(me: Option<&Self>) -> Self::View<'_> {
        B::make_multiplexer(me)
    }
}

impl<B: BehaviorSafe + Multiplexable> ExtendableBehaviorList for SimpleBehaviorList<B> {
    fn push_cx(&mut self, delegate: Self::Delegate, _meta: ()) {
        self.behaviors.push(delegate);
    }
}

impl<B: Multiplexable> MultiplexDriver for SimpleBehaviorList<B> {
    type Item = B;

    fn drive<'a>(&'a self, mut target: impl FnMut(&'a Self::Item)) {
        for bhv in &self.behaviors {
            target(bhv);
        }
    }
}

// === OrderedBehaviorList === //

#[derive(Debug, Clone)]
#[derive_where(Default)]
pub struct OrderedBehaviorList<B, D> {
    behaviors: Vec<OrderedBehavior<B, D>>,
    dependents_on: FxHashMap<D, OrderedDependents>,
    behaviors_topos: OnceLock<Vec<B>>,
}

#[derive(Debug, Clone)]
struct OrderedBehavior<B, D> {
    /// The behavior delegate
    behavior: B,

    /// The number of dependencies the behavior has before it can run.
    dep_count: u32,

    /// The dependencies it resolves in running.
    resolves: FxHashSet<D>,
}

#[derive(Debug, Clone, Default)]
struct OrderedDependents {
    /// The behaviors depending upon this key.
    dependents: Vec<usize>,

    /// The number of behaviors which need to be resolved before this entire dependency is satisfied.
    resolvers: u32,
}

impl<B, D> BehaviorList for OrderedBehaviorList<B, D>
where
    B: BehaviorSafe + Multiplexable,
    D: BehaviorSafe + hash::Hash + Eq,
{
    type View<'a> = B::Multiplexer<'a, Option<&'a Self>>;
    type Delegate = B;

    fn extend(&mut self, mut other: Self) {
        self.behaviors.append(&mut other.behaviors);
    }

    fn extend_ref(&mut self, other: &Self) {
        self.behaviors.extend(other.behaviors.iter().cloned());
    }

    fn opt_view(me: Option<&Self>) -> Self::View<'_> {
        B::make_multiplexer(me)
    }
}

impl<B, D, I1, I2> ExtendableBehaviorList<(I1, I2)> for OrderedBehaviorList<B, D>
where
    B: BehaviorSafe + Multiplexable,
    D: BehaviorSafe + hash::Hash + Eq,
    I1: IntoIterator<Item = D>,
    I2: IntoIterator<Item = D>,
{
    fn push_cx(&mut self, delegate: Self::Delegate, (depends, resolves): (I1, I2)) {
        // Register the behavior
        let bhv_idx = self.behaviors.len();
        self.behaviors.push(OrderedBehavior {
            behavior: delegate,
            dep_count: 0, // This will be adjusted later.
            resolves: resolves.into_iter().collect(),
        });
        let bhv = &mut self.behaviors[bhv_idx];

        // For each resolved dependency, increment its resolver count.
        for resolved in &bhv.resolves {
            if let Some(info) = self.dependents_on.get_mut(resolved) {
                info.resolvers += 1;
            } else {
                self.dependents_on.insert(
                    resolved.clone(),
                    OrderedDependents {
                        dependents: Vec::new(),
                        resolvers: 1,
                    },
                );
            }
        }

        // For each dependency...
        for dep in depends {
            // Increment the behavior's dependency count
            bhv.dep_count += 1;

            // Register a relationship between the dependency and this.
            if let Some(info) = self.dependents_on.get_mut(&dep) {
                info.dependents.push(bhv_idx);
            } else {
                self.dependents_on.insert(
                    dep.clone(),
                    OrderedDependents {
                        dependents: vec![bhv_idx],
                        resolvers: 0,
                    },
                );
            }
        }

        // Invalidate the existing topological sort if applicable
        let _ = OnceLock::take(&mut self.behaviors_topos);
    }
}

impl<B: BehaviorSafe + Multiplexable, D> MultiplexDriver for OrderedBehaviorList<B, D>
where
    B: BehaviorSafe + Multiplexable,
    D: BehaviorSafe + hash::Hash + Eq,
{
    type Item = B;

    fn drive<'a>(&'a self, mut target: impl FnMut(&'a Self::Item)) {
        let topos = self.behaviors_topos.get_or_init(|| {
            let mut toposorted = Vec::new();

            // Create mutable state copies
            let mut behavior_counts = self
                .behaviors
                .iter()
                .map(|b| b.dep_count)
                .collect::<Vec<_>>();

            let mut dependents_on = self
                .dependents_on
                .iter()
                .map(|(k, v)| (k, v.resolvers))
                .collect::<FxHashMap<_, _>>();

            // Decrement the blockers for behaviors whose dependencies have no blocking resolvers
            for (_key, dependents) in &self.dependents_on {
                if dependents.resolvers == 0 {
                    for &blocked_id in &dependents.dependents {
						behavior_counts[blocked_id] -= 1;
					}
                }
            }

			// Collect the initial set of behaviors which are ready to run
			let mut ready_to_run = behavior_counts
				.iter()
				.enumerate()
				.filter_map(|(i, &blockers)| (blockers == 0).then_some(i))
				.collect::<Vec<_>>();

            // Propagate execution to build the toposorted array
            while let Some(ran) = ready_to_run.pop() {
                // Add it to the toposorted array
                toposorted.push(self.behaviors[ran].behavior.clone());

                // Propagate resolution
                ready_to_run.extend(
                    self.behaviors[ran]
                        .resolves
                        .iter()
                        // For every dependency resolved by running this behavior, decrement its blocker count.
                        .filter(|resolved| {
                            let resolved_count = dependents_on.get_mut(resolved).unwrap();
                            *resolved_count -= 1;

                            *resolved_count == 0
                        })
                        // For dependencies with zero blockers, find their dependents.
                        .flat_map(|resolved| &self.dependents_on[resolved].dependents)
                        // ...and attempt to resolve them.
                        .filter(|&&bhv_id| {
                            behavior_counts[bhv_id] -= 1;

                            // ...producing an iterator of behaviors which can run.
                            behavior_counts[bhv_id] == 0
                        }),
                );
            }

            // Now, ensure that all behaviors have run.
            let blocked = behavior_counts
                .iter()
                .enumerate()
                .filter_map(|(i, &blockers)| (blockers > 0).then_some(&self.behaviors[i].behavior))
                .collect::<Vec<_>>();

            if !blocked.is_empty() {
				// TODO: Improve error message.
                panic!("the following behaviors could never run due to cyclic dependencies: {blocked:#?}");
            }

            toposorted
        });

        for bhv in topos {
            target(bhv);
        }
    }
}

// === InitializerBehaviorList === //

// PartialEntity
#[derive(Debug, Copy, Clone)]
pub struct PartialEntity<'a> {
    target: Entity,
    can_access: &'a FxHashSet<TypeId>,
}

impl PartialEntity<'_> {
    pub fn add<T: 'static>(self, component: T) {
        assert!(!self.target.has::<T>());
        self.target.insert(component);
    }

    pub fn get<T: 'static>(self) -> CompRef<'static, T, T> {
        assert!(self.can_access.contains(&TypeId::of::<T>()));
        self.target.get()
    }

    pub fn get_mut<T: 'static>(self) -> CompMut<'static, T, T> {
        assert!(self.can_access.contains(&TypeId::of::<T>()));
        self.target.get_mut()
    }

    pub fn entity(self) -> Entity {
        self.target
    }
}

// InitializerBehaviorList
#[derive(Debug, Clone)]
#[derive_where(Default)]
pub struct InitializerBehaviorList<B> {
    handlers: Vec<InitHandler<B>>,
    handlers_with_deps: FxHashMap<TypeId, Vec<usize>>,
    handlers_without_any_deps: Vec<usize>,
}

#[derive(Debug, Clone)]
struct InitHandler<B> {
    delegate: B,
    deps: FxHashSet<TypeId>,
}

impl<B> InitializerBehaviorList<B> {
    pub fn execute(&self, mut executor: impl FnMut(&B, PartialEntity<'_>), target: Entity) {
        // Execute handlers without dependencies
        for &handler in &self.handlers_without_any_deps {
            executor(
                &self.handlers[handler].delegate,
                PartialEntity {
                    target,
                    can_access: &self.handlers[handler].deps,
                },
            )
        }

        // Execute handlers with dependencies
        let mut remaining_dep_types = self.handlers_with_deps.keys().copied().collect::<Vec<_>>();
        let mut dep_counts = self
            .handlers
            .iter()
            .map(|handler| handler.deps.len())
            .collect::<Vec<_>>();

        while !remaining_dep_types.is_empty() {
            let old_len = remaining_dep_types.len();

            remaining_dep_types.retain(|&dep| {
                if !target.has_dyn(dep) {
                    return true;
                }

                for &handler in &self.handlers_with_deps[&dep] {
                    dep_counts[handler] -= 1;

                    if dep_counts[handler] == 0 {
                        executor(
                            &self.handlers[handler].delegate,
                            PartialEntity {
                                target,
                                can_access: &self.handlers[handler].deps,
                            },
                        );
                    }
                }

                false
            });

            assert_ne!(
                remaining_dep_types.len(),
                old_len,
                "InitializerBehaviorList is unable to load the following required component types: {:?}",
                remaining_dep_types
            );
        }
    }
}

impl<B: BehaviorSafe> BehaviorList for InitializerBehaviorList<B> {
    type View<'a> = InitializerBehaviorListView<'a, B>;
    type Delegate = B;

    fn extend(&mut self, other: Self) {
        self.extend_ref(&other);
    }

    fn extend_ref(&mut self, other: &Self) {
        for (key, deps) in &other.handlers_with_deps {
            self.handlers_with_deps
                .entry(*key)
                .or_default()
                .extend(deps.iter().map(|v| v + self.handlers.len()));
        }

        self.handlers_without_any_deps.extend(
            other
                .handlers_without_any_deps
                .iter()
                .map(|v| v + self.handlers.len()),
        );

        self.handlers.extend(other.handlers.iter().cloned());
    }

    fn opt_view(me: Option<&Self>) -> Self::View<'_> {
        InitializerBehaviorListView(me)
    }
}

impl<B, I> ExtendableBehaviorList<I> for InitializerBehaviorList<B>
where
    B: BehaviorSafe,
    I: IntoIterator<Item = TypeId>,
{
    fn push_cx(&mut self, delegate: Self::Delegate, deps: I) {
        let deps = deps.into_iter().collect::<FxHashSet<_>>();
        if deps.is_empty() {
            self.handlers_without_any_deps.push(self.handlers.len());
        } else {
            for &dep in &deps {
                self.handlers_with_deps
                    .entry(dep)
                    .or_default()
                    .push(self.handlers.len());
            }
        }
        self.handlers.push(InitHandler { delegate, deps });
    }
}

#[derive(Debug)]
#[derive_where(Clone, Copy)]
pub struct InitializerBehaviorListView<'a, B>(Option<&'a InitializerBehaviorList<B>>);

impl<B> InitializerBehaviorListView<'_, B> {
    pub fn execute(&self, executor: impl FnMut(&B, PartialEntity<'_>), target: Entity) {
        if let Some(inner) = self.0 {
            inner.execute(executor, target);
        }
    }
}
