use std::{cell::RefCell, marker::PhantomData, mem, ops::ControlFlow};

use derive_where::derive_where;

use crate::{
    entity::{Entity, OwnedEntity},
    query::{
        ArchetypeId, ArchetypeQueryInfo, DriverArchIterInfo, DriverBlockIterInfo,
        DriverHeapIterInfo, QueryBlockElementHandler, QueryBlockHandler, QueryDriver,
        QueryDriverEntryHandler, QueryDriverTypes, QueryHeapHandler, QueryKey, QueryVersionMap,
        RawTag,
    },
    util::{
        hash_map::FxHashSet,
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

pub trait SimpleEventTarget:
    EventTarget<Self::Event> + QueryDriver + for<'a> QueryDriverTypes<'a, Item = &'a Self::Event>
{
    type Event;
}

impl<E, T> SimpleEventTarget for T
where
    T: EventTarget<E>,
    T: QueryDriver + for<'a> QueryDriverTypes<'a, Item = &'a E>,
{
    type Event = E;
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
        (new == old, new)
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
    pub fn drain_recursive(&mut self, f: impl FnMut(&mut E, &mut E)) {
        drain_recursive(&mut self.primary, &mut self.secondary, f)
    }

    pub fn drain_recursive_breakable<B>(
        &mut self,
        f: impl FnMut(&mut E, &mut E) -> ControlFlow<B>,
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

pub fn drain_recursive<E: ProcessableEvent>(
    primary: &mut E,
    secondary: &mut E,
    mut f: impl FnMut(&mut E, &mut E),
) {
    drain_recursive_breakable::<_, ()>(primary, secondary, |reader, writer| {
        f(reader, writer);
        ControlFlow::Continue(())
    });
}

pub fn drain_recursive_breakable<E: ProcessableEvent, B>(
    primary: &mut E,
    secondary: &mut E,
    mut f: impl FnMut(&mut E, &mut E) -> ControlFlow<B>,
) -> ControlFlow<B> {
    // Run both loops to catch up the handler

    // primary -> secondary
    f(primary, secondary)?;

    // secondary -> primary
    let primary_version = primary.version();
    let secondary_version = secondary.version();
    f(secondary, primary)?;

    // The secondary event has been drained of updates leaving only the primary potentially
    // dirty. Hence, the `primary` is our first reader.
    let mut reader = (primary, primary_version);
    let mut writer = (secondary, secondary_version);

    // If the reader has been updated since the last time we processed it...
    while let (true, reader_version) = reader.0.has_updated_since(reader.1) {
        // Mark the new version.
        reader.1 = reader_version;

        // Let the closure handle it.
        f(reader.0, writer.0)?;

        // Now, because the `writer` is potentially dirty and the `reader` is now drained, the
        // two events swap places and the cycle continues.
        mem::swap(&mut reader, &mut writer);
    }

    ControlFlow::Continue(())
}
