use std::{any::Any, fmt, hash, ops::ControlFlow};

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
    pub fn new() -> Self {
        Self::default()
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

pub trait QueryableEvent {
    type Event;
    type Proxy<'a>: EventTarget<Self::Event>;

    fn query_raw<T: 'static, F>(&self, version_id: T, tags: &[RawTag], handler: F) -> bool
    where
        F: FnMut(Entity, &Self::Event, &mut Self::Proxy<'_>) -> ControlFlow<()>;

    fn poll_recursion(&mut self) -> bool;
}

pub trait ProcessableEvent: QueryableEvent {
    fn is_empty(&self);

    fn clear(&mut self);
}

// === BehaviorRegistry === //

pub trait EventHasBehavior: Sized {
    type Context<'a>;
    type Event: QueryableEvent;
}

#[derive(Debug, Default)]
pub struct BehaviorRegistry {
    handlers: FxHashMap<NamedTypeId, Box<dyn Any + Send + Sync>>,
}

impl BehaviorRegistry {
    pub const fn new() -> Self {
        Self {
            handlers: FxHashMap::with_hasher(ConstSafeBuildHasherDefault::new()),
        }
    }

    pub fn register(&mut self) {
        todo!();
    }

    pub fn unregister(&mut self) {
        todo!();
    }

    pub fn process_cx<E, EL>(&mut self, events: &mut EL, context: E::Context<'_>)
    where
        E: EventHasBehavior,
        EL: ProcessableEvent<Event = E>,
    {
        todo!();
    }

    pub fn process<'a, E, EL>(&mut self, events: &mut EL)
    where
        E: EventHasBehavior,
        EL: ProcessableEvent<Event = E>,
        E::Context<'a>: Default,
    {
        self.process_cx(events, Default::default());
    }
}
