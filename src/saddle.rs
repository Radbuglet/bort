use std::{any::type_name, fmt, marker::PhantomData};

use crate::{
    behavior::BehaviorKind,
    core::cell::{OptRef, OptRefMut},
    entity::{CompMut, CompRef, Entity, OwnedEntity},
    event::{EventGroup, EventGroupMarkerWithSeparated},
    obj::{Obj, OwnedObj},
};

// === Saddle Integration === //

pub use saddle::{scope, Scope};

scope! {
    pub BehaviorKindScope<Bhv>
    where { Bhv: BehaviorKind }
}

#[doc(hidden)]
pub mod macro_internals_saddle_delegate {
    pub use {
        super::BehaviorKindScope,
        crate::behavior::{behavior_delegate, behavior_kind, delegate, BehaviorProvider},
    };
}

#[macro_export]
macro_rules! saddle_delegate {
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
        $crate::saddle::macro_internals_saddle_delegate::delegate!(
            $(#[$attr_meta])*
            $vis fn $name
                $(
                    <$($generic),*>
                    $(<$($fn_lt),*>)?
                )?
                (
                    bhv: $crate::saddle::macro_internals_saddle_delegate::BehaviorProvider<'_>,
                    call_cx: &mut $crate::saddle::macro_internals_saddle_delegate::BehaviorKindScope<$name>,
                    $($para_name: $para),*
                ) $(-> $ret)?
            as deriving $crate::saddle::macro_internals_saddle_delegate::behavior_kind
            as deriving $crate::saddle::macro_internals_saddle_delegate::behavior_delegate { $($list)? }
            $(as deriving $deriving $({ $($deriving_args)* })? )*
            $(where $($where_token)*)?
        );
    };
}

pub use saddle_delegate;

// === CxValue === //

mod cx_value_sealed {
    use std::{fmt, marker::PhantomData};

    use saddle::Scope;

    pub struct AccessToken<T: 'static>(pub PhantomData<fn() -> T>);

    pub trait Sealed {
        const SHOULD_FMT: bool;

        type Storage;

        fn new() -> Self::Storage;

        fn fmt_value(f: &mut fmt::Formatter) -> fmt::Result;

        fn decl_borrows(s: &impl Scope);

        fn decl_grants(s: &impl Scope);
    }
}

pub trait CxValue: cx_value_sealed::Sealed {}

pub trait CxValueLt<'a>: CxValue {}

impl cx_value_sealed::Sealed for NoValue {
    const SHOULD_FMT: bool = false;

    type Storage = NoValue;

    fn new() -> Self::Storage {
        NoValue
    }

    fn fmt_value(_f: &mut std::fmt::Formatter) -> std::fmt::Result {
        Ok(())
    }

    fn decl_borrows(s: &impl Scope) {
        let _ = s;
    }

    fn decl_grants(s: &impl Scope) {
        let _ = s;
    }
}

impl CxValue for NoValue {}

impl<'a> CxValueLt<'a> for NoValue {}

impl<'a, T: 'static> cx_value_sealed::Sealed for &'a T {
    const SHOULD_FMT: bool = true;

    type Storage = &'a cx_value_sealed::AccessToken<T>;

    fn new() -> Self::Storage {
        &cx_value_sealed::AccessToken(PhantomData)
    }

    fn fmt_value(f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "&{}", type_name::<T>())
    }

    fn decl_borrows(s: &impl Scope) {
        s.decl_dep_ref::<cx_value_sealed::AccessToken<T>>();
    }

    fn decl_grants(s: &impl Scope) {
        s.decl_grant_ref::<cx_value_sealed::AccessToken<T>>();
    }
}

impl<T: 'static> CxValue for &'_ T {}

impl<'a, T: 'static> CxValueLt<'a> for &'a T {}

impl<'a, T: 'static> cx_value_sealed::Sealed for &'a mut T {
    const SHOULD_FMT: bool = true;

    type Storage = &'a mut cx_value_sealed::AccessToken<T>;

    fn new() -> Self::Storage {
        Box::leak(Box::new(cx_value_sealed::AccessToken(PhantomData)))
    }

    fn fmt_value(f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "&mut {}", type_name::<T>())
    }

    fn decl_borrows(s: &impl Scope) {
        s.decl_dep_mut::<cx_value_sealed::AccessToken<T>>();
    }

    fn decl_grants(s: &impl Scope) {
        s.decl_grant_mut::<cx_value_sealed::AccessToken<T>>();
    }
}

impl<T: 'static> CxValue for &'_ mut T {}

impl<'a, T: 'static> CxValueLt<'a> for &'a mut T {}

// === Cx === //

pub use compost2::NoValue;
use NoValue as N;

pub struct Cx<A = N, B = N, C = N, D = N, E = N, F = N, G = N, H = N, I = N, J = N, K = N, L = N>
where
    A: CxValue,
    B: CxValue,
    C: CxValue,
    D: CxValue,
    E: CxValue,
    F: CxValue,
    G: CxValue,
    H: CxValue,
    I: CxValue,
    J: CxValue,
    K: CxValue,
    L: CxValue,
{
    #[doc(hidden)]
    pub macro_internal_compost_cx: compost2::Cx<
        A::Storage,
        B::Storage,
        C::Storage,
        D::Storage,
        E::Storage,
        F::Storage,
        G::Storage,
        H::Storage,
        I::Storage,
        J::Storage,
        K::Storage,
        L::Storage,
    >,
}

impl<'a, A, B, C, D, E, F, G, H, I, J, K, L> Cx<A, B, C, D, E, F, G, H, I, J, K, L>
where
    A: CxValueLt<'a>,
    B: CxValueLt<'a>,
    C: CxValueLt<'a>,
    D: CxValueLt<'a>,
    E: CxValueLt<'a>,
    F: CxValueLt<'a>,
    G: CxValueLt<'a>,
    H: CxValueLt<'a>,
    I: CxValueLt<'a>,
    J: CxValueLt<'a>,
    K: CxValueLt<'a>,
    L: CxValueLt<'a>,
{
    pub fn new(scope: &'a mut impl Scope) -> (&'a mut impl Scope, Self) {
        scope! { scope:
            Self::decl_borrows(scope);
            return (scope, Self::new_unchecked());
        }
    }
}

impl<A, B, C, D, E, F, G, H, I, J, K, L> Cx<A, B, C, D, E, F, G, H, I, J, K, L>
where
    A: CxValue,
    B: CxValue,
    C: CxValue,
    D: CxValue,
    E: CxValue,
    F: CxValue,
    G: CxValue,
    H: CxValue,
    I: CxValue,
    J: CxValue,
    K: CxValue,
    L: CxValue,
{
    pub fn new_unchecked() -> Self {
        Self {
            macro_internal_compost_cx: compost2::Cx::from((
                A::new(),
                B::new(),
                C::new(),
                D::new(),
                E::new(),
                F::new(),
                G::new(),
                H::new(),
                I::new(),
                J::new(),
                K::new(),
                L::new(),
            )),
        }
    }

    pub fn decl_borrows(s: &impl Scope) {
        A::decl_borrows(s);
        B::decl_borrows(s);
        C::decl_borrows(s);
        D::decl_borrows(s);
        E::decl_borrows(s);
        F::decl_borrows(s);
        G::decl_borrows(s);
        H::decl_borrows(s);
        I::decl_borrows(s);
        J::decl_borrows(s);
        K::decl_borrows(s);
        L::decl_borrows(s);
    }

    pub fn decl_grants(s: &impl Scope) {
        A::decl_grants(s);
        B::decl_grants(s);
        C::decl_grants(s);
        D::decl_grants(s);
        E::decl_grants(s);
        F::decl_grants(s);
        G::decl_grants(s);
        H::decl_grants(s);
        I::decl_grants(s);
        J::decl_grants(s);
        K::decl_grants(s);
        L::decl_grants(s);
    }
}

impl<A, B, C, D, E, F, G, H, I, J, K, L> fmt::Debug for Cx<A, B, C, D, E, F, G, H, I, J, K, L>
where
    A: CxValue,
    B: CxValue,
    C: CxValue,
    D: CxValue,
    E: CxValue,
    F: CxValue,
    G: CxValue,
    H: CxValue,
    I: CxValue,
    J: CxValue,
    K: CxValue,
    L: CxValue,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn fmt_cx<T: CxValue>(f: &mut fmt::DebugTuple) {
            struct FmtCxValue<T>([T; 0]);

            impl<T: CxValue> fmt::Debug for FmtCxValue<T> {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    T::fmt_value(f)
                }
            }

            if T::SHOULD_FMT {
                f.field(&FmtCxValue::<T>([]));
            }
        }

        let mut f = f.debug_tuple("Cx");
        fmt_cx::<A>(&mut f);
        fmt_cx::<B>(&mut f);
        fmt_cx::<C>(&mut f);
        fmt_cx::<D>(&mut f);
        fmt_cx::<E>(&mut f);
        fmt_cx::<F>(&mut f);
        fmt_cx::<G>(&mut f);
        fmt_cx::<H>(&mut f);
        fmt_cx::<I>(&mut f);
        fmt_cx::<J>(&mut f);
        fmt_cx::<K>(&mut f);
        fmt_cx::<L>(&mut f);

        f.finish()
    }
}

mod any_cx_sealed {
    pub trait Sealed {}
}

pub trait AnyCx: fmt::Debug + any_cx_sealed::Sealed {
    fn new_unchecked() -> Self;

    fn decl_borrows(s: &impl Scope);

    fn decl_grants(s: &impl Scope);
}

pub trait AnyCxLt<'a>: AnyCx {}

impl<A, B, C, D, E, F, G, H, I, J, K, L> any_cx_sealed::Sealed
    for Cx<A, B, C, D, E, F, G, H, I, J, K, L>
where
    A: CxValue,
    B: CxValue,
    C: CxValue,
    D: CxValue,
    E: CxValue,
    F: CxValue,
    G: CxValue,
    H: CxValue,
    I: CxValue,
    J: CxValue,
    K: CxValue,
    L: CxValue,
{
}

impl<A, B, C, D, E, F, G, H, I, J, K, L> AnyCx for Cx<A, B, C, D, E, F, G, H, I, J, K, L>
where
    A: CxValue,
    B: CxValue,
    C: CxValue,
    D: CxValue,
    E: CxValue,
    F: CxValue,
    G: CxValue,
    H: CxValue,
    I: CxValue,
    J: CxValue,
    K: CxValue,
    L: CxValue,
{
    fn new_unchecked() -> Self {
        Self::new_unchecked()
    }

    fn decl_borrows(s: &impl Scope) {
        Self::decl_borrows(s);
    }

    fn decl_grants(s: &impl Scope) {
        Self::decl_grants(s);
    }
}

impl<'a, A, B, C, D, E, F, G, H, I, J, K, L> AnyCxLt<'a> for Cx<A, B, C, D, E, F, G, H, I, J, K, L>
where
    A: CxValueLt<'a>,
    B: CxValueLt<'a>,
    C: CxValueLt<'a>,
    D: CxValueLt<'a>,
    E: CxValueLt<'a>,
    F: CxValueLt<'a>,
    G: CxValueLt<'a>,
    H: CxValueLt<'a>,
    I: CxValueLt<'a>,
    J: CxValueLt<'a>,
    K: CxValueLt<'a>,
    L: CxValueLt<'a>,
{
}

// === `cx!` macro === //

#[doc(hidden)]
pub mod cx_macro_internals {
    use super::CxValue;

    pub use {super::Cx, compost2::cx as compost_cx, std::marker::PhantomData};

    pub fn ensure_is_context<A, B, C, D, E, F, G, H, I, J, K, L>(
        _cx: &Cx<A, B, C, D, E, F, G, H, I, J, K, L>,
        _bound: &PhantomData<Cx<A, B, C, D, E, F, G, H, I, J, K, L>>,
    ) -> !
    where
        A: CxValue,
        B: CxValue,
        C: CxValue,
        D: CxValue,
        E: CxValue,
        F: CxValue,
        G: CxValue,
        H: CxValue,
        I: CxValue,
        J: CxValue,
        K: CxValue,
        L: CxValue,
    {
        unreachable!();
    }

    pub fn bind_marker<T>(_bound: &PhantomData<T>, value: T) -> T {
        value
    }
}

#[macro_export]
macro_rules! cx {
    (noalias $target:ident) => {{
		let binder = $crate::saddle::cx_macro_internals::PhantomData;

		#[allow(unreachable_code)]
        if false {
			loop {}  // This is necessary in case droppable objects depend on borrows of $target.
            $crate::saddle::cx_macro_internals::ensure_is_context(&$target, &binder);
        }

		let duplicate = $crate::saddle::cx_macro_internals::bind_marker(
			&binder,
			$crate::saddle::cx_macro_internals::Cx::new_unchecked(),
		);

		$crate::saddle::cx_macro_internals::Cx {
			macro_internal_compost_cx: $crate::saddle::cx_macro_internals::compost_cx!(
				dangerously_specify_place { duplicate.macro_internal_compost_cx },
			),
		}
    }};
	($target:ident) => {{
		#[allow(unreachable_code)]
        if false {
			loop {}  // This is necessary in case droppable objects depend on borrows of $target.
            $crate::saddle::cx_macro_internals::ensure_is_context(
				&$target,
				&$crate::saddle::cx_macro_internals::PhantomData,
			);
        }

		$crate::saddle::cx_macro_internals::Cx {
			macro_internal_compost_cx: $crate::saddle::cx_macro_internals::compost_cx!(
				dangerously_specify_place { $target.macro_internal_compost_cx },
			),
		}
    }};
}

pub use cx;

// === Safe Variants === //

impl Entity {
    #[track_caller]
    pub fn try_get_s<T: 'static>(self, _cx: Cx<&T>) -> Option<CompRef<'_, T>> {
        self.try_get()
    }

    #[track_caller]
    pub fn try_get_mut_s<T: 'static>(self, _cx: Cx<&mut T>) -> Option<CompMut<'_, T>> {
        self.try_get_mut()
    }

    #[track_caller]
    pub fn get_s<T: 'static>(self, _cx: Cx<&T>) -> CompRef<'_, T> {
        self.get()
    }

    #[track_caller]
    pub fn get_mut_s<T: 'static>(self, _cx: Cx<&mut T>) -> CompMut<'_, T> {
        self.get_mut()
    }
}

impl OwnedEntity {
    #[track_caller]
    pub fn try_get_s<'b, T: 'static>(&self, _cx: Cx<&'b T>) -> Option<CompRef<'b, T>> {
        self.try_get()
    }

    #[track_caller]
    pub fn try_get_mut_s<'b, T: 'static>(&self, _cx: Cx<&'b mut T>) -> Option<CompMut<'b, T>> {
        self.try_get_mut()
    }

    #[track_caller]
    pub fn get_s<'b, T: 'static>(&self, _cx: Cx<&'b T>) -> CompRef<'b, T> {
        self.get()
    }

    #[track_caller]
    pub fn get_mut_s<'b, T: 'static>(&self, _cx: Cx<&'b mut T>) -> CompMut<'b, T> {
        self.get_mut()
    }
}

impl<T: 'static> Obj<T> {
    #[track_caller]
    pub fn try_get_s(self, _cx: Cx<&T>) -> Option<CompRef<'_, T>> {
        self.try_get()
    }

    #[track_caller]
    pub fn try_get_mut_s(self, _cx: Cx<&mut T>) -> Option<CompMut<'_, T>> {
        self.try_get_mut()
    }

    #[track_caller]
    pub fn get_s(self, _cx: Cx<&T>) -> CompRef<'_, T> {
        self.get()
    }

    #[track_caller]
    pub fn get_mut_s(self, _cx: Cx<&mut T>) -> CompMut<'_, T> {
        self.get_mut()
    }
}

impl<T: 'static> OwnedObj<T> {
    #[track_caller]
    pub fn try_get_s<'b>(&self, _cx: Cx<&'b T>) -> Option<CompRef<'b, T>> {
        self.try_get()
    }

    #[track_caller]
    pub fn try_get_mut_s<'b>(&self, _cx: Cx<&'b mut T>) -> Option<CompMut<'b, T>> {
        self.try_get_mut()
    }

    #[track_caller]
    pub fn get_s<'b>(&self, _cx: Cx<&'b T>) -> CompRef<'b, T> {
        self.get()
    }

    #[track_caller]
    pub fn get_mut_s<'b>(&self, _cx: Cx<&'b mut T>) -> CompMut<'b, T> {
        self.get_mut()
    }
}

impl<G: ?Sized> EventGroup<G> {
    pub fn get_s<'a, E>(&'a self, _cx: Cx<&'a G::List>) -> OptRef<'a, G::List>
    where
        G: EventGroupMarkerWithSeparated<E>,
    {
        self.get::<E>()
    }

    pub fn get_mut_s<'a, E>(&'a self, _cx: Cx<&'a mut G::List>) -> OptRefMut<'_, G::List>
    where
        G: EventGroupMarkerWithSeparated<E>,
    {
        self.get_mut::<E>()
    }
}

// === ScopeExt === //

pub trait ScopeExt {
    fn call_with_grant<'a, C: AnyCxLt<'a>, S: Scope>(&'a mut self, cx: C) -> &'a mut S;
}

impl<SParent: Scope> ScopeExt for SParent {
    fn call_with_grant<'a, C: AnyCxLt<'a>, SChild: Scope>(&'a mut self, cx: C) -> &'a mut SChild {
        let _ = cx;

        scope!(SubScope<SParent, SChild>);

        let sub_scope = self.decl_call::<SubScope<SParent, SChild>>();
        C::decl_grants(sub_scope);
        sub_scope.decl_call()
    }
}
