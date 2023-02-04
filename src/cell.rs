// TODO: Code-review

use std::{
    cell::{Cell, RefCell, UnsafeCell},
    fmt,
    marker::PhantomData,
    num::NonZeroU32,
    ops::{Deref, DerefMut},
    ptr::NonNull,
    sync::atomic::{fence, AtomicPtr, AtomicU32, AtomicU64, Ordering},
};

// === ThreadId === //

fn thread_id() -> NonZeroU32 {
    thread_local! {
        static THREAD_ID: Cell<Option<NonZeroU32>> = const { Cell::new(None) };
    }

    THREAD_ID.with(|v| {
        if let Some(v) = v.get() {
            v
        } else {
            static THREAD_ID_ALLOC: AtomicU32 = AtomicU32::new(1);

            // FIXME: Ensure that this ID never loops around.
            let id = NonZeroU32::new(THREAD_ID_ALLOC.fetch_add(1, Ordering::Relaxed)).unwrap();
            v.set(Some(id));
            id
        }
    })
}

// === Namespace === //

pub static MAIN_THREAD: NamespaceState = NamespaceState::new();

#[derive(Debug)] // TODO: Allow users to name these.
pub struct NamespaceState {
    owner: AtomicU32,
}

impl NamespaceState {
    pub const fn new() -> Self {
        NamespaceState {
            owner: AtomicU32::new(0),
        }
    }

    pub const fn as_namespace(&'static self) -> Namespace {
        Namespace(self)
    }
}

// TODO: Reuse unused namespaces.
#[derive(Debug, Copy, Clone)]
pub struct Namespace(&'static NamespaceState);

impl Namespace {
    pub fn new() -> Self {
        Self(Box::leak(Box::new(NamespaceState::new())))
    }

    pub fn acquire(self) -> NamespaceGuard {
        self.acquire_unguarded();
        NamespaceGuard(self)
    }

    pub fn acquire_unguarded(self) {
        let result = self.0.owner.compare_exchange(
            0,
            thread_id().get(),
            Ordering::Acquire,
            Ordering::Relaxed,
        );

        assert!(
            result.is_ok(),
            "Attempted to `acquire` an `Namespace` already held by another thread. \
             Owner: {}. Current thread: {}.",
            self.0.owner.load(Ordering::Relaxed),
            thread_id().get(),
        );
    }

    pub fn unacquire_unguarded(self) {
        let result = self.0.owner.compare_exchange(
            thread_id().get(),
            0,
            Ordering::Release,
            Ordering::Relaxed,
        );

        assert!(
            result.is_ok(),
            "Attempted to `unacquire` an `Namespace` not held by the current thread. \
             Owner: {}. Current thread: {}.",
            self.0.owner.load(Ordering::Relaxed),
            thread_id().get(),
        );
    }

    pub fn is_held(self) -> bool {
        self.is_held_by(thread_id())
    }

    fn is_held_by(self, id: NonZeroU32) -> bool {
        self.0.owner.load(Ordering::Relaxed) == id.get()
    }
}

#[derive(Debug)]
pub struct NamespaceGuard(Namespace);

impl NamespaceGuard {
    pub fn namespace(&self) -> Namespace {
        self.0
    }
}

impl Drop for NamespaceGuard {
    fn drop(&mut self) {
        self.0.unacquire_unguarded();
    }
}

// === SyncRefCell === //

#[derive(Debug)] // TODO: Give this a proper debug implementation.
struct CellState {
    namespace: AtomicPtr<NamespaceState>,
    // Layout: [MSB](lock_state: u32, last_thread: u32)[LSB]
    // N.B. We pack this state into one atomic to ensure that both the `lock_state` and the
    // `last_thread` are made visible at the same time.
    state: AtomicU64,
}

impl CellState {
    const HELD_ELSEWHERE_ERROR: &str = "lock held by another thread";

    pub const fn new(namespace: Namespace) -> Self {
        Self {
            namespace: AtomicPtr::new(namespace.0 as *const NamespaceState as *mut NamespaceState),
            state: AtomicU64::new(0),
        }
    }

    pub fn namespace(&self) -> Namespace {
        Namespace(unsafe { &*self.namespace.load(Ordering::Relaxed) })
    }

    fn decompose_state(full_state: u64) -> (u32, u32) {
        ((full_state >> 32) as u32, full_state as u32)
    }

    fn compose_state(lock_state: u32, current_thread: u32) -> u64 {
        ((lock_state as u64) << 32) + current_thread as u64
    }

    fn ensure_thread_exclusivity(
        &self,
        my_thread: NonZeroU32,
        (lock_state, current_thread): (u32, u32),
    ) {
        // Ensure that we have exclusive access to this cell.
        if current_thread == my_thread.get() {
            // This cell is bound to our thread ID and will reject all other threads.
            // Hence we will be the only thread proceeding, guaranteeing exclusivity.
            // (fallthrough)
        } else if lock_state == 0 {
            // Because the `lock_state` is zero, we know that the previous thread properly
            // relinquished its last reference and just never unset its ownership status.
            assert!(
                self.namespace().is_held_by(my_thread),
                "{}",
                Self::HELD_ELSEWHERE_ERROR
            );

            // This cell is bound to a lock held by this thread. This lock can only
            // be released by the thread that owns it, hence we will be the only
            // thread proceeding, guaranteeing exclusivity.
        } else {
            // Another thread has ongoing borrows on this cell. Regardless of who's the owner,
            // we have to reject this!
            panic!("{}", Self::HELD_ELSEWHERE_ERROR);
        }
    }

    pub fn lock_mutable(&self) {
        // Load state
        let (lock_state, _) = Self::decompose_state(self.state.load(Ordering::Relaxed));

        // Ensure that we can hold this lock.
        assert!(self.namespace().is_held(), "{}", Self::HELD_ELSEWHERE_ERROR);

        // Ensure that no one else is holding this lock. We don't really care whether `current_thread`
        // is ours because it can't have any ongoing borrows and won't be able to acquire any anyways
        // because we hold the namespace.
        assert_eq!(
            lock_state, 0,
            "Cannot borrow cell mutably: cell is already borrowed."
        );

        // Acquire the cell mutably.
        //
        // N.B. This requires neither inter-thread orderings (all these accesses are effectively
        // single-threaded) nor fences (we already place an acquire-release fence in acquiring/giving
        // up a `Namespace`).
        self.state
            .store(Self::compose_state(u32::MAX, 0), Ordering::Relaxed);
    }

    pub fn lock_immutable(&self) {
        // Load state
        let my_thread = thread_id();
        let (lock_state, current_thread) =
            Self::decompose_state(self.state.load(Ordering::Relaxed));

        // Ensure that the current thread has exclusive access of this cell.
        self.ensure_thread_exclusivity(my_thread, (lock_state, current_thread));

        // Acquire the cell mutably.
        assert!(
            lock_state < u32::MAX - 1,
            "Cannot borrow cell immutably: cell is mutably borrowed."
        );

        // N.B. This requires neither inter-thread orderings (all these accesses are effectively
        // single-threaded) nor an acquire fence (we already place an acquire fence for acquiring
        // a `Namespace`).
        self.state.store(
            Self::compose_state(lock_state + 1, my_thread.get()),
            Ordering::Relaxed,
        );
    }

    pub fn relock_immutable(&self) {
        let raw_state = self.state.load(Ordering::Relaxed);
        let (lock_state, current_thread) = Self::decompose_state(raw_state);

        assert_ne!(lock_state, u32::MAX - 1);
        debug_assert_ne!(lock_state, 0);
        debug_assert_ne!(lock_state, u32::MAX);
        debug_assert_eq!(current_thread, thread_id().get());

        self.state.store(raw_state + 1, Ordering::Relaxed);

        // N.B. We have a fence for `unlock_mutable` to ensure that all state changes made during the
        // borrow are made visible to other threads. However, because we're only giving an immutable
        // reference to the contents of this cell, we know that nothing (that we're in charge of) will
        // have been updated.
    }

    pub fn unlock_mutable(&self) {
        #[cfg(debug_assertions)]
        {
            let (lock_state, current_thread) =
                Self::decompose_state(self.state.load(Ordering::Relaxed));

            debug_assert_eq!(lock_state, u32::MAX);
            debug_assert_eq!(current_thread, thread_id().get());
        }

        self.state.store(0, Ordering::Relaxed);

        // N.B. While we typically don't need a release fence because `Namespace::unacquire` handles
        // it for us, unfortunately, they are necessary in the scenario where the `CellState` is still
        // borrowed after the lock is released. Luckily, these are free in x64 (modulo lost compiler
        // optimizations). Unfortunately, the same is not true in ARM64.
        //
        // TODO: Try to avoid it with a check to `self.namespace().is_held()`?
        fence(Ordering::Release);
    }

    pub fn unlock_immutable(&self) {
        #[cfg(debug_assertions)]
        {
            let (lock_state, current_thread) =
                Self::decompose_state(self.state.load(Ordering::Relaxed));

            debug_assert_ne!(lock_state, u32::MAX);
            debug_assert_ne!(lock_state, 0);
            debug_assert_eq!(current_thread, thread_id().get());
        }

        self.state
            .store(self.state.load(Ordering::Relaxed) - 1, Ordering::Relaxed);
    }

    pub fn set_namespace(&self, namespace: Namespace) {
        // Load state
        let my_thread = thread_id();
        let (lock_state, current_thread) =
            Self::decompose_state(self.state.load(Ordering::Relaxed));

        // Ensure that the current thread has exclusive access of this cell.
        self.ensure_thread_exclusivity(my_thread, (lock_state, current_thread));

        // Modify the owning namespace.
        unsafe {
            self.set_namespace_unchecked(namespace);
        }
    }

    pub fn set_namespace_mut(&mut self, namespace: Namespace) {
        *self.namespace.get_mut() = namespace.0 as *const NamespaceState as *mut NamespaceState;
    }

    // Safety: This is dangerous because a user could change the namespace to their own namespace
    // and call `lock_mutable`  while a call to `lock_mutable` on another thread is on-going. Thus,
    // a caller must guarantee that no other thread is trying to lock this cell.
    pub unsafe fn set_namespace_unchecked(&self, namespace: Namespace) {
        self.namespace.store(
            namespace.0 as *const NamespaceState as *mut NamespaceState,
            Ordering::Relaxed,
        );
    }
}

#[derive(Debug)] // TODO: Give this a proper debug, eq, and ord implementation.
pub struct SyncRefCell<T: ?Sized> {
    state: CellState,
    value: UnsafeCell<T>,
}

unsafe impl<T: Send> Send for SyncRefCell<T> {}

unsafe impl<T: Sync> Sync for SyncRefCell<T> {}

impl<T: Default> Default for SyncRefCell<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T> From<T> for SyncRefCell<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

impl<T: Clone> Clone for SyncRefCell<T> {
    fn clone(&self) -> Self {
        Self::new_in(self.namespace(), self.borrow().clone())
    }
}

impl<T> SyncRefCell<T> {
    //> Constructors
    pub const fn new_in(namespace: Namespace, value: T) -> Self {
        Self {
            state: CellState::new(namespace),
            value: UnsafeCell::new(value),
        }
    }

    pub fn new(value: T) -> Self {
        Self::new_in(MAIN_THREAD.as_namespace(), value)
    }

    //> Zero-cost borrows
    pub fn into_inner(self) -> T {
        self.value.into_inner()
    }

    //> Borrowing helpers
    pub fn replace(&self, t: T) -> T {
        std::mem::replace(&mut *self.borrow_mut(), t)
    }

    pub fn replace_with<F>(&self, f: F) -> T
    where
        F: FnOnce(&mut T) -> T,
    {
        let mut guard = self.borrow_mut();
        let t = f(&mut *guard);
        std::mem::replace(&mut *guard, t)
    }

    pub fn swap(&self, other: &RefCell<T>) {
        std::mem::swap(&mut *self.borrow_mut(), &mut *other.borrow_mut())
    }

    pub fn take(&self) -> T
    where
        T: Default,
    {
        std::mem::take(&mut *self.borrow_mut())
    }
}

impl<T: ?Sized> SyncRefCell<T> {
    //> Namespace modification
    pub fn namespace(&self) -> Namespace {
        self.state.namespace()
    }

    pub fn set_namespace(&self, namespace: Namespace) {
        self.state.set_namespace(namespace);
    }

    pub fn set_namespace_mut(&mut self, namespace: Namespace) {
        self.state.set_namespace_mut(namespace);
    }

    pub unsafe fn set_namespace_unchecked(&self, namespace: Namespace) {
        self.state.set_namespace_unchecked(namespace)
    }

    //> Zero-cost borrows
    pub fn as_ptr(&self) -> *mut T {
        self.value.get()
    }

    pub fn as_non_null(&self) -> NonNull<T> {
        NonNull::new(self.as_ptr()).unwrap()
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.value.get_mut()
    }

    //> Regular borrows
    pub fn borrow(&self) -> SyncRef<T> {
        self.state.lock_immutable();

        SyncRef {
            value: self.as_non_null(),
            borrow: &self.state,
        }
    }

    pub fn borrow_mut(&self) -> SyncMut<T> {
        self.state.lock_mutable();

        SyncMut {
            _invariant: PhantomData,
            value: self.as_non_null(),
            borrow: &self.state,
        }
    }
}

pub struct SyncRef<'b, T: ?Sized> {
    // This ensures both that `T` is covariant and that `SyncRef: !Send` and `!Sync`.
    value: NonNull<T>,
    borrow: &'b CellState,
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for SyncRef<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T: ?Sized + fmt::Display> fmt::Display for SyncRef<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

impl<'b, T: ?Sized> SyncRef<'b, T> {
    pub fn clone(orig: &SyncRef<'b, T>) -> SyncRef<'b, T> {
        orig.borrow.relock_immutable();

        SyncRef {
            value: orig.value,
            borrow: orig.borrow,
        }
    }

    pub fn map<U, F>(orig: SyncRef<'b, T>, f: F) -> SyncRef<'b, U>
    where
        F: FnOnce(&T) -> &U,
        U: ?Sized,
    {
        let value = NonNull::from(f(&*orig));
        let borrow = orig.borrow;
        std::mem::forget(orig);

        SyncRef { value, borrow }
    }

    pub fn filter_map<U, F>(orig: SyncRef<'b, T>, f: F) -> Result<SyncRef<'b, U>, SyncRef<'b, T>>
    where
        F: FnOnce(&T) -> Option<&U>,
        U: ?Sized,
    {
        let Some(value) = f(&*orig) else {
			return Err(orig);
		};

        let value = NonNull::from(value);
        let borrow = orig.borrow;
        std::mem::forget(orig);

        Ok(SyncRef { value, borrow })
    }
}

impl<T: ?Sized> Deref for SyncRef<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.value.as_ref() }
    }
}

impl<T: ?Sized> Drop for SyncRef<'_, T> {
    fn drop(&mut self) {
        self.borrow.unlock_immutable();
    }
}

pub struct SyncMut<'b, T: ?Sized> {
    // This ensures that `T` is invariant.
    _invariant: PhantomData<&'b mut T>,
    // This ensures that `SyncRef: !Send` and `!Sync`.
    value: NonNull<T>,
    borrow: &'b CellState,
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for SyncMut<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T: ?Sized + fmt::Display> fmt::Display for SyncMut<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

impl<'b, T: ?Sized> SyncMut<'b, T> {
    pub fn map<U, F>(mut orig: SyncMut<'b, T>, f: F) -> SyncMut<'b, U>
    where
        F: FnOnce(&mut T) -> &mut U,
        U: ?Sized,
    {
        let value = NonNull::from(f(&mut *orig));
        let borrow = orig.borrow;
        std::mem::forget(orig);

        SyncMut {
            _invariant: PhantomData,
            value,
            borrow,
        }
    }

    pub fn filter_map<U, F>(
        mut orig: SyncMut<'b, T>,
        f: F,
    ) -> Result<SyncMut<'b, U>, SyncMut<'b, T>>
    where
        F: FnOnce(&mut T) -> Option<&mut U>,
        U: ?Sized,
    {
        let Some(value) = f(&mut *orig) else {
			return Err(orig);
		};

        let value = NonNull::from(value);
        let borrow = orig.borrow;
        std::mem::forget(orig);

        Ok(SyncMut {
            _invariant: PhantomData,
            value,
            borrow,
        })
    }
}

impl<T: ?Sized> Deref for SyncMut<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.value.as_ref() }
    }
}

impl<T: ?Sized> DerefMut for SyncMut<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.value.as_mut() }
    }
}

impl<T: ?Sized> Drop for SyncMut<'_, T> {
    fn drop(&mut self) {
        self.borrow.unlock_mutable();
    }
}
