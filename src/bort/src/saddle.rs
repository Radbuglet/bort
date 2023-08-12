use crate::{
    entity::{CompMut, CompRef, Entity, OwnedEntity},
    obj::{Obj, OwnedObj},
};

pub use saddle::{self, behavior, cx, namespace, BehaviorToken, RootBehaviorToken};

saddle::universe!(pub BortComponents);

impl Entity {
    pub fn get_s<T: 'static>(self, _cx: &saddle::cx![BortComponents; ref T]) -> CompRef<'_, T> {
        self.get()
    }

    pub fn get_mut_s<T: 'static>(self, _cx: &saddle::cx![BortComponents; mut T]) -> CompMut<'_, T> {
        self.get_mut()
    }
}

impl OwnedEntity {
    pub fn get_s<'b, T: 'static>(
        &self,
        _cx: &'b saddle::cx![BortComponents; ref T],
    ) -> CompRef<'b, T> {
        self.get()
    }

    pub fn get_mut_s<'b, T: 'static>(
        &self,
        _cx: &'b saddle::cx![BortComponents; mut T],
    ) -> CompMut<'b, T> {
        self.get_mut()
    }
}

impl<T: 'static> Obj<T> {
    pub fn get_s(self, _cx: &saddle::cx![BortComponents; ref T]) -> CompRef<'_, T> {
        self.get()
    }

    pub fn get_mut_s(self, _cx: &saddle::cx![BortComponents; mut T]) -> CompMut<'_, T> {
        self.get_mut()
    }
}

impl<T: 'static> OwnedObj<T> {
    pub fn get_s<'b>(&self, _cx: &'b saddle::cx![BortComponents; ref T]) -> CompRef<'b, T> {
        self.get()
    }

    pub fn get_mut_s<'b>(&self, _cx: &'b saddle::cx![BortComponents; mut T]) -> CompMut<'b, T> {
        self.get_mut()
    }
}
