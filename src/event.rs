use std::{
    any::{type_name, Any, TypeId},
    cell::{RefCell, RefMut},
    fmt,
    marker::PhantomData,
    mem,
    ops::{ControlFlow, Deref, DerefMut},
};

use derive_where::derive_where;

use crate::{
    entity::{Entity, OwnedEntity},
    query::{
        ArchetypeId, ArchetypeQueryInfo, DriverArchIterInfo, DriverBlockIterInfo,
        DriverHeapIterInfo, MultiDriverItem, MultiQueryDriver, MultiQueryDriverTypes,
        QueryBlockElementHandler, QueryBlockHandler, QueryDriver, QueryDriverEntryHandler,
        QueryDriverTarget, QueryDriverTypes, QueryHeapHandler, QueryKey, QueryVersionMap, RawTag,
    },
    util::{
        hash_map::{FxHashMap, FxHashSet},
        misc::{IsUnit, Truthy},
    },
};

// === Core Traits === //

pub trait EventTarget<E, C = ()> {
    fn fire(&mut self, target: Entity, event: E)
    where
        IsUnit<C>: Truthy<Unit = C>,
    {
        self.fire_cx(target, event, IsUnit::make_unit());
    }

    fn fire_cx(&mut self, target: Entity, event: E, context: C);

    fn fire_owned(&mut self, target: OwnedEntity, event: E)
    where
        IsUnit<C>: Truthy<Unit = C>,
    {
        self.fire_owned_cx(target, event, IsUnit::make_unit());
    }

    fn fire_owned_cx(&mut self, target: OwnedEntity, event: E, context: C);
}

impl<E, C, F> EventTarget<E, C> for F
where
    F: FnMut(Entity, E, C),
{
    fn fire_cx(&mut self, target: Entity, event: E, context: C) {
        self(target, event, context);
    }

    fn fire_owned_cx(&mut self, target: OwnedEntity, event: E, context: C) {
        self(target.entity(), event, context);
    }
}

pub trait ProcessableEvent {
    type Version: 'static;

    fn version(&self) -> Self::Version;

    fn has_updated_since(&self, old: Self::Version) -> (bool, Self::Version);
}

pub trait ClearableEvent {
    fn clear(&mut self);
}

// === VecEventList === //

#[derive(Debug)]
#[derive_where(Default)]
pub struct VecEventList<T> {
    gen: u64,
    process_list: RefCell<QueryVersionMap<usize>>,
    events: Vec<(Entity, T)>,
    owned: Vec<OwnedEntity>,
}

impl<T> EventTarget<T> for VecEventList<T> {
    fn fire_cx(&mut self, target: Entity, event: T, _context: ()) {
        self.events.push((target, event));
    }

    fn fire_owned_cx(&mut self, target: OwnedEntity, event: T, _context: ()) {
        self.fire(target.entity(), event);
        self.owned.push(target);
    }
}

impl<T> ProcessableEvent for VecEventList<T> {
    type Version = (u64, usize);

    fn version(&self) -> Self::Version {
        (self.gen, self.events.len())
    }

    fn has_updated_since(&self, old: Self::Version) -> (bool, Self::Version) {
        let new = self.version();
        (new != old, new)
    }
}

impl<T> ClearableEvent for VecEventList<T> {
    fn clear(&mut self) {
        self.gen += 1;
        self.process_list.get_mut().clear();
        self.events.clear();
        self.owned.clear();
    }
}

impl<'a, T> MultiQueryDriverTypes<'a> for VecEventList<T> {
    type Item = &'a T;
}

impl<T> MultiQueryDriver for VecEventList<T> {
    fn drive_multi_query<T2: QueryDriverTarget, B>(
        &self,
        target: &mut T2,
        f: impl FnMut((T2::Input<'_>, MultiDriverItem<'_, Self>)) -> ControlFlow<B>,
    ) -> ControlFlow<B> {
        target.handle_driver(self, f)
    }
}

impl<'a, T> QueryDriverTypes<'a> for VecEventList<T> {
    type Item = &'a T;
    type ArchIterInfo = ();
    type HeapIterInfo = ();
    type BlockIterInfo = ();
}

impl<T> QueryDriver for VecEventList<T> {
    fn drive_query<B>(
        &self,
        query_key: impl QueryKey,
        tags: impl IntoIterator<Item = RawTag>,
        _include_entities: bool,
        mut handler: impl QueryDriverEntryHandler<Self, B>,
    ) -> ControlFlow<B> {
        let start = mem::replace(
            self.process_list.borrow_mut().entry(query_key, || 0),
            self.events.len(),
        );

        if let Some(archetypes) = ArchetypeId::in_intersection(tags, false) {
            let archetypes = archetypes
                .into_iter()
                .map(|v| v.archetype())
                .collect::<FxHashSet<_>>();

            for (entity, item) in &self.events[start..] {
                if archetypes.contains(
                    &entity
                        .archetypes()
                        .expect("VecEventList has dead entity")
                        .physical,
                ) {
                    handler.process_arbitrary(*entity, item)?;
                }
            }
        } else {
            for (entity, item) in &self.events[start..] {
                handler.process_arbitrary(*entity, item)?;
            }
        }

        ControlFlow::Continue(())
    }

    fn foreach_heap<B>(
        &self,
        _arch: &ArchetypeQueryInfo,
        _arch_userdata: &mut DriverArchIterInfo<'_, Self>,
        _handler: impl QueryHeapHandler<Self, B>,
    ) -> ControlFlow<B> {
        unimplemented!()
    }

    fn foreach_block<B>(
        &self,
        _heap_idx: usize,
        _heap_len: usize,
        _heap_userdata: &mut DriverHeapIterInfo<'_, Self>,
        _handler: impl QueryBlockHandler<Self, B>,
    ) -> ControlFlow<B> {
        unimplemented!()
    }

    fn foreach_element_in_full_block<B>(
        &self,
        _block: usize,
        _block_userdata: &mut DriverBlockIterInfo<'_, Self>,
        _handler: impl QueryBlockElementHandler<Self, B>,
    ) -> ControlFlow<B> {
        unimplemented!()
    }

    fn foreach_element_in_semi_block<B>(
        &self,
        _block: usize,
        _block_userdata: &mut DriverBlockIterInfo<'_, Self>,
        _handler: impl QueryBlockElementHandler<Self, B>,
    ) -> ControlFlow<B> {
        unimplemented!()
    }
}

// === CountingEvent === //

#[derive(Debug, Default)]
pub struct CountingEvent<E> {
    _ty: PhantomData<fn() -> E>,
    owned: Vec<OwnedEntity>,
    count: u64,
}

impl<E> CountingEvent<E> {
    pub const fn new() -> Self {
        Self {
            _ty: PhantomData,
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

impl<E, C> EventTarget<E, C> for CountingEvent<E> {
    fn fire_cx(&mut self, _target: Entity, _event: E, _cx: C) {
        self.count += 1;
    }

    fn fire_owned_cx(&mut self, target: OwnedEntity, _event: E, _cx: C) {
        self.count += 1;
        self.owned.push(target);
    }
}

// === NopEvent === //

#[derive(Debug, Clone, Default)]
pub struct NopEvent;

impl<E> EventTarget<E> for NopEvent {
    fn fire_cx(&mut self, _target: Entity, _event: E, _context: ()) {}

    fn fire_owned_cx(&mut self, _target: OwnedEntity, _event: E, _context: ()) {}
}

// === EventSwapper === //

#[derive(Debug, Clone, Default)]
pub struct EventSwapper<E> {
    pub primary: E,
    pub secondary: E,
}

impl<E: Default> EventSwapper<E> {
    pub fn new(primary: E) -> Self {
        Self {
            primary,
            secondary: E::default(),
        }
    }
}

impl<E: ProcessableEvent> EventSwapper<E> {
    pub fn drain_recursive(&mut self, f: impl FnMut(&E, &mut E)) {
        drain_recursive(&mut self.primary, &mut self.secondary, f)
    }

    pub fn drain_recursive_breakable<B>(
        &mut self,
        f: impl FnMut((&E, &mut E)) -> ControlFlow<B>,
    ) -> ControlFlow<B> {
        drain_recursive_breakable(&mut self.primary, &mut self.secondary, f)
    }
}

impl<E: ClearableEvent> ClearableEvent for EventSwapper<E> {
    fn clear(&mut self) {
        self.primary.clear();
        self.secondary.clear();
    }
}

impl<E> Deref for EventSwapper<E> {
    type Target = E;

    fn deref(&self) -> &Self::Target {
        &self.primary
    }
}

impl<E> DerefMut for EventSwapper<E> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.primary
    }
}

pub fn drain_recursive<E: ProcessableEvent>(
    primary: &mut E,
    secondary: &mut E,
    mut f: impl FnMut(&E, &mut E),
) {
    drain_recursive_breakable::<_, ()>(primary, secondary, |(reader, writer)| {
        f(reader, writer);
        ControlFlow::Continue(())
    });
}

pub fn drain_recursive_breakable<E: ProcessableEvent, B>(
    primary: &mut E,
    secondary: &mut E,
    mut f: impl FnMut((&E, &mut E)) -> ControlFlow<B>,
) -> ControlFlow<B> {
    // Run both loops to catch up the handler

    // primary -> secondary
    f((primary, secondary))?;

    // secondary -> primary
    let primary_version = primary.version();
    let secondary_version = secondary.version();
    f((secondary, primary))?;

    // The secondary event has been drained of updates leaving only the primary potentially
    // dirty. Hence, the `primary` is our first reader.
    let mut reader = (primary, primary_version);
    let mut writer = (secondary, secondary_version);

    // If the reader has been updated since the last time we processed it...
    while let (true, reader_version) = reader.0.has_updated_since(reader.1) {
        // Mark the new version.
        reader.1 = reader_version;

        // Let the closure handle it.
        f((reader.0, writer.0))?;

        // Now, because the `writer` is potentially dirty and the `reader` is now drained, the
        // two events swap places and the cycle continues.
        mem::swap(&mut reader, &mut writer);
    }

    ControlFlow::Continue(())
}

// === EventGroup === //

// SimpleEventList
pub trait SimpleEventList:
    'static
    + Default
    + Send
    + EventTarget<Self::Event>
    + ClearableEvent
    + QueryDriver
    + for<'a> QueryDriverTypes<'a, Item = &'a Self::Event>
{
    type Event;
}

impl<E, L> SimpleEventList for L
where
    L: 'static + Default + Send + EventTarget<E> + ClearableEvent,
    L: QueryDriver + for<'a> QueryDriverTypes<'a, Item = &'a E>,
{
    type Event = E;
}

// EventGroupMarkerXx
pub trait EventGroupMarkerWithSeparated<E> {
    type List: 'static + SimpleEventList<Event = E> + Default;
}

pub trait EventGroupMarkerWith<L: 'static + SimpleEventList + Default>:
    EventGroupMarkerWithSeparated<L::Event, List = L>
{
}

pub trait EventGroupMarkerExtends<G: ?Sized> {}

// EventGroup
#[repr(C)]
#[derive_where(Default)]
pub struct EventGroup<G: ?Sized> {
    _ty: PhantomData<fn(G) -> G>,
    events: Vec<Box<dyn ErasedEvent>>,
    ty_map: FxHashMap<TypeId, usize>,
    version: u64,
}

trait ErasedEvent: Any + Send + ClearableEvent {
    fn ty_name(&self) -> &'static str;

    fn as_any(&self) -> &dyn Any;

    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: Any + Send + ClearableEvent> ErasedEvent for T {
    fn ty_name(&self) -> &'static str {
        type_name::<T>()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl<G: ?Sized> fmt::Debug for EventGroup<G> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct FmtEventsList<'a>(&'a [Box<dyn ErasedEvent>]);

        impl fmt::Debug for FmtEventsList<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_list()
                    .entries(self.0.iter().map(|v| v.ty_name()))
                    .finish()
            }
        }

        f.debug_struct("EventGroup")
            .field("events", &FmtEventsList(&self.events))
            .finish_non_exhaustive()
    }
}

impl<G: ?Sized> EventGroup<G> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn wrap_swapper(self) -> EventSwapper<Self> {
        EventSwapper::new(self)
    }

    pub fn read_raw<L: SimpleEventList>(&self) -> Option<&L> {
        self.ty_map
            .get(&TypeId::of::<L>())
            .map(|&idx| self.events[idx].as_any().downcast_ref::<L>().unwrap())
    }

    pub fn read<E>(&self) -> Option<&G::List>
    where
        G: EventGroupMarkerWithSeparated<E>,
    {
        self.read_raw()
    }

    fn write_raw_index<L: SimpleEventList>(&mut self) -> usize {
        *self.ty_map.entry(TypeId::of::<L>()).or_insert_with(|| {
            let idx = self.events.len();
            self.events.push(Box::<L>::default());
            idx
        })
    }

    pub fn fire_raw<L: SimpleEventList>(&mut self, target: Entity, event: L::Event) {
        self.version += 1;

        let index = self.write_raw_index::<L>();
        self.events[index]
            .as_any_mut()
            .downcast_mut::<L>()
            .unwrap()
            .fire(target, event);
    }

    pub fn fire_owned_raw<L: SimpleEventList>(&mut self, target: OwnedEntity, event: L::Event) {
        self.version += 1;

        let index = self.write_raw_index::<L>();
        self.events[index]
            .as_any_mut()
            .downcast_mut::<L>()
            .unwrap()
            .fire_owned(target, event);
    }

    pub fn clear_single_raw<L: SimpleEventList>(&mut self) {
        // N.B. we don't update the version here since this action didn't actually add any new events.

        if let Some(&index) = self.ty_map.get(&TypeId::of::<L>()) {
            self.events[index].clear();
        }
    }

    pub fn clear_single<E>(&mut self)
    where
        G: EventGroupMarkerWithSeparated<E>,
    {
        self.clear_single_raw::<G::List>();
    }

    pub fn writer(&mut self) -> EventGroupWriter<'_, G> {
        EventGroupWriter {
            group: RefCell::new(self),
        }
    }

    pub fn cast_arbitrary<G2: ?Sized>(self) -> EventGroup<G2> {
        EventGroup {
            _ty: PhantomData,
            events: self.events,
            ty_map: self.ty_map,
            version: self.version,
        }
    }

    pub fn cast_arbitrary_ref<G2: ?Sized>(&self) -> &EventGroup<G2> {
        #[allow(unsafe_code)] // TODO: Use a library
        unsafe {
            mem::transmute(self)
        }
    }

    pub fn cast_arbitrary_mut<G2: ?Sized>(&mut self) -> &mut EventGroup<G2> {
        #[allow(unsafe_code)] // TODO: Use a library
        unsafe {
            mem::transmute(self)
        }
    }

    pub fn cast<G2: ?Sized + EventGroupMarkerExtends<G>>(self) -> EventGroup<G2> {
        self.cast_arbitrary()
    }

    pub fn cast_ref<G2: ?Sized + EventGroupMarkerExtends<G>>(&self) -> &EventGroup<G2> {
        self.cast_arbitrary_ref()
    }

    pub fn cast_mut<G2: ?Sized + EventGroupMarkerExtends<G>>(&mut self) -> &mut EventGroup<G2> {
        self.cast_arbitrary_mut()
    }
}

impl<G: ?Sized, E, C> EventTarget<E, C> for EventGroup<G>
where
    G: EventGroupMarkerWithSeparated<E>,
{
    fn fire_cx(&mut self, target: Entity, event: E, _context: C) {
        self.fire_raw::<G::List>(target, event);
    }

    fn fire_owned_cx(&mut self, target: OwnedEntity, event: E, _context: C) {
        self.fire_owned_raw::<G::List>(target, event);
    }
}

impl<G: ?Sized> ProcessableEvent for EventGroup<G> {
    type Version = u64;

    fn version(&self) -> Self::Version {
        self.version
    }

    fn has_updated_since(&self, old: Self::Version) -> (bool, Self::Version) {
        let new = self.version;
        (new != old, new)
    }
}

impl<G: ?Sized> ClearableEvent for EventGroup<G> {
    fn clear(&mut self) {
        // N.B. we don't update the version here since this action didn't actually add any new events.

        for event in &mut self.events {
            event.clear();
        }
    }
}

#[derive_where(Debug)]
pub struct EventGroupWriter<'g, G: ?Sized> {
    group: RefCell<&'g mut EventGroup<G>>,
}

impl<'g, G: ?Sized> EventGroupWriter<'g, G> {
    pub fn group(&mut self) -> &mut EventGroup<G> {
        self.group.get_mut()
    }

    pub fn event_raw<'w, L: SimpleEventList>(&'w self) -> EventGroupWriterSpec<'g, 'w, G, L> {
        EventGroupWriterSpec {
            _ty: PhantomData,
            index: self.group.borrow_mut().write_raw_index::<L>(),
            writer: self,
        }
    }

    pub fn event<'w, E>(&'w self) -> EventGroupWriterSpec<'g, 'w, G, G::List>
    where
        G: EventGroupMarkerWithSeparated<E>,
    {
        self.event_raw::<G::List>()
    }

    pub fn fire_raw<L: SimpleEventList>(&self, target: Entity, event: L::Event) {
        self.group.borrow_mut().fire_raw::<L>(target, event);
    }

    pub fn fire_owned_raw<L: SimpleEventList>(&self, target: OwnedEntity, event: L::Event) {
        self.group.borrow_mut().fire_owned_raw::<L>(target, event);
    }

    pub fn fire<E>(&self, target: Entity, event: E)
    where
        G: EventGroupMarkerWithSeparated<E>,
    {
        self.group.borrow_mut().fire(target, event);
    }

    pub fn fire_owned<E>(&self, target: OwnedEntity, event: E)
    where
        G: EventGroupMarkerWithSeparated<E>,
    {
        self.group.borrow_mut().fire_owned(target, event);
    }

    pub fn clear_single_raw<L: SimpleEventList>(&self) {
        self.group.borrow_mut().clear_single_raw::<L>();
    }

    pub fn clear_single<E>(&self)
    where
        G: EventGroupMarkerWithSeparated<E>,
    {
        self.group.borrow_mut().clear_single::<E>();
    }
}

pub struct EventGroupWriterSpec<'g, 'w, G: ?Sized, L> {
    _ty: PhantomData<fn() -> L>,
    writer: &'w EventGroupWriter<'g, G>,
    index: usize,
}

impl<G: ?Sized, L: 'static + fmt::Debug> fmt::Debug for EventGroupWriterSpec<'_, '_, G, L> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventGroupWriterSpec")
            .field("writer", &self.writer)
            .field("index", &self.index)
            .field("instance", &*self.get(false))
            .finish()
    }
}

impl<G: ?Sized, L: 'static> EventGroupWriterSpec<'_, '_, G, L> {
    fn get(&self, increment_version: bool) -> RefMut<'_, L> {
        RefMut::map(self.writer.group.borrow_mut(), |group| {
            if increment_version {
                group.version += 1;
            }

            group.events[self.index]
                .as_any_mut()
                .downcast_mut::<L>()
                .unwrap()
        })
    }
}

impl<G: ?Sized, L, C> EventTarget<L::Event, C> for EventGroupWriterSpec<'_, '_, G, L>
where
    L: SimpleEventList,
{
    fn fire_cx(&mut self, target: Entity, event: L::Event, _context: C) {
        self.get(true).fire(target, event);
    }

    fn fire_owned_cx(&mut self, target: OwnedEntity, event: L::Event, _context: C) {
        self.get(true).fire_owned(target, event);
    }
}

impl<G: ?Sized, L> ClearableEvent for EventGroupWriterSpec<'_, '_, G, L>
where
    L: SimpleEventList,
{
    fn clear(&mut self) {
        // N.B. we don't update the version here since this action didn't actually add any new events.
        self.get(false).clear();
    }
}
