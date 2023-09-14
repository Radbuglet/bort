use std::{any::type_name, fmt, marker::PhantomData};

use crate::{
    entity::{CompMut, CompRef, Entity, OwnedEntity},
    obj::{Obj, OwnedObj},
};

// === CxValue === //

mod cx_value_sealed {
    use std::{fmt, marker::PhantomData};

    pub struct AccessToken<T: 'static>(pub PhantomData<fn() -> T>);

    pub trait Sealed {
        const SHOULD_FMT: bool;

        type Storage;
        type WithLt<'b>: super::CxValue;

        fn new() -> Self::Storage;

        fn fmt_value(f: &mut fmt::Formatter) -> fmt::Result;
    }
}

pub trait CxValue: cx_value_sealed::Sealed {}

impl cx_value_sealed::Sealed for NoValue {
    const SHOULD_FMT: bool = false;

    type Storage = NoValue;
    type WithLt<'b> = NoValue;

    fn new() -> Self::Storage {
        NoValue
    }

    fn fmt_value(_f: &mut std::fmt::Formatter) -> std::fmt::Result {
        Ok(())
    }
}

impl CxValue for NoValue {}

impl<'a, T: 'static> cx_value_sealed::Sealed for &'a T {
    const SHOULD_FMT: bool = true;

    type Storage = &'a cx_value_sealed::AccessToken<T>;
    type WithLt<'b> = &'b T;

    fn new() -> Self::Storage {
        &cx_value_sealed::AccessToken(PhantomData)
    }

    fn fmt_value(f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "&{}", type_name::<T>())
    }
}

impl<T: 'static> CxValue for &'_ T {}

impl<'a, T: 'static> cx_value_sealed::Sealed for &'a mut T {
    const SHOULD_FMT: bool = true;

    type Storage = &'a mut cx_value_sealed::AccessToken<T>;
    type WithLt<'b> = &'b mut T;

    fn new() -> Self::Storage {
        Box::leak(Box::new(cx_value_sealed::AccessToken(PhantomData)))
    }

    fn fmt_value(f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "&mut {}", type_name::<T>())
    }
}

impl<T: 'static> CxValue for &'_ mut T {}

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
    pub fn new() -> Self {
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
    (assert_noalias $target:ident) => {{
		let binder = $crate::saddle::cx_macro_internals::PhantomData;

		#[allow(unreachable_code)]
        if false {
			loop {}  // This is necessary in case droppable objects depend on borrows of $target.
            $crate::saddle::cx_macro_internals::ensure_is_context(&$target, &binder);
        }

		let duplicate = $crate::saddle::cx_macro_internals::bind_marker(
			&binder,
			$crate::saddle::cx_macro_internals::Cx::new(),
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

// === Procedure Collections === //

// TODO
