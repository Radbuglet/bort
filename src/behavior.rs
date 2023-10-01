use std::{
    any::{Any, TypeId},
    fmt,
    ops::{Deref, DerefMut},
};

use derive_where::derive_where;

use crate::{
    entity::{CompMut, CompRef, Entity},
    util::{
        hash_map::{ConstSafeBuildHasherDefault, FxHashMap, FxHashSet},
        misc::{MapFmt, NamedTypeId},
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

pub trait Delegate: fmt::Debug + Clone + Send + Sync + Deref<Target = Self::DynFn> {
    type DynFn: ?Sized + Send + Sync;
}

pub trait Dispatchable<A>: Delegate {
    type Output;

    fn dispatch(&self, args: A) -> Self::Output;
}

// === Delegate === //

#[doc(hidden)]
pub mod delegate_macro_internal {
    use std::ops::DerefMut;

    // === Re-exports === //

    pub use {
        super::{Delegate, Dispatchable, FuncMethodInjectorMut, FuncMethodInjectorRef},
        std::{
            clone::Clone,
            convert::From,
            fmt,
            marker::{PhantomData, Send, Sync},
            ops::Deref,
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
			#[cfg_attr(debug_assertions, track_caller)]
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
			type DynFn = dyn $($(for<$($fn_lt),*>)?)? Fn($($para),*) $(-> $ret)? +
				$crate::behavior::delegate_macro_internal::Send +
				$crate::behavior::delegate_macro_internal::Sync;
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

        own_registry.push(delegate, meta);

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
        <B::List as BehaviorList>::view(self.get_list::<B>())
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

    fn view<'a>(me: Option<&'a Self>) -> Self::View<'a>;
}

pub trait ExtendableBehaviorList<M = ()>: BehaviorList {
    fn push(&mut self, delegate: Self::Delegate, meta: M);
}

pub trait BehaviorSafe: 'static + Sized + Send + Sync + Clone + fmt::Debug {}

impl<T: 'static + Send + Sync + Clone + fmt::Debug> BehaviorSafe for T {}

// === Multiplexable === //

pub trait Multiplexable: Delegate {
    type Multiplexer<'a, D>
    where
        Self: 'a,
        D: MultiplexDriver<Item = Self::DynFn> + 'a;

    fn make_multiplexer<'a, D>(driver: D) -> Self::Multiplexer<'a, D>
    where
        D: MultiplexDriver<Item = Self::DynFn> + 'a;
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
				D: 'a + $crate::behavior::multiplexed_macro_internals::MultiplexDriver<Item = Self::DynFn>;

			fn make_multiplexer<'a, D>(driver: D) -> Self::Multiplexer<'a, D>
			where
				D: 'a + $crate::behavior::multiplexed_macro_internals::MultiplexDriver<Item = Self::DynFn>,
			{
				$crate::behavior::multiplexed_macro_internals::Box::new(move |$($para_name),*| {
					driver.drive(|item| item($($para_name),*));
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

    fn view<'a>(me: Option<&'a Self>) -> Self::View<'a> {
        B::make_multiplexer(me)
    }
}

impl<B: BehaviorSafe + Multiplexable> ExtendableBehaviorList for SimpleBehaviorList<B> {
    fn push(&mut self, delegate: Self::Delegate, _meta: ()) {
        self.behaviors.push(delegate);
    }
}

impl<B: Multiplexable> MultiplexDriver for SimpleBehaviorList<B> {
    type Item = B::DynFn;

    fn drive<'a>(&'a self, mut target: impl FnMut(&'a Self::Item)) {
        for bhv in &self.behaviors {
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

    pub fn get<T: 'static>(self) -> CompRef<'static, T> {
        assert!(self.can_access.contains(&TypeId::of::<T>()));
        self.target.get()
    }

    pub fn get_mut<'a, T: 'static>(self) -> CompMut<'static, T> {
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
pub struct InitializerBehaviorList<I> {
    handlers: Vec<Handler<I>>,
    handlers_with_deps: FxHashMap<TypeId, Vec<usize>>,
    handlers_without_any_deps: Vec<usize>,
}

#[derive(Debug, Clone)]
struct Handler<I> {
    delegate: I,
    deps: FxHashSet<TypeId>,
}

impl<I> InitializerBehaviorList<I> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, deps: impl IntoIterator<Item = TypeId>, delegate: I) -> Self {
        self.register(deps, delegate);
        self
    }

    pub fn with_many(mut self, f: impl FnOnce(&mut InitializerBehaviorList<I>)) -> Self {
        self.register_many(f);
        self
    }

    pub fn register(&mut self, deps: impl IntoIterator<Item = TypeId>, delegate: I) -> &mut Self {
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
        self.handlers.push(Handler { delegate, deps });

        self
    }

    pub fn register_many(&mut self, f: impl FnOnce(&mut InitializerBehaviorList<I>)) -> &mut Self {
        f(self);
        self
    }

    pub fn execute(&self, mut executor: impl FnMut(&I, PartialEntity<'_>), target: Entity) {
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
                if !target.has_dyn(dep.into()) {
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

impl<I: BehaviorSafe> BehaviorList for InitializerBehaviorList<I> {
    type View<'a> = InitializerBehaviorListView<'a, I>;
    type Delegate = I;

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

    fn view<'a>(me: Option<&'a Self>) -> Self::View<'a> {
        InitializerBehaviorListView(me)
    }
}

impl<I: BehaviorSafe, D: IntoIterator<Item = TypeId>> ExtendableBehaviorList<D>
    for InitializerBehaviorList<I>
{
    fn push(&mut self, delegate: Self::Delegate, deps: D) {
        self.register(deps, delegate);
    }
}

#[derive(Debug)]
#[derive_where(Clone, Copy)]
pub struct InitializerBehaviorListView<'a, I>(Option<&'a InitializerBehaviorList<I>>);

impl<I> InitializerBehaviorListView<'_, I> {
    pub fn execute(&self, executor: impl FnMut(&I, PartialEntity<'_>), target: Entity) {
        if let Some(inner) = self.0 {
            inner.execute(executor, target);
        }
    }
}
