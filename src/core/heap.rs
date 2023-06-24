use std::{
    fmt,
    marker::PhantomData,
    mem::ManuallyDrop,
    ptr::null_mut,
    sync::atomic::{AtomicU64, Ordering::Relaxed},
};

use derive_where::derive_where;

use crate::{
    database::InertEntity,
    entity::Entity,
    util::{
        hash_map::{FxHashBuilder, FxHashMap},
        misc::{leak, NamedTypeId},
    },
};

use super::{
    cell::{OptRef, OptRefMut},
    token::{BorrowMutToken, BorrowToken, GetToken, MainThreadToken, Token, TokenFor},
    token_cell::{NMainCell, NOptRefCell},
};

pub(crate) static DEBUG_HEAP_COUNTER: AtomicU64 = AtomicU64::new(0);
pub(crate) static DEBUG_SLOT_COUNTER: AtomicU64 = AtomicU64::new(0);

// === ThreadedPtrMut == //

#[derive_where(Debug)]
#[derive_where(Copy, Clone)]
struct ThreadedPtrRef<T: ?Sized>(pub *const T);

unsafe impl<T: ?Sized> Send for ThreadedPtrRef<T> {}
unsafe impl<T: ?Sized> Sync for ThreadedPtrRef<T> {}

// === Indirector === //

static FREE_INDIRECTORS: NOptRefCell<FxHashMap<NamedTypeId, IndirectorSet>> =
    NOptRefCell::new_full(FxHashMap::with_hasher(FxHashBuilder::new()));

struct IndirectorSet {
    empty: ThreadedPtrRef<()>,
    free_indirectors: Vec<&'static Indirector>,
}

struct Indirector {
    owner: NMainCell<Option<InertEntity>>,
    value: NMainCell<ThreadedPtrRef<()>>,
}

impl Default for Indirector {
    fn default() -> Self {
        Self {
            owner: NMainCell::new(None),
            value: NMainCell::new(ThreadedPtrRef(null_mut())),
        }
    }
}

// === Heap === //

pub struct Heap<T: 'static> {
    // N.B. mutability is not transitive through this box since indirectors will be referencing these
    // values.
    values: ManuallyDrop<Box<[HeapValue<T>]>>,
    slots: Box<[Slot<T>]>,
}

struct HeapValue<T> {
    value: NOptRefCell<T>,
}

impl<T> Heap<T> {
    pub fn new(token: &MainThreadToken, len: usize) -> Self {
        // Allocate slot data
        let values = ManuallyDrop::new(Box::from_iter((0..len).map(|_| HeapValue {
            value: NOptRefCell::new_empty(),
        })));

        // Allocate free slots
        let mut free_slots = FREE_INDIRECTORS.borrow_mut(token);
        let free_slots =
            free_slots
                .entry(NamedTypeId::of::<T>())
                .or_insert_with(|| IndirectorSet {
                    empty: ThreadedPtrRef(leak(HeapValue {
                        value: NOptRefCell::new_empty(),
                    }) as *const HeapValue<T>
                        as *const ()),
                    free_indirectors: Vec::new(),
                });

        let free_slots = &mut free_slots.free_indirectors;

        if free_slots.len() < len {
            let additional = (len - free_slots.len()).max(128);
            free_slots.extend(
                Box::leak(Box::from_iter(
                    (0..additional).map(|_| Indirector::default()),
                ))
                .iter(),
            );
        }

        // Construct our slot vector
        let slots = Box::from_iter(
            free_slots
                .drain((free_slots.len() - len)..)
                .enumerate()
                .map(|(i, data)| {
                    data.value.set(
                        token,
                        ThreadedPtrRef(&values[i] as *const HeapValue<T> as *const ()),
                    );

                    Slot {
                        _ty: PhantomData,
                        indirector: data,
                    }
                }),
        );

        DEBUG_HEAP_COUNTER.fetch_add(1, Relaxed);

        Self { values, slots }
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn slot(&self, i: usize) -> DirectSlot<'_, T> {
        DirectSlot {
            slot: self.slots[i],
            heap_value: &self.values[i],
        }
    }

    pub fn slots(&self) -> (impl ExactSizeIterator<Item = DirectSlot<'_, T>> + Clone) {
        self.slots
            .iter()
            .zip(self.values.iter())
            .map(|(slot, heap_value)| DirectSlot {
                slot: *slot,
                heap_value,
            })
    }

    pub fn clear_slots(&self, token: &MainThreadToken) {
        for slot in self.slots() {
            slot.set_value_owner_pair(token, None);
        }
    }
}

impl<T> fmt::Debug for Heap<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Heap").field("slots", &self.slots).finish()
    }
}

impl<T> Drop for Heap<T> {
    fn drop(&mut self) {
        let token = MainThreadToken::acquire_fmt("destroy a heap");

        // We decrement the heap counter here so unfree-able heaps aren't forever included in the
        // count.
        DEBUG_HEAP_COUNTER.fetch_sub(1, Relaxed);

        // Ensure that all slots are cleared. If a slot is still being borrowed, this will panic.
        self.clear_slots(token);

        // Free all slots from their indirector and add them to the free indirector set.
        let mut free_slots = FREE_INDIRECTORS.borrow_mut(token);
        let entry = free_slots.get_mut(&NamedTypeId::of::<T>()).unwrap();

        for slot in self.slots.iter() {
            slot.indirector.value.set(token, entry.empty);
            entry.free_indirectors.push(slot.indirector);
        }

        // Drop the boxed slice of heap values.
        unsafe { ManuallyDrop::drop(&mut self.values) };
    }
}

// === Slot === //

#[derive_where(Copy, Clone)]
pub struct DirectSlot<'a, T: 'static> {
    slot: Slot<T>,
    heap_value: &'a HeapValue<T>,
}

impl<'a, T: 'static> DirectSlot<'a, T> {
    unsafe fn heap_value_prolonged<'b>(self) -> &'b HeapValue<T> {
        &*(self.heap_value as *const HeapValue<T>)
    }

    pub fn slot(self) -> Slot<T> {
        self.slot
    }

    pub fn set_owner(self, token: &MainThreadToken, owner: Option<Entity>) {
        self.slot
            .indirector
            .owner
            .set(token, owner.map(|ent| ent.inert));
    }

    pub fn set_value(self, token: &impl BorrowMutToken<T>, value: Option<T>) -> Option<T> {
        let new_state = value.is_some();
        let old_state = self.heap_value.value.replace(token, value);

        match new_state as i8 - old_state.is_some() as i8 {
            1 => {
                DEBUG_SLOT_COUNTER.fetch_add(1, Relaxed);
            }
            -1 => {
                DEBUG_SLOT_COUNTER.fetch_sub(1, Relaxed);
            }
            _ => {}
        };

        old_state
    }

    pub fn set_value_owner_pair(
        self,
        token: &MainThreadToken,
        value: Option<(Entity, T)>,
    ) -> Option<T> {
        if let Some((owner, value)) = value {
            self.set_owner(token, Some(owner));
            self.set_value(token, Some(value))
        } else {
            self.set_owner(token, None);
            self.set_value(token, None)
        }
    }

    pub fn swap(self, token: &MainThreadToken, other: DirectSlot<'_, T>) {
        // Swap the values
        self.heap_value.value.swap(token, &other.heap_value.value);

        // Swap the owners
        self.slot
            .indirector
            .owner
            .swap(token, &other.slot.indirector.owner);

        // Swap the indirector pointees
        self.slot
            .indirector
            .value
            .swap(token, &other.slot.indirector.value);
    }

    pub fn owner(self, token: &impl Token) -> Option<Entity> {
        self.slot
            .indirector
            .owner
            .get(token)
            .map(|ent| ent.into_dangerous_entity())
    }

    pub fn get_or_none(self, token: &impl GetToken<T>) -> Option<&T> {
        unsafe {
            // Safety: a valid `GetToken` precludes main thread access for its lifetime.
            self.heap_value_prolonged()
        }
        .value
        .get_or_none(token)
    }

    pub fn get(self, token: &impl GetToken<T>) -> &T {
        unsafe {
            // Safety: a valid `GetToken` precludes main thread access for its lifetime.
            self.heap_value_prolonged()
        }
        .value
        .get(token)
    }

    pub fn borrow_or_none(self, token: &impl BorrowToken<T>) -> Option<OptRef<T>> {
        unsafe {
            // Safety: is this function succeeds, it will return an `OptRef` to its contents, which
            // precludes deletion until the reference expires.
            self.heap_value_prolonged()
        }
        .value
        .borrow_or_none(token)
    }

    pub fn borrow(self, token: &impl BorrowToken<T>) -> OptRef<T> {
        unsafe {
            // Safety: is this function succeeds, it will return an `OptRef` to its contents, which
            // precludes deletion until the reference expires.
            self.heap_value_prolonged()
        }
        .value
        .borrow(token)
    }

    pub fn borrow_mut_or_none(self, token: &impl BorrowMutToken<T>) -> Option<OptRefMut<T>> {
        unsafe {
            // Safety: is this function succeeds, it will return an `OptRef` to its contents, which
            // precludes deletion until the reference expires.
            self.heap_value_prolonged()
        }
        .value
        .borrow_mut_or_none(token)
    }

    pub fn borrow_mut(self, token: &impl BorrowMutToken<T>) -> OptRefMut<T> {
        unsafe {
            // Safety: is this function succeeds, it will return an `OptRef` to its contents, which
            // precludes deletion until the reference expires.
            self.heap_value_prolonged()
        }
        .value
        .borrow_mut(token)
    }

    pub fn take(self, token: &impl BorrowMutToken<T>) -> Option<T> {
        let taken = self.heap_value.value.take(token);

        if taken.is_some() {
            DEBUG_SLOT_COUNTER.fetch_sub(1, Relaxed);
        }
        taken
    }

    pub fn is_empty(self, token: &impl TokenFor<T>) -> bool {
        self.heap_value.value.is_empty(token)
    }
}

#[derive_where(Copy, Clone)]
pub struct Slot<T: 'static> {
    _ty: PhantomData<&'static HeapValue<T>>,
    // Invariants: this indirector must always point to a valid instance of `HeapValue<T>`.
    // Additionally, the active indirect pointee cannot be invalidated until:
    //
    // a) the main thread regains control
    // b) the slot is empty
    //
    indirector: &'static Indirector,
}

impl<T> fmt::Debug for Slot<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Slot")
            .field("indirector", &(self.indirector as *const Indirector))
            .field("owner", &self.indirector.owner)
            .field("value", &self.indirector.value)
            .finish_non_exhaustive()
    }
}

impl<T> Slot<T> {
    pub unsafe fn direct_slot<'a>(self, token: &impl Token) -> DirectSlot<'a, T> {
        let heap_value = unsafe {
            // Safety: provided by caller
            &*self.indirector.value.get(token).0.cast::<HeapValue<T>>()
        };

        DirectSlot {
            slot: self,
            heap_value,
        }
    }

    pub fn set_owner(self, token: &MainThreadToken, owner: Option<Entity>) {
        unsafe {
            // Safety: we only use the `DirectSlot` until the function returns, and we know the
            // direct slot cannot be invalidated until then because we never call something which
            // could potentially destroy the heap.
            self.direct_slot(token).set_owner(token, owner)
        }
    }

    pub fn set_value(self, token: &impl BorrowMutToken<T>, value: Option<T>) -> Option<T> {
        unsafe {
            // Safety: we only use the `DirectSlot` until the function returns, and we know the
            // direct slot cannot be invalidated until then because we never call something which
            // could potentially destroy the heap.
            self.direct_slot(token).set_value(token, value)
        }
    }

    pub fn set_value_owner_pair(
        self,
        token: &MainThreadToken,
        value: Option<(Entity, T)>,
    ) -> Option<T> {
        unsafe {
            // Safety: we only use the `DirectSlot` until the function returns, and we know the
            // direct slot cannot be invalidated until then because we never call something which
            // could potentially destroy the heap.
            self.direct_slot(token).set_value_owner_pair(token, value)
        }
    }

    pub fn swap_indirect(self, token: &MainThreadToken, other: Slot<T>) {
        unsafe {
            // Safety: we only use the `DirectSlot` until the function returns, and we know the
            // direct slot cannot be invalidated until then because we never call something which
            // could potentially destroy the heap.
            self.direct_slot(token)
                .swap(token, other.direct_slot(token))
        }
    }

    pub fn swap_direct(self, token: &MainThreadToken, other: DirectSlot<'_, T>) {
        unsafe {
            // Safety: we only use the `DirectSlot` until the function returns, and we know the
            // direct slot cannot be invalidated until then because we never call something which
            // could potentially destroy the heap.
            self.direct_slot(token).swap(token, other)
        }
    }

    pub fn owner(&self, token: &impl Token) -> Option<Entity> {
        unsafe {
            // Safety: we only use the `DirectSlot` until the function returns, and we know the
            // direct slot cannot be invalidated until then because we never call something which
            // could potentially destroy the heap.
            self.direct_slot(token).owner(token)
        }
    }

    pub fn get_or_none(self, token: &impl GetToken<T>) -> Option<&T> {
        unsafe {
            // Safety: we only use the `DirectSlot` until the function returns, and we know the
            // direct slot cannot be invalidated until then because we never call something which
            // could potentially destroy the heap.
            self.direct_slot(token).get_or_none(token)
        }
    }

    pub fn get(self, token: &impl GetToken<T>) -> &T {
        unsafe {
            // Safety: we only use the `DirectSlot` until the function returns, and we know the
            // direct slot cannot be invalidated until then because we never call something which
            // could potentially destroy the heap.
            self.direct_slot(token).get(token)
        }
    }

    pub fn borrow_or_none(self, token: &impl BorrowToken<T>) -> Option<OptRef<T>> {
        unsafe {
            // Safety: we only use the `DirectSlot` until the function returns, and we know the
            // direct slot cannot be invalidated until then because we never call something which
            // could potentially destroy the heap.
            self.direct_slot(token).borrow_or_none(token)
        }
    }

    pub fn borrow(self, token: &impl BorrowToken<T>) -> OptRef<T> {
        unsafe {
            // Safety: we only use the `DirectSlot` until the function returns, and we know the
            // direct slot cannot be invalidated until then because we never call something which
            // could potentially destroy the heap.
            self.direct_slot(token).borrow(token)
        }
    }

    pub fn borrow_mut_or_none(self, token: &impl BorrowMutToken<T>) -> Option<OptRefMut<T>> {
        unsafe {
            // Safety: we only use the `DirectSlot` until the function returns, and we know the
            // direct slot cannot be invalidated until then because we never call something which
            // could potentially destroy the heap.
            self.direct_slot(token).borrow_mut_or_none(token)
        }
    }

    pub fn borrow_mut(self, token: &impl BorrowMutToken<T>) -> OptRefMut<T> {
        unsafe {
            // Safety: we only use the `DirectSlot` until the function returns, and we know the
            // direct slot cannot be invalidated until then because we never call something which
            // could potentially destroy the heap.
            self.direct_slot(token).borrow_mut(token)
        }
    }

    pub fn take(&self, token: &impl BorrowMutToken<T>) -> Option<T> {
        unsafe {
            // Safety: we only use the `DirectSlot` until the function returns, and we know the
            // direct slot cannot be invalidated until then because we never call something which
            // could potentially destroy the heap.
            self.direct_slot(token).take(token)
        }
    }

    pub fn is_empty(&self, token: &impl TokenFor<T>) -> bool {
        unsafe {
            // Safety: we only use the `DirectSlot` until the function returns, and we know the
            // direct slot cannot be invalidated until then because we never call something which
            // could potentially destroy the heap.
            self.direct_slot(token).is_empty(token)
        }
    }
}
