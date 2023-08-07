use std::{any::type_name, borrow::Borrow, mem};

use derive_where::derive_where;

use crate::{
    core::{
        heap::Slot,
        token::{MainThreadToken, Token},
    },
    debug::AsDebugLabel,
    entity::{CompMut, CompRef, Entity, OwnedEntity},
};

// === Obj === //

#[derive(Debug)]
#[derive_where(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct Obj<T: 'static> {
    entity: Entity,
    #[derive_where(skip)]
    value: Slot<T>,
}

impl<T: 'static> Obj<T> {
    pub fn from_raw_parts(entity: Entity, value: Slot<T>) -> Self {
        Self { entity, value }
    }

    pub fn insert(entity: Entity, value: T) -> Self {
        entity.insert_with_obj(value).1
    }

    pub fn wrap(entity: Entity) -> Self {
        Self {
            entity,
            value: entity.get_slot(),
        }
    }

    pub fn new_unmanaged(value: T) -> Self {
        Self::insert(Entity::new_unmanaged(), value)
    }

    pub fn entity(self) -> Entity {
        self.entity
    }

    fn is_alive_internal(self, token: &impl Token) -> bool {
        self.value.owner(token) == Some(self.entity)
    }

    pub fn is_alive(self) -> bool {
        self.is_alive_internal(MainThreadToken::acquire_fmt(
            "determine whether an Obj was alive",
        ))
    }

    pub fn with_debug_label<L: AsDebugLabel>(self, label: L) -> Self {
        self.entity.with_debug_label(label);
        self
    }

    pub fn value(self) -> Slot<T> {
        self.value
    }

    #[track_caller]
    pub fn try_get(self) -> Option<CompRef<T>> {
        let token = MainThreadToken::acquire_fmt("fetch entity component data");

        self.is_alive_internal(token)
            .then(|| self.value.borrow_or_none(token))
            .flatten()
            .map(|r| CompRef::new(self, r))
    }

    #[track_caller]
    pub fn try_get_mut(self) -> Option<CompMut<T>> {
        let token = MainThreadToken::acquire_fmt("fetch entity component data");

        self.is_alive_internal(token)
            .then(|| self.value.borrow_mut_or_none(token))
            .flatten()
            .map(|r| CompMut::new(self, r))
    }

    #[track_caller]
    pub fn get(self) -> CompRef<T> {
        let token = MainThreadToken::acquire_fmt("fetch entity component data");
        assert!(
            self.is_alive_internal(token),
            "attempted to get the value of a dead `Obj<{}>` corresponding to {:?}",
            type_name::<T>(),
            self.entity(),
        );
        CompRef::new(self, self.value.borrow(token))
    }

    #[track_caller]
    pub fn get_mut(self) -> CompMut<T> {
        let token = MainThreadToken::acquire_fmt("fetch entity component data");
        assert!(
            self.is_alive_internal(token),
            "attempted to get the value of a dead `Obj<{}>` corresponding to {:?}",
            type_name::<T>(),
            self.entity(),
        );
        CompMut::new(self, self.value.borrow_mut(token))
    }

    pub fn destroy(self) {
        self.entity.destroy()
    }
}

impl<T: 'static> Borrow<Entity> for Obj<T> {
    fn borrow(&self) -> &Entity {
        &self.entity
    }
}

// === OwnedObj === //

#[derive(Debug)]
#[derive_where(Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct OwnedObj<T: 'static> {
    obj: Obj<T>,
}

impl<T: 'static> OwnedObj<T> {
    // === Lifecycle === //

    pub fn from_raw_parts(entity: OwnedEntity, value: Slot<T>) -> Self {
        Self {
            obj: Obj::from_raw_parts(entity.unmanage(), value),
        }
    }

    pub fn insert(entity: OwnedEntity, value: T) -> Self {
        let obj = Self::from_raw_obj(Obj::insert(entity.entity(), value));
        // N.B. we unmanage the entity here to ensure that it gets dropped if the above call panics.
        entity.unmanage();
        obj
    }

    pub fn wrap(entity: OwnedEntity) -> Self {
        let obj = Self::from_raw_obj(Obj::wrap(entity.entity()));
        // N.B. we unmanage the entity here to ensure that it gets dropped if the above call panics.
        entity.unmanage();
        obj
    }

    pub fn new(value: T) -> Self {
        Self::from_raw_obj(Obj::new_unmanaged(value))
    }

    pub fn from_raw_obj(obj: Obj<T>) -> Self {
        Self { obj }
    }

    pub fn obj(&self) -> Obj<T> {
        self.obj
    }

    pub fn entity(&self) -> Entity {
        self.obj.entity()
    }

    pub fn owned_entity(self) -> OwnedEntity {
        OwnedEntity::from_raw_entity(self.unmanage().entity())
    }

    pub fn unmanage(self) -> Obj<T> {
        let obj = self.obj;
        mem::forget(self);
        obj
    }

    pub fn split_guard(self) -> (Self, Obj<T>) {
        let obj = self.obj();
        (self, obj)
    }

    // === Forwards === //

    pub fn with_debug_label<L: AsDebugLabel>(self, label: L) -> Self {
        self.obj.with_debug_label(label);
        self
    }

    pub fn value(&self) -> Slot<T> {
        self.obj.value()
    }

    pub fn try_get(&self) -> Option<CompRef<T>> {
        self.obj.try_get()
    }

    pub fn try_get_mut(&self) -> Option<CompMut<T>> {
        self.obj.try_get_mut()
    }

    pub fn get(&self) -> CompRef<T> {
        self.obj.get()
    }

    pub fn get_mut(&self) -> CompMut<T> {
        self.obj.get_mut()
    }

    pub fn is_alive(&self) -> bool {
        self.obj.is_alive()
    }

    pub fn destroy(self) {
        drop(self);
    }
}

impl<T: 'static> Drop for OwnedObj<T> {
    fn drop(&mut self) {
        self.obj.destroy();
    }
}

impl<T: 'static + Default> Default for OwnedObj<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: 'static> Borrow<Obj<T>> for OwnedObj<T> {
    fn borrow(&self) -> &Obj<T> {
        &self.obj
    }
}

impl<T: 'static> Borrow<Entity> for OwnedObj<T> {
    fn borrow(&self) -> &Entity {
        &self.obj.entity
    }
}
