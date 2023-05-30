// === Obj === //

use std::{any::type_name, borrow::Borrow, fmt, hash, mem};

use crate::{
    core::{heap::Slot, token::MainThreadToken},
    debug::AsDebugLabel,
    entity::{CompMut, CompRef, Entity, OwnedEntity},
};

pub struct Obj<T: 'static> {
    entity: Entity,
    value: Slot<T>,
}

impl<T: 'static> Obj<T> {
    pub fn from_raw_parts(entity: Entity, value: Slot<T>) -> Self {
        Self { entity, value }
    }

    pub fn insert(entity: Entity, value: T) -> Self {
        entity.insert_and_return_slot(value).0
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

    pub fn with_debug_label<L: AsDebugLabel>(self, label: L) -> Self {
        self.entity.with_debug_label(label);
        self
    }

    pub fn value(self) -> Slot<T> {
        self.value
    }

    pub fn try_get(self) -> Option<CompRef<T>> {
        self.value.borrow_or_none(MainThreadToken::acquire())
    }

    pub fn try_get_mut(self) -> Option<CompMut<T>> {
        self.value.borrow_mut_or_none(MainThreadToken::acquire())
    }

    pub fn get(self) -> CompRef<T> {
        self.try_get().unwrap_or_else(|| {
            panic!(
                "attempted to get the value of a dead `Obj<{}>` corresponding to {:?}",
                type_name::<T>(),
                self.entity()
            )
        })
    }

    pub fn get_mut(self) -> CompMut<T> {
        self.try_get_mut().unwrap_or_else(|| {
            panic!(
                "attempted to get the value of a dead `Obj<{}>` corresponding to {:?}",
                type_name::<T>(),
                self.entity()
            )
        })
    }

    pub fn is_alive(self) -> bool {
        self.entity.is_alive()
    }

    pub fn destroy(self) {
        self.entity.destroy()
    }
}

impl<T: 'static + fmt::Debug> fmt::Debug for Obj<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Obj")
            .field("entity", &self.entity)
            .field("value", &self.value)
            .finish()
    }
}

impl<T: 'static> Copy for Obj<T> {}

impl<T: 'static> Clone for Obj<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static> hash::Hash for Obj<T> {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.entity.hash(state);
    }
}

impl<T: 'static> Eq for Obj<T> {}

impl<T: 'static> PartialEq for Obj<T> {
    fn eq(&self, other: &Self) -> bool {
        self.entity == other.entity
    }
}

impl<T: 'static> Ord for Obj<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.entity.cmp(&other.entity)
    }
}

impl<T: 'static> PartialOrd for Obj<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T: 'static> Borrow<Entity> for Obj<T> {
    fn borrow(&self) -> &Entity {
        &self.entity
    }
}

// === OwnedObj === //

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

impl<T: 'static + fmt::Debug> fmt::Debug for OwnedObj<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OwnedObj").field("obj", &self.obj).finish()
    }
}

impl<T: 'static + Default> Default for OwnedObj<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: 'static> hash::Hash for OwnedObj<T> {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.obj.hash(state);
    }
}

impl<T: 'static> Eq for OwnedObj<T> {}

impl<T: 'static> PartialEq for OwnedObj<T> {
    fn eq(&self, other: &Self) -> bool {
        self.obj == other.obj
    }
}

impl<T: 'static> Ord for OwnedObj<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.obj.cmp(&other.obj)
    }
}

impl<T: 'static> PartialOrd for OwnedObj<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.obj.partial_cmp(&other.obj)
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
