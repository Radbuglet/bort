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

use derive_where::derive_where;
use petgraph::{stable_graph::NodeIndex, Graph};

use std::{
    any::{Any, TypeId},
    fmt, hash,
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
    type List: BehaviorList;
    type View<'a>;

    fn view<'a>(bhv: &'a BehaviorRegistry, list: Option<&'a Self::List>) -> Self::View<'a>;
}

pub trait BehaviorList: 'static + Send + Sync + Default {
    type Element;
    type ElementIter<'a>: Iterator<Item = &'a Self::Element>;

    fn iter(&self) -> Self::ElementIter<'_>;
}

pub trait PushToBehaviorList<L: BehaviorList> {
    fn push_to(self, list: &mut L);
}

// DAG behavior list
#[derive(Debug, Clone)]
#[derive_where(Default)]
pub struct DagBehaviorList<D, I = BehaviorBarrierId> {
    graph: Graph<DagGraphNode<D, I>, ()>,
    id_map: FxHashMap<I, NodeIndex>,
}

#[derive(Debug, Clone)]
enum DagGraphNode<D, I> {
    Terminal(D),
    NonTerminal { _id: I, delegate: Option<D> },
}

// TODO: Error handling
impl<D, I> BehaviorList for DagBehaviorList<D, I>
where
    D: 'static + Sync + Send,
    I: 'static + Sync + Send + Copy + fmt::Debug,
{
    type Element = D;
    type ElementIter<'a> = DagBehaviorListIter<'a, D, I>;

    fn iter(&self) -> Self::ElementIter<'_> {
        DagBehaviorListIter {
            graph: &self.graph,
            exec_order: petgraph::algo::toposort(&self.graph, None)
                .unwrap()
                .into_iter(),
        }
    }
}

#[derive(Debug)]
pub struct DagBehaviorListIter<'a, D, I> {
    graph: &'a Graph<DagGraphNode<D, I>, ()>,
    exec_order: std::vec::IntoIter<NodeIndex>,
}

impl<'a, D, I> Iterator for DagBehaviorListIter<'a, D, I>
where
    I: fmt::Debug,
{
    type Item = &'a D;

    fn next(&mut self) -> Option<Self::Item> {
        self.exec_order.next().map(|v| match &self.graph[v] {
            DagGraphNode::Terminal(delegate) => delegate,
            DagGraphNode::NonTerminal { delegate, .. } => delegate.as_ref().unwrap(),
        })
    }
}

impl<D, I> PushToBehaviorList<DagBehaviorList<D, I>> for D
where
    D: 'static + Sync + Send,
    I: 'static + Sync + Send + Copy + fmt::Debug + Eq + hash::Hash,
{
    fn push_to(self, list: &mut DagBehaviorList<D, I>) {
        list.graph.add_node(DagGraphNode::Terminal(self));
    }
}

#[derive(Debug)]
pub struct DagBehaviorOrdering<D, I, IL> {
    pub delegate: D,
    pub id: I,
    pub deps: IL,
}

impl<D, I, IL> PushToBehaviorList<DagBehaviorList<D, I>> for DagBehaviorOrdering<D, I, IL>
where
    D: 'static + Sync + Send,
    I: 'static + Sync + Send + Copy + fmt::Debug + Eq + hash::Hash,
    IL: IntoIterator<Item = I>,
{
    fn push_to(self, list: &mut DagBehaviorList<D, I>) {
        let index = list.graph.add_node(DagGraphNode::NonTerminal {
            _id: self.id,
            delegate: Some(self.delegate),
        });
        list.id_map.insert(self.id, index);

        for dep in self.deps {
            let dep_index = *list.id_map.entry(dep).or_insert_with(|| {
                list.graph.add_node(DagGraphNode::NonTerminal {
                    _id: self.id,
                    delegate: None,
                })
            });

            list.graph.add_edge(dep_index, index, ());
        }
    }
}

pub trait DagBehaviorOrderingCtorExt: Sized {
    type Id;

    fn with_ordering<IL>(self, id: Self::Id, deps: IL) -> DagBehaviorOrdering<Self, Self::Id, IL>
    where
        IL: IntoIterator<Item = Self::Id>;
}

impl<D, I> DagBehaviorOrderingCtorExt for D
where
    D: BehaviorDelegate<List = DagBehaviorList<D, I>>,
{
    type Id = I;

    fn with_ordering<IL>(self, id: Self::Id, deps: IL) -> DagBehaviorOrdering<Self, Self::Id, IL>
    where
        IL: IntoIterator<Item = Self::Id>,
    {
        DagBehaviorOrdering {
            delegate: self,
            id,
            deps,
        }
    }
}

// DagTypeId
#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub struct BehaviorBarrierId {
    id: TypeId,
}

pub trait BehaviorBarrierMarker: 'static {
    fn barrier_id() -> BehaviorBarrierId {
        BehaviorBarrierId {
            id: TypeId::of::<Self>(),
        }
    }
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

    pub fn register<B: BehaviorKind>(
        &mut self,
        delegate: impl PushToBehaviorList<<B::Delegate as BehaviorDelegate>::List>,
    ) -> &mut Self {
        let own_registry = self
            .behaviors
            .entry(NamedTypeId::of::<B>())
            .or_insert_with(|| Box::<<B::Delegate as BehaviorDelegate>::List>::default())
            .downcast_mut::<<B::Delegate as BehaviorDelegate>::List>()
            .unwrap();

        delegate.push_to(own_registry);

        self
    }

    pub fn register_combined<B>(&mut self, delegate: B) -> &mut Self
    where
        B: BehaviorKind<Delegate = B> + BehaviorDelegate + PushToBehaviorList<B::List>,
    {
        self.register::<B>(delegate)
    }

    pub fn register_many(&mut self, registrar: impl FnOnce(&mut Self)) -> &mut Self {
        registrar(self);
        self
    }

    pub fn with<B: BehaviorKind>(
        mut self,
        delegate: impl PushToBehaviorList<<B::Delegate as BehaviorDelegate>::List>,
    ) -> Self {
        self.register::<B>(delegate);
        self
    }

    pub fn with_combined<B>(mut self, delegate: B) -> Self
    where
        B: BehaviorKind<Delegate = B> + BehaviorDelegate + PushToBehaviorList<B::List>,
    {
        self.register_combined(delegate);
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
        super::{
            behavior_delegate, BehaviorDelegate, BehaviorList, BehaviorRegistry, DagBehaviorList,
        },
        crate::event::ProcessableEventList,
        std::{boxed::Box, compile_error, concat, option::Option, stringify},
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
        args { $($ty:ty)? }

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
            type List = $crate::behavior::behavior_derive_macro_internal::behavior_delegate!(
				@__internal_choose_first
					$({ $ty })?
					{ $crate::behavior::behavior_derive_macro_internal::DagBehaviorList<Self> }
			);
            type View<'a> = $crate::behavior::behavior_derive_macro_internal::Box<dyn $(for<$($fn_lt,)*>)? Fn($($para),*) + 'a>;

            fn view<'a>(
                bhv: &'a $crate::behavior::behavior_derive_macro_internal::BehaviorRegistry,
                list: $crate::behavior::behavior_derive_macro_internal::Option<&'a Self::List>,
            ) -> Self::View<'a> {
                $crate::behavior::behavior_derive_macro_internal::Box::new(move |$($para_name,)*| {
					let Some(list) = list else { return };

                    for behavior in $crate::behavior::behavior_derive_macro_internal::BehaviorList::iter(list) {
                        behavior(bhv, $($para_name,)*);
                    }
                })
            }
        }
    };
	(@__internal_choose_first {$($first:tt)*} $({ $($ignored:tt)* })*) => { $($first)* };
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
