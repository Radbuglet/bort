use std::{
    any::{type_name, TypeId},
    borrow, fmt, mem,
    num::NonZeroU64,
    ops::{Deref, DerefMut},
};

use derive_where::derive_where;

use crate::{
    core::{
        cell::{OptRef, OptRefMut},
        heap::Slot,
        token::MainThreadToken,
    },
    database::{DbRoot, DbStorage, EntityDeadError, InertEntity},
    debug::AsDebugLabel,
    obj::{Obj, OwnedObj},
    query::{RawTag, Tag},
    util::misc::RawFmt,
};

// === Storage === //

pub fn storage<T: 'static>() -> Storage<T> {
    let token = MainThreadToken::acquire_fmt("fetch entity component data");

    Storage::from_database(token, DbRoot::get(token).get_storage::<T>())
}

#[derive_where(Debug, Copy, Clone)]
pub struct Storage<T: 'static> {
    token: MainThreadToken,
    inner: &'static DbStorage<T>,
}

impl<T: 'static> Storage<T> {
    pub(crate) fn from_database(
        token: &'static MainThreadToken,
        inner: &'static DbStorage<T>,
    ) -> Self {
        Self {
            token: *token,
            inner,
        }
    }

    pub fn acquire() -> Storage<T> {
        storage::<T>()
    }

    // === Management === //

    pub fn insert_with_obj(&self, entity: Entity, value: T) -> (Option<T>, Obj<T>) {
        match DbRoot::get(self.token.make_ref()).insert_component(
            self.token.make_ref(),
            &mut self.inner.borrow_mut(self.token.make_ref()),
            entity.inert,
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
        match DbRoot::get(self.token.make_ref()).remove_component(
            self.token.make_ref(),
            &mut self.inner.borrow_mut(self.token.make_ref()),
            entity.inert,
        ) {
            Ok(removed) => removed,
            Err(EntityDeadError) => {
                panic!("Attempted to remove component from dead entity {entity:?}")
            }
        }
    }

    // === Getters === //

    pub fn try_get_slot(&self, entity: Entity) -> Option<Slot<T>> {
        DbRoot::get_component(&self.inner.borrow(self.token.make_ref()), entity.inert)
    }

    pub fn get_slot(&self, entity: Entity) -> Slot<T> {
        let slot = self.try_get_slot(entity).unwrap_or_else(|| {
            panic!(
                "failed to find component of type {} for {:?}",
                type_name::<T>(),
                entity,
            )
        });
        debug_assert_eq!(slot.owner(self.token.make_ref()), Some(entity));

        slot
    }

    #[track_caller]
    pub fn try_get(&self, entity: Entity) -> Option<CompRef<'static, T>> {
        self.try_get_slot(entity).map(|slot| {
            CompRef::new(
                Obj::from_raw_parts(entity, slot),
                slot.borrow(self.token.make_ref()),
            )
        })
    }

    #[track_caller]
    pub fn try_get_mut(&self, entity: Entity) -> Option<CompMut<'static, T>> {
        self.try_get_slot(entity).map(|slot| {
            CompMut::new(
                Obj::from_raw_parts(entity, slot),
                slot.borrow_mut(self.token.make_ref()),
            )
        })
    }

    #[track_caller]
    pub fn get(&self, entity: Entity) -> CompRef<'static, T> {
        let slot = self.get_slot(entity);

        CompRef::new(
            Obj::from_raw_parts(entity, slot),
            slot.borrow(self.token.make_ref()),
        )
    }

    #[track_caller]
    pub fn get_mut(&self, entity: Entity) -> CompMut<'static, T> {
        let slot = self.get_slot(entity);

        CompMut::new(
            Obj::from_raw_parts(entity, slot),
            slot.borrow_mut(self.token.make_ref()),
        )
    }

    pub fn has(&self, entity: Entity) -> bool {
        self.try_get_slot(entity).is_some()
    }
}

// === Entity === //

#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct Entity {
    pub(crate) inert: InertEntity,
}

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

    pub fn with_many<F>(self, f: F) -> Self
    where
        F: FnOnce(Entity),
    {
        f(self);
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

    #[track_caller]
    pub fn try_get_slot<T: 'static>(self) -> Option<Slot<T>> {
        storage::<T>().try_get_slot(self)
    }

    #[track_caller]
    pub fn try_get<T: 'static>(self) -> Option<CompRef<'static, T>> {
        storage::<T>().try_get(self)
    }

    #[track_caller]
    pub fn try_get_mut<T: 'static>(self) -> Option<CompMut<'static, T>> {
        storage::<T>().try_get_mut(self)
    }

    pub fn get_slot<T: 'static>(self) -> Slot<T> {
        storage::<T>().get_slot(self)
    }

    #[track_caller]
    pub fn get<T: 'static>(self) -> CompRef<'static, T> {
        storage::<T>().get(self)
    }

    #[track_caller]
    pub fn get_mut<T: 'static>(self) -> CompMut<'static, T> {
        storage::<T>().get_mut(self)
    }

    pub fn has<T: 'static>(self) -> bool {
        storage::<T>().has(self)
    }

    pub fn has_dyn(self, ty: TypeId) -> bool {
        let token = MainThreadToken::acquire_fmt("check the component list of an entity");
        DbRoot::get(token).entity_has_component_dyn(token, self.inert, ty)
    }

    pub fn obj<T: 'static>(self) -> Obj<T> {
        Obj::wrap(self)
    }

    pub fn tag(self, tag: impl Into<RawTag>) {
        let tag = tag.into().0;

        match DbRoot::get(MainThreadToken::acquire_fmt("tag an entity")).tag_entity(self.inert, tag)
        {
            Ok(()) => { /* no-op */ }
            Err(EntityDeadError) => panic!("Attempted to add tag to dead entity {self:?}"),
        }
    }

    pub fn untag(self, tag: impl Into<RawTag>) {
        let tag = tag.into().0;
        match DbRoot::get(MainThreadToken::acquire_fmt("untag an entity"))
            .untag_entity(self.inert, tag)
        {
            Ok(()) => {}
            Err(EntityDeadError) => panic!("Attempted to remove tag from dead entity {self:?}"),
        }
    }

    pub fn with_tag(self, tag: impl Into<RawTag>) -> Self {
        self.tag(tag);
        self
    }

    pub fn with_tagged<T: 'static>(self, tag: impl Into<Tag<T>>, comp: T) -> Self {
        self.insert(comp);
        self.tag(tag.into());
        self
    }

    pub fn is_tagged(self, tag: impl Into<RawTag>) -> bool {
        let tag = tag.into().0;
        let is_tagged = DbRoot::get(MainThreadToken::acquire_fmt("query entity tags"))
            .is_entity_tagged(self.inert, tag);

        match is_tagged {
            Ok(result) => result,
            Err(EntityDeadError) => panic!("Attempted to query tags of dead entity {self:?}"),
        }
    }

    pub fn is_alive(self) -> bool {
        DbRoot::get(MainThreadToken::acquire_fmt(
            "check the liveness state of an entity",
        ))
        .is_entity_alive(self.inert)
    }

    pub fn destroy(self) {
        let token = MainThreadToken::acquire_fmt("destroy entity");
        let components = DbRoot::get(token)
            .despawn_entity_without_comp_cleanup(self.inert)
            .unwrap_or_else(|_| panic!("Attempted to destroy already dead entity {self:?}"));

        components.run_dtors(token, self.inert);
    }
}

impl fmt::Debug for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(token) = MainThreadToken::try_acquire() {
            DbRoot::get(token).debug_format_entity(f, token, self.inert)
        } else {
            #[derive(Debug)]
            struct Id(NonZeroU64);

            f.debug_tuple("Entity")
                .field(&RawFmt("<cross-thread>"))
                .field(&Id(self.inert.id()))
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

    pub fn with_many<F>(self, f: F) -> Self
    where
        F: FnOnce(Entity),
    {
        f(self.entity);
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

    #[track_caller]
    pub fn try_get<T: 'static>(&self) -> Option<CompRef<'static, T>> {
        self.entity.try_get()
    }

    #[track_caller]
    pub fn try_get_mut<T: 'static>(&self) -> Option<CompMut<'static, T>> {
        self.entity.try_get_mut()
    }

    pub fn get_slot<T: 'static>(&self) -> Slot<T> {
        self.entity.get_slot()
    }

    #[track_caller]
    pub fn get<T: 'static>(&self) -> CompRef<'static, T> {
        self.entity.get()
    }

    #[track_caller]
    pub fn get_mut<T: 'static>(&self) -> CompMut<'static, T> {
        self.entity.get_mut()
    }

    pub fn has<T: 'static>(&self) -> bool {
        self.entity.has::<T>()
    }

    pub fn has_dyn(self, ty: TypeId) -> bool {
        self.entity.has_dyn(ty)
    }

    pub fn obj<T: 'static>(&self) -> Obj<T> {
        self.entity.obj()
    }

    pub fn into_obj<T: 'static>(self) -> OwnedObj<T> {
        OwnedObj::wrap(self)
    }

    pub fn tag(&self, tag: impl Into<RawTag>) {
        self.entity.tag(tag)
    }

    pub fn untag(&self, tag: impl Into<RawTag>) {
        self.entity.untag(tag)
    }

    pub fn with_tag(self, tag: impl Into<RawTag>) -> Self {
        self.entity.tag(tag);
        self
    }

    pub fn with_tagged<T: 'static>(self, tag: impl Into<Tag<T>>, comp: T) -> Self {
        self.insert(comp);
        self.tag(tag.into());
        self
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

// === `CompRef` and `CompMut` === //

pub struct CompRef<'b, T: ?Sized, O: Copy = Obj<T>> {
    owner: O,
    value: OptRef<'b, T>,
}

impl<'b, T: ?Sized, O: Copy> CompRef<'b, T, O> {
    pub fn new(owner: O, value: OptRef<'b, T>) -> Self {
        Self { owner, value }
    }

    pub fn into_opt_ref(orig: Self) -> OptRef<'b, T> {
        orig.value
    }

    pub fn map_owner<P: Copy>(orig: Self, f: impl FnOnce(O) -> P) -> CompRef<'b, T, P> {
        CompRef {
            owner: f(orig.owner),
            value: orig.value,
        }
    }

    pub fn erase_owner(orig: Self) -> CompRef<'b, T, Entity>
    where
        O: Into<Entity>,
    {
        Self::map_owner(orig, |obj| obj.into())
    }

    pub fn owner(orig: &Self) -> O {
        orig.owner
    }

    #[allow(clippy::should_implement_trait)] // (follows standard library conventions)
    pub fn clone(orig: &Self) -> Self {
        Self {
            owner: orig.owner,
            value: OptRef::clone(&orig.value),
        }
    }

    pub fn map<U: ?Sized, F>(orig: CompRef<'b, T, O>, f: F) -> CompRef<'b, U, O>
    where
        F: FnOnce(&T) -> &U,
    {
        CompRef {
            owner: orig.owner,
            value: OptRef::map(orig.value, f),
        }
    }

    pub fn filter_map<U: ?Sized, F>(
        orig: CompRef<'b, T, O>,
        f: F,
    ) -> Result<CompRef<'b, U, O>, Self>
    where
        F: FnOnce(&T) -> Option<&U>,
    {
        let owner = orig.owner;

        match OptRef::filter_map(orig.value, f) {
            Ok(value) => Ok(CompRef { owner, value }),
            Err(value) => Err(CompRef { owner, value }),
        }
    }

    pub fn map_split<U: ?Sized, V: ?Sized, F>(
        orig: CompRef<'b, T, O>,
        f: F,
    ) -> (CompRef<'b, U, O>, CompRef<'b, V, O>)
    where
        F: FnOnce(&T) -> (&U, &V),
    {
        let owner = orig.owner;
        let (left, right) = OptRef::map_split(orig.value, f);

        (
            CompRef { owner, value: left },
            CompRef {
                owner,
                value: right,
            },
        )
    }

    pub fn leak(orig: CompRef<'b, T, O>) -> &'b T {
        OptRef::leak(orig.value)
    }
}

impl<T: ?Sized, O: Copy> Deref for CompRef<'_, T, O> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<T: ?Sized + fmt::Debug, O: Copy> fmt::Debug for CompRef<'_, T, O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.value.fmt(f)
    }
}

impl<T: ?Sized + fmt::Display, O: Copy> fmt::Display for CompRef<'_, T, O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.value.fmt(f)
    }
}

pub struct CompMut<'b, T: ?Sized, O: Copy = Obj<T>> {
    owner: O,
    value: OptRefMut<'b, T>,
}

impl<'b, T: ?Sized, O: Copy> CompMut<'b, T, O> {
    pub fn new(owner: O, value: OptRefMut<'b, T>) -> Self {
        Self { owner, value }
    }

    pub fn into_opt_ref_mut(orig: Self) -> OptRefMut<'b, T> {
        orig.value
    }

    pub fn map_owner<P: Copy>(orig: Self, f: impl FnOnce(O) -> P) -> CompMut<'b, T, P> {
        CompMut {
            owner: f(orig.owner),
            value: orig.value,
        }
    }

    pub fn owner(orig: &Self) -> O {
        orig.owner
    }

    pub fn map<U: ?Sized, F>(orig: CompMut<'b, T, O>, f: F) -> CompMut<'b, U, O>
    where
        F: FnOnce(&mut T) -> &mut U,
    {
        CompMut {
            owner: orig.owner,
            value: OptRefMut::map(orig.value, f),
        }
    }

    pub fn filter_map<U: ?Sized, F>(
        orig: CompMut<'b, T, O>,
        f: F,
    ) -> Result<CompMut<'b, U, O>, Self>
    where
        F: FnOnce(&mut T) -> Option<&mut U>,
    {
        let entity = orig.owner;

        match OptRefMut::filter_map(orig.value, f) {
            Ok(value) => Ok(CompMut {
                owner: entity,
                value,
            }),
            Err(value) => Err(Self {
                owner: entity,
                value,
            }),
        }
    }

    pub fn map_split<U: ?Sized, V: ?Sized, F>(
        orig: CompMut<'b, T, O>,
        f: F,
    ) -> (CompMut<'b, U, O>, CompMut<'b, V, O>)
    where
        F: FnOnce(&mut T) -> (&mut U, &mut V),
    {
        let entity = orig.owner;
        let (left, right) = OptRefMut::map_split(orig.value, f);

        (
            CompMut {
                owner: entity,
                value: left,
            },
            CompMut {
                owner: entity,
                value: right,
            },
        )
    }

    pub fn leak(orig: CompMut<'b, T, O>) -> &'b mut T {
        OptRefMut::leak(orig.value)
    }
}

impl<T: ?Sized, O: Copy> Deref for CompMut<'_, T, O> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<T: ?Sized, O: Copy> DerefMut for CompMut<'_, T, O> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

impl<T: ?Sized + fmt::Debug, O: Copy> fmt::Debug for CompMut<'_, T, O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T: ?Sized + fmt::Display, O: Copy> fmt::Display for CompMut<'_, T, O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}
