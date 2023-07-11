use std::{
    any::{type_name, Any},
    cell::RefCell,
    fmt, hash, mem,
    ops::ControlFlow,
};

use derive_where::derive_where;

use crate::{
    entity::{Entity, OwnedEntity},
    query::RawTag,
    util::{
        hash_map::{ConstSafeBuildHasherDefault, FxHashMap},
        iter::hash_one,
        misc::NamedTypeId,
    },
};

// === EventTarget === //

pub trait EventTarget<E, C = ()> {
    fn fire_cx(&mut self, target: Entity, event: E, context: C);

    fn fire_cx_owned(&mut self, target: OwnedEntity, event: E, context: C);

    fn fire(&mut self, target: Entity, event: E)
    where
        C: Default,
    {
        self.fire_cx(target, event, C::default());
    }

    fn fire_owned(&mut self, target: OwnedEntity, event: E)
    where
        C: Default,
    {
        self.fire_cx_owned(target, event, C::default());
    }
}

impl<E, C, F> EventTarget<E, C> for F
where
    F: FnMut(Entity, E, C),
{
    fn fire_cx(&mut self, target: Entity, event: E, context: C) {
        self(target, event, context);
    }

    fn fire_cx_owned(&mut self, target: OwnedEntity, event: E, context: C) {
        self(target.entity(), event, context);
    }
}

// === QueryableEvent === //

#[derive_where(Default)]
pub struct QueryVersionMap<V> {
    versions: FxHashMap<QueryKey, V>,
}

struct QueryKey {
    hash: u64,
    value: Box<dyn Any + Send + Sync>,
}

impl<V> fmt::Debug for QueryVersionMap<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QueryVersionMap").finish_non_exhaustive()
    }
}

impl<V> QueryVersionMap<V> {
    pub const fn new() -> Self {
        Self {
            versions: FxHashMap::with_hasher(ConstSafeBuildHasherDefault::new()),
        }
    }

    pub fn clear(&mut self) {
        self.versions.clear();
    }

    pub fn entry<K>(&mut self, key: K, version_ctor: impl FnOnce() -> V) -> &mut V
    where
        K: 'static + Send + Sync + hash::Hash + PartialEq,
    {
        let hash = hash_one(self.versions.hasher(), &key);
        let entry = self.versions.raw_entry_mut().from_hash(hash, |entry| {
            hash == entry.hash
                && entry
                    .value
                    .downcast_ref::<K>()
                    .is_some_and(|candidate| &*candidate == &key)
        });

        match entry {
            hashbrown::hash_map::RawEntryMut::Occupied(occupied) => occupied.into_mut(),
            hashbrown::hash_map::RawEntryMut::Vacant(vacant) => {
                vacant
                    .insert_with_hasher(
                        hash,
                        QueryKey {
                            hash,
                            value: Box::new(key),
                        },
                        version_ctor(),
                        |entry| entry.hash,
                    )
                    .1
            }
        }
    }
}

pub trait QueryableEventList {
    type Event;

    fn query_raw<K, F>(&self, version_id: K, tags: &[RawTag], handler: F)
    where
        K: 'static + Send + Sync + hash::Hash + PartialEq,
        F: FnMut(Entity, &Self::Event) -> ControlFlow<()>;
}

pub trait ProcessableEventList: QueryableEventList {
    fn is_empty(&self) -> bool;

    fn clear(&mut self);
}

// === BehaviorRegistry === //

pub trait EventHasBehavior: Sized + 'static {
    type Context<'a>;
    type EventList: 'static + QueryableEventList<Event = Self>;
}

#[derive(Debug, Default)]
pub struct BehaviorRegistry {
    handlers: FxHashMap<NamedTypeId, Box<dyn Any + Send + Sync>>,
}

#[rustfmt::skip]
type BehaviorRegistryHandler<E> = Vec<Box<dyn Send + Sync + Fn(
	&BehaviorRegistry,
	&mut <E as EventHasBehavior>::EventList,
	&mut <E as EventHasBehavior>::Context<'_>,
)>>;

impl BehaviorRegistry {
    pub const fn new() -> Self {
        Self {
            handlers: FxHashMap::with_hasher(ConstSafeBuildHasherDefault::new()),
        }
    }

    pub fn register<E: EventHasBehavior>(
        &mut self,
        handler: impl 'static + Fn(&Self, &mut E::EventList, &mut E::Context<'_>) + Send + Sync,
    ) {
        self.handlers
            .entry(NamedTypeId::of::<E>())
            .or_insert_with(|| Box::new(BehaviorRegistryHandler::<E>::new()))
            .downcast_mut::<BehaviorRegistryHandler<E>>()
            .unwrap()
            .push(Box::new(handler));
    }

    pub fn with<E: EventHasBehavior>(
        mut self,
        handler: impl 'static + Fn(&Self, &mut E::EventList, &mut E::Context<'_>) + Send + Sync,
    ) -> Self {
        self.register(handler);
        self
    }

    pub fn process_cx<EL>(
        &self,
        events: &mut EL,
        context: &mut <EL::Event as EventHasBehavior>::Context<'_>,
    ) where
        EL: 'static + ProcessableEventList,
        EL::Event: EventHasBehavior<EventList = EL>,
    {
        let Some(handlers) = self.handlers.get(&NamedTypeId::of::<EL::Event>()) else { return };

        for handler in handlers
            .downcast_ref::<BehaviorRegistryHandler<EL::Event>>()
            .unwrap()
        {
            handler(self, events, context);
        }
    }

    pub fn process<'a, EL>(&self, events: &mut EL)
    where
        EL: 'static + ProcessableEventList,
        EL::Event: EventHasBehavior<EventList = EL>,
        <EL::Event as EventHasBehavior>::Context<'a>: Default,
    {
        self.process_cx(events, &mut Default::default());
    }
}

// === VecEventList === //

#[derive(Debug)]
#[derive_where(Default)]
pub struct VecEventList<E> {
    process_list: RefCell<QueryVersionMap<usize>>,
    events: Vec<(Entity, E)>,
    owned: Vec<OwnedEntity>,
}

impl<E> VecEventList<E> {
    pub const fn new() -> Self {
        Self {
            process_list: RefCell::new(QueryVersionMap::new()),
            events: Vec::new(),
            owned: Vec::new(),
        }
    }
}

impl<E, C> EventTarget<E, C> for VecEventList<E> {
    fn fire_cx(&mut self, target: Entity, event: E, _context: C) {
        self.events.push((target, event));
    }

    fn fire_cx_owned(&mut self, target: OwnedEntity, event: E, context: C) {
        let (target, target_handle) = target.split_guard();
        self.owned.push(target);
        self.fire_cx(target_handle, event, context);
    }
}

impl<E> QueryableEventList for VecEventList<E> {
    type Event = E;

    fn query_raw<K, F>(&self, version_id: K, tags: &[RawTag], mut handler: F)
    where
        K: 'static + Send + Sync + hash::Hash + PartialEq,
        F: FnMut(Entity, &Self::Event) -> ControlFlow<()>,
    {
        let version = mem::replace(
            &mut *self.process_list.borrow_mut().entry(version_id, || 0),
            self.events.len(),
        );

        for (entity, event) in &self.events[version..] {
            if tags.iter().all(|&tag| entity.is_tagged(tag)) {
                match handler(*entity, event) {
                    ControlFlow::Continue(()) => {}
                    ControlFlow::Break(()) => break,
                }
            }
        }
    }
}

impl<E> ProcessableEventList for VecEventList<E> {
    fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    fn clear(&mut self) {
        self.process_list.get_mut().clear();
        self.events.clear();
        self.owned.clear();
    }
}

impl<E> Drop for VecEventList<E> {
    fn drop(&mut self) {
        debug_assert!(
            self.is_empty(),
            "Leaked one or more events from a VecEventList<{}>.",
            type_name::<E>()
        );
    }
}
