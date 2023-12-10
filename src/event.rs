use std::{cell::RefCell, marker::PhantomData, mem, ops::ControlFlow};

use derive_where::derive_where;

use crate::{
    entity::{Entity, OwnedEntity},
    query::{
        ArchetypeId, ArchetypeQueryInfo, DriverArchIterInfo, DriverBlockIterInfo,
        DriverHeapIterInfo, QueryBlockElementHandler, QueryBlockHandler, QueryDriveEntryHandler,
        QueryDriver, QueryDriverTypes, QueryHeapHandler, QueryKey, QueryVersionMap, RawTag,
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

    fn has_updated(&self, old: Self::Version) -> (bool, Self::Version);

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

    fn has_updated(&self, old: Self::Version) -> (bool, Self::Version) {
        let new = (self.gen, self.events.len());
        (new == old, new)
    }

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
        mut handler: impl QueryDriveEntryHandler<Self, B>,
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
