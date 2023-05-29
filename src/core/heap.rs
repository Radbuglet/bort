// TODO: Replace with a non-leaky implementation once `OptRefCell`s become zeroable.

use std::any::TypeId;

use crate::util::{leak, ConstSafeBuildHasherDefault, FxHashMap};

use super::{
    cell::{OptRef, OptRefMut},
    token::{BorrowMutToken, BorrowToken, GetToken, MainThreadToken, NOptRefCell},
};

static FREE_SLOTS: NOptRefCell<FxHashMap<TypeId, Vec<*const ()>>> =
    NOptRefCell::new_full(FxHashMap::with_hasher(ConstSafeBuildHasherDefault::new()));

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
                leak(Box::from_iter(
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

        Self { slots }
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn set_slot(
        &self,
        token: &impl BorrowMutToken<T>,
        i: usize,
        value: Option<T>,
    ) -> Option<T> {
        self.slots[i].value.replace(token, value)
    }

    pub fn slot(&self, i: usize) -> Slot<T> {
        self.slots[i]
    }

    pub fn slots(&self) -> &[Slot<T>] {
        &self.slots
    }
}

impl<T> Drop for Heap<T> {
    fn drop(&mut self) {
        let token = MainThreadToken::acquire();
        let mut free_slots = FREE_SLOTS.borrow_mut(token);
        let free_slots = free_slots.entry(TypeId::of::<T>()).or_default();

        todo!();
    }
}

#[derive(Debug)]
pub struct Slot<T: 'static> {
    value: &'static NOptRefCell<T>,
}

impl<T> Slot<T> {
    pub fn get(self, token: &impl GetToken<T>) -> &T {
        self.value.get(token)
    }

    pub fn borrow(self, token: &impl BorrowToken<T>) -> OptRef<T> {
        self.value.borrow(token)
    }

    pub fn borrow_mut(self, token: &impl BorrowMutToken<T>) -> OptRefMut<T> {
        self.value.borrow_mut(token)
    }

    pub fn take(&self, token: &impl BorrowMutToken<T>) -> Option<T> {
        self.value.take(token)
    }
}

impl<T> Copy for Slot<T> {}

impl<T> Clone for Slot<T> {
    fn clone(&self) -> Self {
        *self
    }
}
