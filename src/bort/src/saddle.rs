use std::marker::PhantomData;

use crate::{
    entity::{CompMut, CompRef, Entity, OwnedEntity},
    obj::{Obj, OwnedObj},
};

use derive_where::derive_where;

// === Saddle re-exports === //

pub use saddle::{self, behavior, cx, namespace, AccessMut, AccessRef, BehaviorToken};

pub type RootBehaviorToken = saddle::RootBehaviorToken<BortComponents>;

saddle::universe!(pub BortComponents);

// === Safe method variants === //

impl Entity {
    #[track_caller]
    pub fn get_s<T: 'static>(self, _cx: &saddle::cx![BortComponents; ref T]) -> CompRef<'_, T> {
        self.get()
    }

    #[track_caller]
    pub fn get_mut_s<T: 'static>(self, _cx: &saddle::cx![BortComponents; mut T]) -> CompMut<'_, T> {
        self.get_mut()
    }
}

impl OwnedEntity {
    #[track_caller]
    pub fn get_s<'b, T: 'static>(
        &self,
        _cx: &'b saddle::cx![BortComponents; ref T],
    ) -> CompRef<'b, T> {
        self.get()
    }

    #[track_caller]
    pub fn get_mut_s<'b, T: 'static>(
        &self,
        _cx: &'b saddle::cx![BortComponents; mut T],
    ) -> CompMut<'b, T> {
        self.get_mut()
    }
}

impl<T: 'static> Obj<T> {
    #[track_caller]
    pub fn get_s(self, _cx: &saddle::cx![BortComponents; ref T]) -> CompRef<'_, T> {
        self.get()
    }

    #[track_caller]
    pub fn get_mut_s(self, _cx: &saddle::cx![BortComponents; mut T]) -> CompMut<'_, T> {
        self.get_mut()
    }
}

impl<T: 'static> OwnedObj<T> {
    #[track_caller]
    pub fn get_s<'b>(&self, _cx: &'b saddle::cx![BortComponents; ref T]) -> CompRef<'b, T> {
        self.get()
    }

    #[track_caller]
    pub fn get_mut_s<'b>(&self, _cx: &'b saddle::cx![BortComponents; mut T]) -> CompMut<'b, T> {
        self.get_mut()
    }
}

// === Late borrows === //

#[derive_where(Copy; F: Copy)]
#[derive_where(Clone; F: Clone)]
pub struct LateBorrow<T, F> {
    _ty: PhantomData<fn() -> T>,
    f: F,
}

impl<T, F> LateBorrow<T, F>
where
    T: 'static,
    F: Fn(&'_ dyn AccessRef<BortComponents, T>) -> CompRef<'_, T>,
{
    #[track_caller]
    pub fn get<'b>(&self, cx: &'b cx![BortComponents; ref T]) -> CompRef<'b, T> {
        (self.f)(cx.as_dyn())
    }
}

pub fn late_borrow<T, F>(f: F) -> LateBorrow<T, F>
where
    T: 'static,
    F: for<'b> Fn(&'b dyn AccessRef<BortComponents, T>) -> CompRef<'b, T>,
{
    LateBorrow {
        _ty: PhantomData,
        f,
    }
}

#[derive_where(Copy; F: Copy)]
#[derive_where(Clone; F: Clone)]
pub struct LateBorrowMut<T, F> {
    _ty: PhantomData<fn() -> T>,
    f: F,
}

impl<T, F> LateBorrowMut<T, F>
where
    T: 'static,
    F: Fn(&'_ dyn AccessMut<BortComponents, T>) -> CompMut<'_, T>,
{
    #[track_caller]
    pub fn get<'b>(&self, cx: &'b cx![BortComponents; mut T]) -> CompMut<'b, T> {
        (self.f)(cx.as_dyn_mut())
    }
}

pub fn late_borrow_mut<T, F>(f: F) -> LateBorrowMut<T, F>
where
    T: 'static,
    F: for<'b> Fn(&'_ dyn AccessMut<BortComponents, T>) -> CompMut<'_, T>,
{
    LateBorrowMut {
        _ty: PhantomData,
        f,
    }
}
