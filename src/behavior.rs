use std::{
    any::{Any, TypeId},
    fmt,
    ops::{Deref, DerefMut},
};

use derive_where::derive_where;

use crate::{
    entity::{CompMut, CompRef, Entity},
    util::{
        hash_map::{ConstSafeBuildHasherDefault, FxHashMap},
        misc::{MapFmt, NamedTypeId, RawFmt},
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

    pub fn provider(&self) -> BehaviorProvider {
        BehaviorProvider::wrap(self)
    }

    pub fn get_list<B: Behavior>(&self) -> Option<&B::List> {
        self.provider().get_list::<B>()
    }

    pub fn get<B: Behavior>(&self) -> <B::List as BehaviorList>::View<'_> {
        self.provider().get::<B>()
    }
}

impl RawBehaviorProvider for BehaviorRegistry {
    fn get_list_untyped(&self, behavior_id: TypeId) -> Option<&(dyn DynBehaviorList)> {
        self.behaviors.get(&behavior_id).map(|v| &**v)
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

// === BehaviorProvider === //

#[derive(Debug, Copy, Clone)]
pub struct BehaviorProvider<'a> {
    raw: &'a (dyn RawBehaviorProvider + 'a),
}

pub trait RawBehaviorProvider: fmt::Debug + Send + Sync {
    fn get_list_untyped(&self, behavior_id: TypeId) -> Option<&(dyn DynBehaviorList)>;
}

impl<'a> BehaviorProvider<'a> {
    pub fn wrap(raw: &'a (dyn RawBehaviorProvider + 'a)) -> Self {
        Self { raw }
    }

    pub fn raw(self) -> &'a (dyn RawBehaviorProvider + 'a) {
        self.raw
    }

    pub fn get_list<B: Behavior>(self) -> Option<&'a B::List> {
        self.raw
            .get_list_untyped(TypeId::of::<B>())
            .map(|list| list.as_any().downcast_ref().unwrap())
    }

    pub fn get<B: Behavior>(self) -> <B::List as BehaviorList>::View<'a> {
        BehaviorList::view(self.get_list::<B>())
    }
}

// === Behavior === //

pub trait Behavior: Sized + 'static {
    type List: BehaviorList<Delegate = Self>;
}

pub trait DynBehaviorList: 'static + Send + Sync {
    fn as_any(&self) -> &(dyn Any + Send + Sync);

    fn as_any_mut(&mut self) -> &mut (dyn Any + Send + Sync);

    fn clone(&self) -> Box<dyn DynBehaviorList>;

    fn extend_dyn(&mut self, other: &dyn DynBehaviorList);
}

impl<T: BehaviorList> DynBehaviorList for T {
    fn as_any(&self) -> &(dyn Any + Send + Sync) {
        self
    }

    fn as_any_mut(&mut self) -> &mut (dyn Any + Send + Sync) {
        self
    }

    fn clone(&self) -> Box<dyn DynBehaviorList> {
        Box::new(self.clone())
    }

    fn extend_dyn(&mut self, other: &dyn DynBehaviorList) {
        self.extend_ref(other.as_any().downcast_ref().unwrap())
    }
}

pub trait BehaviorList: BehaviorSafe + Default {
    type View<'a>;
    type Delegate: 'static;

    fn extend(&mut self, other: Self);

    fn extend_ref(&mut self, other: &Self);

    fn view<'a>(me: Option<&'a Self>) -> Self::View<'a>;
}

pub trait ExtendableBehaviorList<M = ()>: BehaviorList {
    fn push(&mut self, delegate: Self::Delegate, meta: M);
}

pub trait BehaviorSafe: 'static + Send + Sync + Clone + Sized {}

impl<T: 'static + Send + Sync + Clone> BehaviorSafe for T {}

// === MultiplexedHandler === //

pub trait MultiplexedHandler {
    type Multiplexer<'a, L: 'a>
    where
        Self: 'a;

    fn make_multiplexer<'a, L: 'a>(list: L) -> Self::Multiplexer<'a, L>
    where
        L: Clone + IntoIterator<Item = &'a Self>;
}

#[doc(hidden)]
pub mod multiplexed_macro_internals {
    pub use {
        super::{behavior, Behavior, MultiplexedHandler, SimpleBehaviorList},
        std::{boxed::Box, clone::Clone, iter::IntoIterator, ops::Fn},
    };
}

#[macro_export]
macro_rules! behavior {
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
		impl<$($generic),*> $crate::behavior::multiplexed_macro_internals::MultiplexedHandler for $name<$($generic),*>
		$(where $($where_token)*)?
		{
			type Multiplexer<'a, L: 'a> = $crate::behavior::multiplexed_macro_internals::Box<
				dyn $(for<$($fn_lt),*>)? $crate::behavior::multiplexed_macro_internals::Fn($($para),*) + 'a
			>
			where
				Self: 'a;

			fn make_multiplexer<'a, L: 'a>(list: L) -> Self::Multiplexer<'a, L>
			where
				L:
					$crate::behavior::multiplexed_macro_internals::Clone +
					$crate::behavior::multiplexed_macro_internals::IntoIterator<Item = &'a Self>
			{
				$crate::behavior::multiplexed_macro_internals::Box::new(move |$($para_name),*| {
					for item in $crate::behavior::multiplexed_macro_internals::Clone::clone(&list) {
						item($($para_name),*);
					}
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

impl<B: BehaviorSafe + MultiplexedHandler> BehaviorList for SimpleBehaviorList<B> {
    type View<'a> = B::Multiplexer<'a, std::slice::Iter<'a, B>>;
    type Delegate = B;

    fn extend(&mut self, mut other: Self) {
        self.behaviors.append(&mut other.behaviors);
    }

    fn extend_ref(&mut self, other: &Self) {
        self.behaviors.extend(other.behaviors.iter().cloned());
    }

    fn view<'a>(me: Option<&'a Self>) -> Self::View<'a> {
        B::make_multiplexer(me.map_or(std::slice::Iter::default(), |l| l.behaviors.iter()))
    }
}

impl<B: BehaviorSafe + MultiplexedHandler> ExtendableBehaviorList for SimpleBehaviorList<B> {
    fn push(&mut self, delegate: Self::Delegate, _meta: ()) {
        self.behaviors.push(delegate);
    }
}
