use std::marker::PhantomData;

use crate::{
    entity::{CompMut, CompRef, Entity, OwnedEntity},
    obj::{Obj, OwnedObj},
};

use derive_where::derive_where;

// === Saddle re-exports === //

pub use saddle::{self, behavior, cx, namespace, BehaviorToken};

saddle::universe!(pub BortComponents);

pub type RootBehaviorToken = saddle::RootBehaviorToken<BortComponents>;

pub trait AccessRef<T: ?Sized>: saddle::AccessRef<BortComponents, T> {
    fn as_dyn_bort(&self) -> &dyn AccessRef<T>;
}

impl<R, T> AccessRef<T> for R
where
    R: ?Sized + saddle::AccessRef<BortComponents, T>,
    T: ?Sized,
{
    fn as_dyn_bort(&self) -> &dyn AccessRef<T> {
        &saddle::SuperDangerousGlobalToken
    }
}

pub trait AccessMut<T: ?Sized>: saddle::AccessMut<BortComponents, T> {
    fn as_dyn_mut_bort(&self) -> &dyn AccessMut<T> {
        &saddle::SuperDangerousGlobalToken
    }
}

impl<R, T> AccessMut<T> for R
where
    R: ?Sized + saddle::AccessMut<BortComponents, T>,
    T: ?Sized,
{
}

// === Safe method variants === //

impl Entity {
    #[track_caller]
    pub fn try_get_s<T: 'static>(
        self,
        _cx: &saddle::cx![BortComponents; ref T],
    ) -> Option<CompRef<'_, T>> {
        self.try_get()
    }

    #[track_caller]
    pub fn try_get_mut_s<T: 'static>(
        self,
        _cx: &saddle::cx![BortComponents; mut T],
    ) -> Option<CompMut<'_, T>> {
        self.try_get_mut()
    }

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
    pub fn try_get_s<'b, T: 'static>(
        &self,
        _cx: &'b saddle::cx![BortComponents; ref T],
    ) -> Option<CompRef<'b, T>> {
        self.try_get()
    }

    #[track_caller]
    pub fn try_get_mut_s<'b, T: 'static>(
        &self,
        _cx: &'b saddle::cx![BortComponents; mut T],
    ) -> Option<CompMut<'b, T>> {
        self.try_get_mut()
    }

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
    pub fn try_get_s(self, _cx: &saddle::cx![BortComponents; ref T]) -> Option<CompRef<'_, T>> {
        self.try_get()
    }

    #[track_caller]
    pub fn try_get_mut_s(self, _cx: &saddle::cx![BortComponents; mut T]) -> Option<CompMut<'_, T>> {
        self.try_get_mut()
    }

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
    pub fn try_get_s<'b>(
        &self,
        _cx: &'b saddle::cx![BortComponents; ref T],
    ) -> Option<CompRef<'b, T>> {
        self.try_get()
    }

    #[track_caller]
    pub fn try_get_mut_s<'b>(
        &self,
        _cx: &'b saddle::cx![BortComponents; mut T],
    ) -> Option<CompMut<'b, T>> {
        self.try_get_mut()
    }

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
    F: Fn(&'_ dyn AccessRef<T>) -> CompRef<'_, T>,
{
    #[track_caller]
    pub fn get<'b>(&self, cx: &'b cx![BortComponents; ref T]) -> CompRef<'b, T> {
        (self.f)(cx.as_dyn_bort())
    }
}

pub fn late_borrow<T, F>(f: F) -> LateBorrow<T, F>
where
    T: 'static,
    F: for<'b> Fn(&'b dyn AccessRef<T>) -> CompRef<'b, T>,
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
    F: Fn(&'_ dyn AccessMut<T>) -> CompMut<'_, T>,
{
    #[track_caller]
    pub fn get<'b>(&self, cx: &'b cx![BortComponents; mut T]) -> CompMut<'b, T> {
        (self.f)(cx.as_dyn_mut_bort())
    }
}

pub fn late_borrow_mut<T, F>(f: F) -> LateBorrowMut<T, F>
where
    T: 'static,
    F: for<'b> Fn(&'_ dyn AccessMut<T>) -> CompMut<'_, T>,
{
    LateBorrowMut {
        _ty: PhantomData,
        f,
    }
}
