use std::{
    any::TypeId,
    marker::PhantomData,
    mem::ManuallyDrop,
    ptr::null_mut,
    sync::atomic::{AtomicPtr, Ordering::Relaxed},
};

use crate::util::{leak, ConstSafeBuildHasherDefault, FxHashMap};

use super::{
    cell::{OptRef, OptRefMut},
    namespace::{
        ExclusiveToken, MainThreadToken, NOptRefCell, ReadToken, UnJailMutToken, UnJailRefToken,
    },
};

// === SlotData === //

// Invariants: `SlotData` must, at all times, point to a valid instance of `NOptRefCell<T>`.
//  This instance must stay alive for as long as the value is borrowed or about to be borrowed
//  by the current thread (in other words: the deleting thread must have exclusive access to the
//  value and it must be unborrowed).
type SlotData = &'static AtomicPtr<()>;

static FREE_SLOTS: NOptRefCell<FxHashMap<TypeId, Vec<SlotData>>> =
    NOptRefCell::new_full(FxHashMap::with_hasher(ConstSafeBuildHasherDefault::new()));

fn alloc_slots(token: &MainThreadToken, ty: TypeId, count: usize) -> Box<[SlotData]> {
    let mut free_slots = FREE_SLOTS.borrow_mut(token);
    let free_slots = free_slots.entry(ty).or_default();

    // Ensure that we have enough free slots for the operation.
    if free_slots.len() < count {
        // Determine the number of elements needed to get `free_slots.len() == count`.
        // We always want to allocate at least 128 slots to avoid the overhead of small allocations.
        let additional = (count - free_slots.len()).max(128);

        // ...and add them to the slots vector. All slots are allocated contiguously in a leaked
        // allocation and then pointers to them are inserted into the list.
        free_slots.extend(
            leak(Box::from_iter(
                (0..additional).map(|_| AtomicPtr::new(null_mut())),
            ))
            .iter(),
        );
    }

    // ...now, to return `N` slots in a new `Box<[...]>`.
    //
    // To avoid an off-by-one, let's be a bit explicit here:
    //
    // `Vec::drain(new_len..)` drains the vector such that `new_len` becomes the new length of the
    // vector (this is true because the element at `new_len` is the first element of the drain, not
    // the last element kept in the vector; hence the vector is left with elements up to `new_len - 1`).
    //
    // Since we're reducing the length of the vector from `len` to `len - count`, we should expect
    // `count` elements to be returned, as was desired.
    Box::from_iter(free_slots.drain((free_slots.len() - count)..))
}

fn dealloc_slots(token: &MainThreadToken, ty: TypeId, slots: impl IntoIterator<Item = SlotData>) {
    FREE_SLOTS
        .borrow_mut(token)
        .entry(ty)
        .or_default()
        .extend(slots);
}

// === Slot === //

#[derive(Debug)]
pub struct Slot<T: 'static> {
    ty: PhantomData<&'static NOptRefCell<T>>,
    ptr: SlotData,
}

impl<T> Copy for Slot<T> {}

impl<T> Clone for Slot<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Slot<T> {
    pub fn get<'b, V>(self, token: &'b V) -> &'b T
    where
        V: ReadToken<T> + UnJailRefToken<T>,
    {
        unsafe {
            // Safety: this `OptRefCell` is alive for as long as our reference to `V`.
            (*self.ptr.load(Relaxed).cast::<NOptRefCell<T>>()).get(token)
        }
    }

    pub fn borrow<'b, V>(self, token: &'b V) -> OptRef<'b, T>
    where
        V: ExclusiveToken<T> + UnJailRefToken<T>,
    {
        unsafe {
            // Safety: this `OptRefCell` is alive until our `OptRef` is dropped.
            (*self.ptr.load(Relaxed).cast::<NOptRefCell<T>>()).borrow(token)
        }
    }

    pub fn borrow_mut<'b, V>(self, token: &'b V) -> OptRefMut<'b, T>
    where
        V: ExclusiveToken<T> + UnJailMutToken<T>,
    {
        unsafe {
            // Safety: this `OptRefCell` is alive until our `OptRefMut` is dropped.
            (*self.ptr.load(Relaxed).cast::<NOptRefCell<T>>()).borrow_mut(token)
        }
    }
}

// === Heap === //

#[derive(Debug)]
pub struct Heap<T: 'static> {
    slots: Box<[SlotData]>,
    values: ManuallyDrop<Box<[NOptRefCell<T>]>>,
}

impl<T: 'static> Heap<T> {
    pub fn new(approx_len: usize) -> Self {
        todo!()
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn slot(&self, i: usize) -> Slot<T> {
        Slot {
            ty: PhantomData,
            ptr: self.slots[i],
        }
    }
}

impl<T: 'static> Drop for Heap<T> {
    fn drop(&mut self) {
        let token = MainThreadToken::acquire();

        dealloc_slots(
            token,
            TypeId::of::<T>(),
            self.slots.iter().map(|slot| *slot),
        );
    }
}
