use std::{
    any::TypeId,
    fmt,
    marker::PhantomData,
    mem::ManuallyDrop,
    ops::Deref,
    ptr::null_mut,
    sync::atomic::{AtomicU64, Ordering::Relaxed},
};

use crate::{
    util::{leak, ConstSafeBuildHasherDefault, FxHashMap},
    Entity,
};

use super::{
    cell::{OptRef, OptRefMut},
    token::{BorrowMutToken, BorrowToken, GetToken, MainThreadToken, Token, TokenFor},
    token_cell::{NMainCell, NOptRefCell},
};

pub(crate) static DEBUG_HEAP_COUNTER: AtomicU64 = AtomicU64::new(0);
pub(crate) static DEBUG_SLOT_COUNTER: AtomicU64 = AtomicU64::new(0);

// === ThreadedPtrMut == //

struct ThreadedPtrRef<T: ?Sized>(pub *const T);

impl<T: ?Sized> Copy for ThreadedPtrRef<T> {}

impl<T: ?Sized> Clone for ThreadedPtrRef<T> {
    fn clone(&self) -> Self {
        *self
    }
}

unsafe impl<T: ?Sized> Send for ThreadedPtrRef<T> {}
unsafe impl<T: ?Sized> Sync for ThreadedPtrRef<T> {}

// === Indirector === //

static FREE_INDIRECTORS: NOptRefCell<FxHashMap<TypeId, IndirectorSet>> =
    NOptRefCell::new_full(FxHashMap::with_hasher(ConstSafeBuildHasherDefault::new()));

struct IndirectorSet {
    empty: ThreadedPtrRef<()>,
    free_indirectors: Vec<&'static Indirector>,
}

struct Indirector(NMainCell<ThreadedPtrRef<()>>);

impl Default for Indirector {
    fn default() -> Self {
        Self(NMainCell::new(ThreadedPtrRef(null_mut())))
    }
}

// === Heap === //

#[derive(Debug)]
pub struct HeapValue<T> {
    owner: NMainCell<Option<Entity>>,
    value: NOptRefCell<T>,
}

impl<T> HeapValue<T> {
    pub fn owner(&self, token: &impl Token) -> Option<Entity> {
        self.owner.get(token)
    }

    pub fn value(&self) -> &NOptRefCell<T> {
        &self.value
    }
}

#[derive(Debug)]
pub struct Heap<T: 'static> {
    // N.B. mutability is not transitive through this box since indirectors will be referencing these
    // values.
    values: ManuallyDrop<Box<[HeapValue<T>]>>,
    slots: Box<[Slot<T>]>,
}

impl<T> Heap<T> {
    pub fn new(len: usize) -> Self {
        let token = MainThreadToken::acquire();

        // Allocate slot data
        let values = ManuallyDrop::new(Box::from_iter((0..len).map(|_| HeapValue {
            owner: NMainCell::new(None),
            value: NOptRefCell::new_empty(),
        })));

        // Allocate free slots
        let mut free_slots = FREE_INDIRECTORS.borrow_mut(token);
        let free_slots = free_slots
            .entry(TypeId::of::<T>())
            .or_insert_with(|| IndirectorSet {
                empty: ThreadedPtrRef(leak(HeapValue {
                    owner: NMainCell::new(None),
                    value: NOptRefCell::new_empty(),
                }) as *const HeapValue<T> as *const ()),
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
                    data.0.set(
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
        let mut free_slots = FREE_INDIRECTORS.borrow_mut(token);
        let entry = free_slots.get_mut(&TypeId::of::<T>()).unwrap();

        // Ensure that all slots are unborrowed and free them from their indirector.
        for slot in self.slots.iter() {
            assert!(
                slot.is_empty(token),
                "Heap was leaked because one or more slots were non-empty."
            );

            slot.indirector.0.set(token, entry.empty);
        }

        // Make all the slots contained in the heap free
        entry
            .free_indirectors
            .extend(self.slots.iter().map(|slot| slot.indirector));

        // Drop the heap values
        unsafe { ManuallyDrop::drop(&mut self.values) };
    }
}

// === Slot === //

pub struct WritableSlot<'a, T: 'static> {
    _ty: PhantomData<&'a Heap<T>>,
    slot: Slot<T>,
}

impl<T: 'static> WritableSlot<'_, T> {
    // TODO: Maybe we shouldn't be using main tokens for these??
    pub fn set_owner(&self, token: &MainThreadToken, owner: Option<Entity>) {
        unsafe { self.slot.heap_value(token) }
            .owner
            .set(token, owner);
    }

    pub fn set_value(&self, token: &impl BorrowMutToken<T>, value: Option<T>) -> Option<T> {
        let new_state = value.is_some();

        let old_state = unsafe {
            // Safety: this method is trivially safe since we never yield to the user while performing
            // the potentially unsafe action.
            self.slot.heap_value(token).value().replace(token, value)
        };

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

    pub fn set_value_owner_pair(
        &self,
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
}

impl<T: 'static> Deref for WritableSlot<'_, T> {
    type Target = Slot<T>;

    fn deref(&self) -> &Self::Target {
        &self.slot
    }
}

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
        f.debug_struct("Slot").finish_non_exhaustive()
    }
}

impl<T> Slot<T> {
    pub unsafe fn heap_value<'a>(&self, token: &impl Token) -> &'a HeapValue<T> {
        unsafe { &*self.indirector.0.get(token).0.cast::<HeapValue<T>>() }
    }

    pub fn owner(&self, token: &impl Token) -> Option<Entity> {
        unsafe {
            // Safety: this method is trivially safe since we never yield to the user while performing
            // the potentially unsafe action.
            self.heap_value(token).owner(token)
        }
    }

    pub fn get_or_none(self, token: &impl GetToken<T>) -> Option<&T> {
        unsafe {
            // Safety: a valid `GetToken` precludes main thread access for its lifetime.
            self.heap_value(token).value().get_or_none(token)
        }
    }

    pub fn get(self, token: &impl GetToken<T>) -> &T {
        unsafe {
            // Safety: a valid `GetToken` precludes main thread access for its lifetime.
            self.heap_value(token).value().get(token)
        }
    }

    pub fn borrow_or_none(self, token: &impl BorrowToken<T>) -> Option<OptRef<T>> {
        unsafe {
            // Safety: is this function succeeds, it will return an `OptRef` to its contents, which
            // precludes deletion until the reference expires.
            self.heap_value(token).value().borrow_or_none(token)
        }
    }

    pub fn borrow(self, token: &impl BorrowToken<T>) -> OptRef<T> {
        unsafe {
            // Safety: is this function succeeds, it will return an `OptRef` to its contents, which
            // precludes deletion until the reference expires.
            self.heap_value(token).value().borrow(token)
        }
    }

    pub fn borrow_mut_or_none(self, token: &impl BorrowMutToken<T>) -> Option<OptRefMut<T>> {
        unsafe {
            // Safety: is this function succeeds, it will return an `OptRef` to its contents, which
            // precludes deletion until the reference expires.
            self.heap_value(token).value().borrow_mut_or_none(token)
        }
    }

    pub fn borrow_mut(self, token: &impl BorrowMutToken<T>) -> OptRefMut<T> {
        unsafe {
            // Safety: is this function succeeds, it will return an `OptRef` to its contents, which
            // precludes deletion until the reference expires.
            self.heap_value(token).value().borrow_mut(token)
        }
    }

    pub fn take(&self, token: &impl BorrowMutToken<T>) -> Option<T> {
        let taken = unsafe {
            // Safety: this method is trivially safe since we never yield to the user while performing
            // the potentially unsafe action.
            self.heap_value(token).value().take(token)
        };

        if taken.is_some() {
            DEBUG_SLOT_COUNTER.fetch_sub(1, Relaxed);
        }
        taken
    }

    pub fn is_empty(&self, token: &impl TokenFor<T>) -> bool {
        unsafe {
            // Safety: this method is trivially safe since we never yield to the user while performing
            // the potentially unsafe action.
            self.heap_value(token).value().is_empty(token)
        }
    }
}

impl<T> Copy for Slot<T> {}

impl<T> Clone for Slot<T> {
    fn clone(&self) -> Self {
        *self
    }
}
