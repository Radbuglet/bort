// TODO: Replace with a non-leaky implementation once `OptRefCell`s become zeroable.

use std::{
    any::TypeId,
    marker::PhantomData,
    ops::Deref,
    sync::atomic::{AtomicU64, Ordering::Relaxed},
};

use crate::util::{ConstSafeBuildHasherDefault, FxHashMap};

use super::{
    cell::{OptRef, OptRefMut},
    token::{BorrowMutToken, BorrowToken, GetToken, MainThreadToken, NOptRefCell},
};

static FREE_SLOTS: NOptRefCell<FxHashMap<TypeId, Vec<*const ()>>> =
    NOptRefCell::new_full(FxHashMap::with_hasher(ConstSafeBuildHasherDefault::new()));

pub(crate) static DEBUG_HEAP_COUNTER: AtomicU64 = AtomicU64::new(0);
pub(crate) static DEBUG_SLOT_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub struct Heap<T: 'static> {
    slots: Box<[Slot<T>]>,
}

impl<T> Heap<T> {
    pub fn new(min_len: usize) -> Self {
        let mut free_slots = FREE_SLOTS.borrow_mut(MainThreadToken::acquire());
        let free_slots = free_slots.entry(TypeId::of::<T>()).or_default();

        if free_slots.len() < min_len {
            let additional = (min_len - free_slots.len()).max(128);
            free_slots.extend(
                Box::leak(Box::from_iter(
                    (0..additional).map(|_| NOptRefCell::new_empty()),
                ))
                .iter()
                .map(|p| p as *const NOptRefCell<T> as *const ()),
            );
        }

        let slots = Box::from_iter(
            free_slots
                .drain((free_slots.len() - min_len)..)
                .map(|slot| Slot {
                    value: unsafe { &*slot.cast::<NOptRefCell<T>>() },
                }),
        );

        DEBUG_HEAP_COUNTER.fetch_add(1, Relaxed);

        Self { slots }
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn slot(&self, i: usize) -> WritableSlot<'_, T> {
        WritableSlot {
            _ty: PhantomData,
            slot: self.slots[i],
        }
    }

    pub fn slots(&self) -> &[Slot<T>] {
        &self.slots
    }
}

impl<T> Drop for Heap<T> {
    fn drop(&mut self) {
        DEBUG_HEAP_COUNTER.fetch_sub(1, Relaxed);

        let token = MainThreadToken::acquire();
        let mut free_slots = FREE_SLOTS.borrow_mut(token);
        let free_slots = free_slots.entry(TypeId::of::<T>()).or_default();

        // Make all the slots contained in the heap free
        free_slots.extend(
            self.slots
                .iter()
                .map(|slot| slot.value as *const NOptRefCell<T> as *const ()),
        );

        // If any of the slots are still borrowed, panic here.
        // In the real implementation, we'd use this panic to guard against invalid deallocation.
        // Here, however, we just use the panic for forwards-compatibility reasons.
        assert!(
            self.slots.iter().all(|slot| slot.value.is_empty(token)),
            "Heap was leaked because one or more slots were non-empty."
        );
    }
}

pub struct WritableSlot<'a, T: 'static> {
    _ty: PhantomData<&'a Heap<T>>,
    slot: Slot<T>,
}

impl<T: 'static> WritableSlot<'_, T> {
    pub fn write(&self, token: &impl BorrowMutToken<T>, value: Option<T>) -> Option<T> {
        let new_state = value.is_some();

        let old_state = self.slot.value.replace(token, value);

        match new_state as i8 - old_state.is_some() as i8 {
            -1 => {
                DEBUG_SLOT_COUNTER.fetch_sub(1, Relaxed);
            }
            1 => {
                DEBUG_SLOT_COUNTER.fetch_add(1, Relaxed);
            }
            _ => {}
        };

        old_state
    }
}

impl<T: 'static> Deref for WritableSlot<'_, T> {
    type Target = Slot<T>;

    fn deref(&self) -> &Self::Target {
        &self.slot
    }
}

#[derive(Debug)]
pub struct Slot<T: 'static> {
    value: &'static NOptRefCell<T>,
}

impl<T> Slot<T> {
    pub fn get_or_none(self, token: &impl GetToken<T>) -> Option<&T> {
        self.value.get_or_none(token)
    }

    pub fn get(self, token: &impl GetToken<T>) -> &T {
        self.value.get(token)
    }

    pub fn borrow_or_none(self, token: &impl BorrowToken<T>) -> Option<OptRef<T>> {
        self.value.borrow_or_none(token)
    }

    pub fn borrow(self, token: &impl BorrowToken<T>) -> OptRef<T> {
        self.value.borrow(token)
    }

    pub fn borrow_mut_or_none(self, token: &impl BorrowMutToken<T>) -> Option<OptRefMut<T>> {
        self.value.borrow_mut_or_none(token)
    }

    pub fn borrow_mut(self, token: &impl BorrowMutToken<T>) -> OptRefMut<T> {
        self.value.borrow_mut(token)
    }

    pub fn take(&self, token: &impl BorrowMutToken<T>) -> Option<T> {
        let taken = self.value.take(token);
        if taken.is_some() {
            DEBUG_SLOT_COUNTER.fetch_sub(1, Relaxed);
        }
        taken
    }
}

impl<T> Copy for Slot<T> {}

impl<T> Clone for Slot<T> {
    fn clone(&self) -> Self {
        *self
    }
}