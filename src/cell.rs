// TODO: Code-review

use std::{
    cell::{Cell, RefCell, UnsafeCell},
    fmt,
    marker::PhantomData,
    num::NonZeroU32,
    ops::{Deref, DerefMut},
    ptr::NonNull,
    sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, Ordering},
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

#[derive(Debug)]
struct CellState {
    // On orderings:
    // - This will not change underneath your feet during `acquire/release`
    // methods if you own the namespace.
    namespace: AtomicPtr<NamespaceState>,

    // Layout: [MSB](thread_id: u32, lock_state: u32)[LSB]
    //
    // Interpretation:
    // - `lock_state == 0`: the cell is completely unborrowed.
    //   Only the `namespace` owner is allowed to acquire the
    //   cell now.
    // - `0 < lock_state < u32::MAX`: the cell is immutably
    //   borrowed by `thread_id`. Only `thread_id` is allowed
    //   to acquire the cell now.
    // - `lock_state == u32::MAX`: the cell is mutably borrowed.
    //
    // On memory orderings:
    // - Unfortunately, we can't rely on the `Acquire` and `Release`
    //   orderings of the `Namespace` acquisition methods to properly
    //   make our changes to the interior of this cell visible to the
    //   owning thread. This is because cells can be borrowed past the
    //   namespace acquisition.
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
        Namespace(unsafe {
            // Safety: we are always pointing to a valid `&'static NamespaceState`.
            &*self.namespace.load(Ordering::Relaxed)
        })
    }

    const fn decode_state(full_state: u64) -> (u32, u32) {
        (
            (full_state >> 32) as u32,
            Self::decode_lock_state(full_state),
        )
    }

    const fn decode_lock_state(full_state: u64) -> u32 {
        full_state as u32
    }

    const fn compose_state(current_thread: u32, lock_state: u32) -> u64 {
        ((current_thread as u64) << 32) + lock_state as u64
    }

    pub fn lock_mutable(&self) {
        // First, we need to make sure that we are the logical owner of this cell.
        assert!(self.namespace().is_held(), "{}", Self::HELD_ELSEWHERE_ERROR);

        // Now, we just need to make sure that it can actually be locked.
        assert_eq!(
            // N.B. See "on memory orderings" in the item documentation.
            Self::decode_lock_state(self.state.load(Ordering::Acquire)),
            0,
            "{}",
            Self::HELD_ELSEWHERE_ERROR,
        );

        // It is now safe to update the lock state and acquire the cell.
        const FULLY_LOCKED_STATE: u64 = CellState::compose_state(0, u32::MAX);

        self.state.store(FULLY_LOCKED_STATE, Ordering::Relaxed);
    }

    pub fn lock_immutable(&self) {
        let my_thread = thread_id();
        let (using_thread, lock_state) = Self::decode_state(self.state.load(Ordering::Acquire));

        // Ensure that the borrow is valid.
        if lock_state == 0 {
            // If `lock_state == 0` and we own the lock, we can borrow freely.
            assert!(
                self.namespace().is_held_by(my_thread),
                "{}",
                Self::HELD_ELSEWHERE_ERROR
            );
        } else if lock_state < u32::MAX - 1 {
            // If the owning thread ID is ours, we can borrow this lock.
            assert_eq!(
                using_thread,
                my_thread.get(),
                "{}",
                Self::HELD_ELSEWHERE_ERROR
            );
        } else {
            // If this branch is reached, we know the lock is held mutably or that
            // we immutably borrowed it too many times.
            panic!("{}", Self::HELD_ELSEWHERE_ERROR);
        }

        // Commit it!
        self.state.store(
            Self::compose_state(my_thread.get(), lock_state + 1),
            Ordering::Relaxed,
        );
    }

    pub fn relock_immutable(&self) {
        let raw_state = self.state.load(Ordering::Relaxed);

        let (owning_thread, lock_state) = Self::decode_state(raw_state);
        debug_assert_eq!(owning_thread, thread_id().get());
        debug_assert_ne!(lock_state, 0);
        assert!(lock_state < u32::MAX - 1);

        // Lock state is in the MSB and we already checked against overflow.
        // N.B. we don't need acquire ordering because our thread already has
        // exclusive access to this cell.
        self.state.store(raw_state + 1, Ordering::Relaxed);
    }

    pub fn unlock_mutable(&self) {
        #[cfg(debug_assertions)]
        {
            let (_, lock_state) = Self::decode_state(self.state.load(Ordering::Relaxed));
            debug_assert_eq!(lock_state, u32::MAX);
        }

        // We locked it so we can unlock it.
        const FULLY_UNLOCKED_STATE: u64 = CellState::compose_state(u32::MAX, 0);

        self.state.store(FULLY_UNLOCKED_STATE, Ordering::Release);
    }

    pub fn unlock_immutable(&self) {
        let raw_state = self.state.load(Ordering::Relaxed);

        let (owning_thread, lock_state) = Self::decode_state(raw_state);
        debug_assert_eq!(owning_thread, thread_id().get());
        debug_assert_ne!(lock_state, 0);
        debug_assert!(lock_state < u32::MAX - 1);

        // Lock state is in the MSB and overflow is not possible.
        // N.B. we don't need to make any of our changes visible because we
        // only handed out an immutable reference.
        self.state.store(raw_state - 1, Ordering::Relaxed);
    }

    pub fn give_namespace(&self, namespace: Namespace) {
        assert!(self.namespace().is_held());

        unsafe {
            self.set_namespace_unchecked(namespace);
        }
    }

    pub fn set_namespace_mut(&mut self, namespace: Namespace) {
        *self.namespace.get_mut() = namespace.0 as *const NamespaceState as *mut NamespaceState;
    }

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

    pub fn give_namespace(&self, namespace: Namespace) {
        self.state.give_namespace(namespace);
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
