use std::{
    any::{type_name, TypeId},
    borrow, fmt,
    marker::PhantomData,
    mem,
    num::NonZeroU64,
};

use derive_where::derive_where;

use crate::{
    core::{
        cell::{OptRef, OptRefMut},
        heap::Slot,
        token::MainThreadToken,
    },
    database::{DbRoot, DbStorage, EntityDeadError, InertEntity, InertTag},
    debug::AsDebugLabel,
    obj::{Obj, OwnedObj},
    util::misc::RawFmt,
};

// === Storage === //

// Aliases
pub type CompRef<T> = OptRef<'static, T>;

pub type CompMut<T> = OptRefMut<'static, T>;

// Storage API
pub fn storage<T: 'static>() -> Storage<T> {
    let token = MainThreadToken::acquire_fmt("fetch entity component data");

    Storage {
        inner: DbRoot::get(token).get_storage::<T>(),
        token,
    }
}

// #[derive(Debug)]  TODO
#[derive_where(Copy, Clone)]
pub struct Storage<T: 'static> {
    inner: &'static DbStorage<T>,
    token: &'static MainThreadToken,
}

impl<T: 'static> Storage<T> {
    pub fn acquire() -> Storage<T> {
        storage::<T>()
    }

    // === Management === //

    pub fn insert_with_obj(&self, entity: Entity, value: T) -> (Option<T>, Obj<T>) {
        match DbRoot::get(self.token).insert_component(
            self.token,
            &mut self.inner.borrow_mut(self.token),
            entity.0,
            value,
        ) {
            Ok((replaced, slot)) => (replaced, Obj::from_raw_parts(entity, slot)),
            Err(EntityDeadError) => panic!("Attempted to add component to dead entity {entity:?}"),
        }
    }

    pub fn insert(&self, entity: Entity, value: T) -> Option<T> {
        self.insert_with_obj(entity, value).0
    }

    pub fn remove(&self, entity: Entity) -> Option<T> {
        match DbRoot::get(self.token).remove_component(
            self.token,
            &mut self.inner.borrow_mut(self.token),
            entity.0,
        ) {
            Ok(removed) => removed,
            Err(EntityDeadError) => {
                panic!("Attempted to remove component from dead entity {entity:?}")
            }
        }
    }

    // === Getters === //

    pub fn try_get_slot(&self, entity: Entity) -> Option<Slot<T>> {
        DbRoot::get_component(&self.inner.borrow(self.token), entity.0)
    }

    pub fn get_slot(&self, entity: Entity) -> Slot<T> {
        let slot = self.try_get_slot(entity).unwrap_or_else(|| {
            panic!(
                "failed to find component of type {} for {:?}",
                type_name::<T>(),
                entity,
            )
        });
        debug_assert_eq!(slot.owner(self.token), Some(entity));

        slot
    }

    pub fn try_get(&self, entity: Entity) -> Option<CompRef<T>> {
        self.try_get_slot(entity)
            .map(|slot| slot.borrow(self.token))
    }

    pub fn try_get_mut(&self, entity: Entity) -> Option<CompMut<T>> {
        self.try_get_slot(entity)
            .map(|slot| slot.borrow_mut(self.token))
    }

    pub fn get(&self, entity: Entity) -> CompRef<T> {
        self.get_slot(entity).borrow(self.token)
    }

    pub fn get_mut(&self, entity: Entity) -> CompMut<T> {
        self.get_slot(entity).borrow_mut(self.token)
    }

    pub fn has(&self, entity: Entity) -> bool {
        self.try_get_slot(entity).is_some()
    }
}

// === Entity === //

#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct Entity(pub(crate) InertEntity);

impl Entity {
    pub fn new_unmanaged() -> Self {
        DbRoot::get(MainThreadToken::acquire_fmt("fetch entity component data"))
            .spawn_entity()
            .into_dangerous_entity()
    }

    pub fn with<T: 'static>(self, comp: T) -> Self {
        self.insert(comp);
        self
    }

    pub fn with_self_referential<T: 'static>(self, func: impl FnOnce(Entity) -> T) -> Self {
        self.insert(func(self));
        self
    }

    pub fn with_debug_label<L: AsDebugLabel>(self, label: L) -> Self {
        #[cfg(debug_assertions)]
        self.with(crate::debug::DebugLabel::from(label));
        #[cfg(not(debug_assertions))]
        let _ = label;
        self
    }

    pub fn insert_with_obj<T: 'static>(self, comp: T) -> (Option<T>, Obj<T>) {
        storage::<T>().insert_with_obj(self, comp)
    }

    pub fn insert<T: 'static>(self, comp: T) -> Option<T> {
        storage::<T>().insert(self, comp)
    }

    pub fn remove<T: 'static>(self) -> Option<T> {
        storage::<T>().remove(self)
    }

    pub fn try_get_slot<T: 'static>(self) -> Option<Slot<T>> {
        storage::<T>().try_get_slot(self)
    }

    pub fn try_get<T: 'static>(self) -> Option<CompRef<T>> {
        storage::<T>().try_get(self)
    }

    pub fn try_get_mut<T: 'static>(self) -> Option<CompMut<T>> {
        storage::<T>().try_get_mut(self)
    }

    pub fn get_slot<T: 'static>(self) -> Slot<T> {
        storage::<T>().get_slot(self)
    }

    pub fn get<T: 'static>(self) -> CompRef<T> {
        storage::<T>().get(self)
    }

    pub fn get_mut<T: 'static>(self) -> CompMut<T> {
        storage::<T>().get_mut(self)
    }

    pub fn has<T: 'static>(self) -> bool {
        storage::<T>().has(self)
    }

    pub fn obj<T: 'static>(self) -> Obj<T> {
        Obj::wrap(self)
    }

    pub fn tag(self, tag: impl Into<RawTag>) {
        match DbRoot::get(MainThreadToken::acquire_fmt("tag an entity"))
            .tag_entity(self.0, tag.into().0)
        {
            Ok(()) => { /* no-op */ }
            Err(EntityDeadError) => panic!("Attempted to add tag to dead entity {self:?}"),
        }
    }

    pub fn untag(self, tag: impl Into<RawTag>) {
        match DbRoot::get(MainThreadToken::acquire_fmt("untag an entity"))
            .untag_entity(self.0, tag.into().0)
        {
            Ok(()) => {}
            Err(EntityDeadError) => panic!("Attempted to remove tag from dead entity {self:?}"),
        }
    }

    pub fn is_tagged(self, tag: impl Into<RawTag>) -> bool {
        match DbRoot::get(MainThreadToken::acquire_fmt("query entity tags"))
            .is_entity_tagged(self.0, tag.into().0)
        {
            Ok(result) => result,
            Err(EntityDeadError) => panic!("Attempted to query tags of dead entity {self:?}"),
        }
    }

    pub fn is_alive(self) -> bool {
        DbRoot::get(MainThreadToken::acquire_fmt(
            "check the liveness state of an entity",
        ))
        .is_entity_alive(self.0)
    }

    pub fn destroy(self) {
        let token = MainThreadToken::acquire_fmt("destroy entity");
        match DbRoot::get(token).despawn_entity(token, self.0) {
            Ok(()) => {}
            Err(_) => panic!("Attempted to destroy already dead entity {self:?}"),
        }
    }
}

impl fmt::Debug for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(token) = MainThreadToken::try_acquire() {
            DbRoot::get(token).debug_format_entity(f, token, self.0)
        } else {
            #[derive(Debug)]
            struct Id(NonZeroU64);

            f.debug_tuple("Entity")
                .field(&RawFmt("<cross-thread>"))
                .field(&Id(self.0.id()))
                .finish()
        }
    }
}

// === OwnedEntity === //

#[derive(Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct OwnedEntity {
    entity: Entity,
}

impl OwnedEntity {
    // === Lifecycle === //

    pub fn new() -> Self {
        Self::from_raw_entity(Entity::new_unmanaged())
    }

    pub fn from_raw_entity(entity: Entity) -> Self {
        Self { entity }
    }

    pub fn entity(&self) -> Entity {
        self.entity
    }

    pub fn unmanage(self) -> Entity {
        let entity = self.entity;
        mem::forget(self);

        entity
    }

    pub fn split_guard(self) -> (Self, Entity) {
        let entity = self.entity();
        (self, entity)
    }

    // === Forwards === //

    pub fn with<T: 'static>(self, comp: T) -> Self {
        self.entity.insert(comp);
        self
    }

    pub fn with_self_referential<T: 'static>(self, func: impl FnOnce(Entity) -> T) -> Self {
        self.entity.insert(func(self.entity()));
        self
    }

    pub fn with_debug_label<L: AsDebugLabel>(self, label: L) -> Self {
        self.entity.with_debug_label(label);
        self
    }

    pub fn insert_with_obj<T: 'static>(&self, comp: T) -> (Option<T>, Obj<T>) {
        self.entity.insert_with_obj(comp)
    }

    pub fn insert<T: 'static>(&self, comp: T) -> Option<T> {
        self.entity.insert(comp)
    }

    pub fn remove<T: 'static>(&self) -> Option<T> {
        self.entity.remove()
    }

    pub fn try_get_slot<T: 'static>(&self) -> Option<Slot<T>> {
        self.entity.try_get_slot()
    }

    pub fn try_get<T: 'static>(&self) -> Option<CompRef<T>> {
        self.entity.try_get()
    }

    pub fn try_get_mut<T: 'static>(&self) -> Option<CompMut<T>> {
        self.entity.try_get_mut()
    }

    pub fn get_slot<T: 'static>(&self) -> Slot<T> {
        self.entity.get_slot()
    }

    pub fn get<T: 'static>(&self) -> CompRef<T> {
        self.entity.get()
    }

    pub fn get_mut<T: 'static>(&self) -> CompMut<T> {
        self.entity.get_mut()
    }

    pub fn has<T: 'static>(&self) -> bool {
        self.entity.has::<T>()
    }

    pub fn obj<T: 'static>(&self) -> Obj<T> {
        self.entity.obj()
    }

    pub fn into_obj<T: 'static>(self) -> OwnedObj<T> {
        OwnedObj::wrap(self)
    }

    pub fn tag(&self, tag: impl Into<RawTag>) {
        self.entity().tag(tag)
    }

    pub fn untag(&self, tag: impl Into<RawTag>) {
        self.entity().untag(tag)
    }

    pub fn is_tagged(&self, tag: impl Into<RawTag>) -> bool {
        self.entity().is_tagged(tag)
    }

    pub fn is_alive(&self) -> bool {
        self.entity.is_alive()
    }

    pub fn destroy(self) {
        drop(self);
    }
}

impl Default for OwnedEntity {
    fn default() -> Self {
        Self::new()
    }
}

impl borrow::Borrow<Entity> for OwnedEntity {
    fn borrow(&self) -> &Entity {
        &self.entity
    }
}

impl Drop for OwnedEntity {
    fn drop(&mut self) {
        self.entity.destroy();
    }
}

// === Tag === //

pub struct VirtualTagMarker {
    _never: (),
}

pub type VirtualTag = Tag<VirtualTagMarker>;

#[derive_where(Debug, Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct Tag<T: 'static> {
    _ty: PhantomData<fn() -> T>,
    raw: RawTag,
}

impl<T> Tag<T> {
    pub fn new() -> Self {
        Self {
            _ty: PhantomData,
            raw: RawTag::new(TypeId::of::<T>()),
        }
    }

    pub fn raw(self) -> RawTag {
        self.raw
    }
}

impl<T> Into<RawTag> for Tag<T> {
    fn into(self) -> RawTag {
        self.raw
    }
}

#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct RawTag(pub(crate) InertTag);

impl RawTag {
    pub fn new(ty: TypeId) -> Self {
        DbRoot::get(MainThreadToken::acquire_fmt("create tag"))
            .spawn_tag(ty)
            .into_dangerous_tag()
    }
}

impl fmt::Debug for RawTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawTag")
            .field("id", &self.0.id())
            .field("ty", &self.0.ty())
            .finish()
    }
}
