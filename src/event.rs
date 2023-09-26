use std::{
    any::{type_name, Any},
    cell::RefCell,
    fmt, hash,
    marker::PhantomData,
    mem,
    ops::ControlFlow,
};

use derive_where::derive_where;

use crate::{
    core::cell::{OptRef, OptRefMut},
    entity::{CompMut, CompRef, Entity, OwnedEntity},
    query::RawTag,
    util::{
        hash_map::{ConstSafeBuildHasherDefault, FxHashMap},
        iter::hash_one,
    },
};

// === EventTarget === //

pub trait EventTarget<E, C = ()> {
    fn fire(&mut self, target: Entity, event: E, context: C);

    fn fire_owned(&mut self, target: OwnedEntity, event: E, context: C);
}

impl<E, C, F> EventTarget<E, C> for F
where
    F: FnMut(Entity, E, C),
{
    fn fire(&mut self, target: Entity, event: E, context: C) {
        self(target, event, context);
    }

    fn fire_owned(&mut self, target: OwnedEntity, event: E, context: C) {
        self(target.entity(), event, context);
    }
}

// === SimpleEventTarget === //

pub trait SimpleEventTarget: EventTarget<Self::Event> + QueryableEventList {}

impl<T: EventTarget<T::Event> + QueryableEventList> SimpleEventTarget for T {}

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
                    .is_some_and(|candidate| candidate == &key)
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

    fn query_raw<K, I, F>(&self, version_id: K, tags: I, handler: F)
    where
        K: 'static + Send + Sync + hash::Hash + PartialEq,
        I: IntoIterator<Item = RawTag>,
        I::IntoIter: Clone,
        F: FnMut(Entity, &Self::Event) -> ControlFlow<()>;
}

pub trait ProcessableEventList {
    fn is_empty(&self) -> bool;

    fn clear(&mut self);
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
    fn fire(&mut self, target: Entity, event: E, _context: C) {
        self.events.push((target, event));
    }

    fn fire_owned(&mut self, target: OwnedEntity, event: E, context: C) {
        let (target, target_handle) = target.split_guard();
        self.owned.push(target);
        self.fire(target_handle, event, context);
    }
}

impl<E> QueryableEventList for VecEventList<E> {
    type Event = E;

    fn query_raw<K, I, F>(&self, version_id: K, tags: I, mut handler: F)
    where
        K: 'static + Send + Sync + hash::Hash + PartialEq,
        I: IntoIterator<Item = RawTag>,
        I::IntoIter: Clone,
        F: FnMut(Entity, &Self::Event) -> ControlFlow<()>,
    {
        let tags = tags.into_iter();
        let version = mem::replace(
            &mut *self.process_list.borrow_mut().entry(version_id, || 0),
            self.events.len(),
        );

        for (entity, event) in &self.events[version..] {
            if tags.clone().all(|tag| entity.is_tagged(tag)) {
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

// === CountingEvent === //

#[derive(Debug, Default)]
pub struct CountingEvent<E> {
    _ty: PhantomData<fn() -> E>,
    process_list: RefCell<QueryVersionMap<usize>>,
    owned: Vec<OwnedEntity>,
    count: u64,
}

impl<E> CountingEvent<E> {
    pub const fn new() -> Self {
        Self {
            _ty: PhantomData,
            process_list: RefCell::new(QueryVersionMap::new()),
            owned: Vec::new(),
            count: 0,
        }
    }

    pub fn take_all_events(&mut self) -> bool {
        let had_event = self.has_event();
        self.count = 0;
        had_event
    }

    pub fn take_one_event(&mut self) -> bool {
        let had_event = self.has_event();
        self.count -= 1;
        had_event
    }

    pub fn has_event(&self) -> bool {
        self.count > 0
    }

    pub fn count(&self) -> u64 {
        self.count
    }
}

impl<E> EventTarget<E> for CountingEvent<E> {
    fn fire(&mut self, _target: Entity, _event: E, _context: ()) {
        self.count += 1;
    }

    fn fire_owned(&mut self, target: OwnedEntity, _event: E, _context: ()) {
        self.count += 1;
        self.owned.push(target);
    }
}

impl<E> ProcessableEventList for CountingEvent<E> {
    fn is_empty(&self) -> bool {
        !self.has_event()
    }

    fn clear(&mut self) {
        self.process_list.get_mut().clear();
        self.count = 0;
        self.owned.clear();
    }
}

// === EventGroup === //

pub trait EventGroupMarkerWithSeparated<E> {
    type List: 'static + SimpleEventTarget<Event = E> + Default;
}

pub trait EventGroupMarkerWith<L: 'static + SimpleEventTarget + Default>:
    EventGroupMarkerWithSeparated<L::Event, List = L>
{
}

pub struct EventGroup<G: ?Sized> {
    _ty: PhantomData<fn(G) -> G>,
    events: OwnedEntity,
}

impl<G: ?Sized> EventGroup<G> {
    pub fn new() -> Self {
        Self {
            _ty: PhantomData,
            events: OwnedEntity::new()
                .with_debug_label(format_args!("EventGroup<{}>", type_name::<G>())),
        }
    }

    pub fn get<E>(&self) -> OptRef<'_, G::List>
    where
        G: EventGroupMarkerWithSeparated<E>,
    {
        CompRef::into_opt_ref(if !self.events.has::<G::List>() {
            let (_, obj) = self.events.insert_with_obj(<G::List as Default>::default());
            obj.get()
        } else {
            self.events.get()
        })
    }

    pub fn get_mut<E>(&self) -> OptRefMut<'_, G::List>
    where
        G: EventGroupMarkerWithSeparated<E>,
    {
        CompMut::into_opt_ref_mut(if !self.events.has::<G::List>() {
            let (_, obj) = self.events.insert_with_obj(<G::List as Default>::default());
            obj.get_mut()
        } else {
            self.events.get_mut()
        })
    }
}