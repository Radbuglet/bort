//! Mechanisms to exclusively or cooperatively acquire a set of instances on a thread and prove one's
//! access to them using tokens.
//!
//! ## Borrowing
//!
//! Fundamentally, tokens regulate which threads have access to which `Type` + [`Namespace`] pairs,
//! which are frequently called "component sets" in this documentation.
//!
//! Bort has the notion of a main thread. This thread is decided automatically as the first thread to
//! call to one of this module's methods. Main threads are assumed to have access to all values in
//! the application and thus, code written for the main thread can pretend that it is singlethreaded.
//!
//! One may suspend the main thread using [`MainThreadToken::parallelize`], which allows users to
//! run code on "worker threads." Worker threads can obtain more fine-grained access to different
//! component sets using the various methods available on the [`ParallelTokenSource`] instance provided
//! by `parallelize`.
//!
//! A component set can be acquired in one of two ways:
//!
//! - Exclusive access, which means that only one thread has access to a given component at a time and
//!   can thus borrow any value in the set mutably.
//!
//! - Shared access,which means that multiple threads have access to the same component set
//!   simultaneously, but may only borrow these values immutably.
//!
//! To prove the current thread's access to a given value, one passes in a reference to a token
//! object. For example, the main thread can acquire a [`MainThreadToken`] while worker threads can
//! acquire [`TypeExclusiveToken`]s and [`TypeSharedToken`]s, which are produced by the
//! [`ParallelTokenSource`].
//!
//! ## Jailing
//!
//! Additionally, tokens come with the notion of a "main thread jail." Values which are non-`Sync`
//! can be jailed on the main thread, meaning that the wrapper cell itself can be made `Sync` but the
//! inner, potentially dangerous value, can only be access on the main thread. See [`UnJailRefToken`]
//! and [`UnJailMutToken`] for details.
//!
//! ## Edge-cases
//!
//! It is important to note that there is a different between worker threads and regular external
//! threads. Indeed, one may run a regular thread and the main thread simultaneously but one may not
//! run a worker thread and the main thread simultaneously. This is important for objects synchronized
//! against the main thread rather than against a component set. Methods on these objects will
//! consume a blank [`Token`] which seemingly affords no guarantees but, in actuality, affords the
//! guarantee that the token is belongs to either a worker or a main thread.

use hashbrown::hash_map::Entry as HashMapEntry;
use std::{
    any::{type_name, TypeId},
    cell::Cell,
    fmt,
    marker::PhantomData,
    num::NonZeroU64,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Mutex, MutexGuard,
    },
    thread::{self, current, Thread},
};

use crate::util::{misc::unpoison, map::FxHashMap};

// === Access Token Traits === //

// Namespaces
#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct Namespace(pub(super) NonZeroU64);

impl Namespace {
    pub fn new() -> Self {
        static ALLOC: AtomicU64 = AtomicU64::new(1);

        Self(NonZeroU64::new(ALLOC.fetch_add(1, Ordering::Relaxed)).unwrap())
    }
}

impl Default for Namespace {
    fn default() -> Self {
        Self::new()
    }
}

// Token Type Markers
mod sealed {
    pub trait TokenKindMarker: Sized {}
}

use sealed::TokenKindMarker;

/// A marker type indicating that the token exists on the main thread.
pub struct MainThreadTokenKind {
    _private: (),
}

impl TokenKindMarker for MainThreadTokenKind {}

/// A marker type indicating that the token exists on either the main thread or the worker thread.
pub struct WorkerOrMainThreadTokenKind {
    _private: (),
}

impl TokenKindMarker for WorkerOrMainThreadTokenKind {}

// Access Tokens

/// The type of access a token grants to a given value type.
#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub enum ThreadAccess {
    Exclusive,
    Shared,
}

/// A marker trait indicating the type of a token. See the module documentation for more information
/// on tokens.
///
/// ## Safety
///
/// - If `Kind` is specified as [`MainThreadTokenKind`], this token may only exist on the main
/// thread.
///
/// - If `Kind` is specified as [`WorkerOrMainThreadTokenKind`], this token is either a main thread
///   token or the main thread has been suspended and this is a *real* worker thread.
///
pub unsafe trait Token: Sized + fmt::Debug {
    type Kind: TokenKindMarker;
}

/// Specifies the token's access permissions to a given type `T`. Note that the presence of this trait
/// alone does not make any access guarantees—the `check_access` method must be called to determine
/// actual access levels.
///
/// See the module documentation for more information on tokens.
///
/// ## Safety
///
/// - Thread access rules must be upheld by implementations of this trait.
/// - If `check_access` ever guarantees access to `T` under a given namespace, this access must not
///   be revoked or invalidated until the token instance from which the guarantee was acquired expires.
///
pub unsafe trait TokenFor<T: ?Sized>: Token {
    fn check_access(&self, namespace: Option<Namespace>) -> Option<ThreadAccess>;
}

/// Specifies that a token *may* have shared access to the given type `T`. Unlike [`TokenFor`], this
/// trait is not `unsafe` and, indeed, can only be relied on for convenient—not safety. To validate
/// that the token does actually provide shared access to `T`, one must call [`TokenFor::check_access`]
/// on the appropriate namespace and check its result at runtime.
///
/// See the module documentation for more information on tokens.
pub trait SharedTokenHint<T: ?Sized>: TokenFor<T> {}

/// Specifies that a token *may* have exclusive access to the given type `T`. Unlike [`TokenFor`], this
/// trait is not `unsafe` and, indeed, can only be relied on for convenient—not safety. To validate
/// that the token does actually provide exclusive access to `T`, one must call [`TokenFor::check_access`]
/// on the appropriate namespace and check its result at runtime.
///
/// See the module documentation for more information on tokens.
pub trait ExclusiveTokenHint<T: ?Sized>: TokenFor<T> {}

// === Unjailing Token Traits === //

/// Many token-based cells in [`token_cell`](super::token_cell) implement an "unjailing" system where
/// users can only get references to non-`Sync` values on the main thread. This trait asserts that an
/// immutable access to `T` is safe on the current thread given the token—see the safety section for a
/// more precise formulation of this trait's guarantees.
///
/// ## Safety
///
/// This trait cannot be implemented directly — its implementation is derived from one's implementation
/// of [`Token`].
///
/// If implemented on a token, this trait asserts that either:
///
/// - this token is a main thread token
/// - the value `T` is `Sync` and can therefore be safely acquired immutably from several threads
///   at once
///
///
pub unsafe trait UnJailRefToken<T: ?Sized>: Token {}

/// Many token-based cells in [`token_cell`](super::token_cell) implement an "unjailing" system where
/// users can only get references to non-`Sync` values on the main thread. This trait asserts that a
/// mutable access to `T` is safe on the current thread given the token—see the safety section for a
/// more precise formulation of this trait's guarantees.
///
/// ## Safety
///
/// This trait cannot be implemented directly — its implementation is derived from one's implementation
/// of [`Token`].
///
/// If implemented on a token, this trait asserts that either:
///
/// - this token is a main thread token
/// - the value `T` is `Send` and can therefore be safely acquired mutably from the current thread
///
pub unsafe trait UnJailMutToken<T: ?Sized>: Token {}

mod unjail_impl {
    use super::*;

    pub unsafe trait UnJailRefTokenDisamb<T: ?Sized, Kind: TokenKindMarker> {}

    pub unsafe trait UnJailMutTokenDisamb<T: ?Sized, Kind: TokenKindMarker> {}

    // Safety: if `V: !Sync`, `V` can only be read on the main thread, so there is no risk of a race
    // condition.
    unsafe impl<T: Token<Kind = MainThreadTokenKind>, V: ?Sized>
        UnJailRefTokenDisamb<V, MainThreadTokenKind> for T
    {
    }

    unsafe impl<T: Token<Kind = MainThreadTokenKind>, V: ?Sized>
        UnJailMutTokenDisamb<V, MainThreadTokenKind> for T
    {
    }

    unsafe impl<T: Token<Kind = WorkerOrMainThreadTokenKind>, V: ?Sized + Sync>
        UnJailRefTokenDisamb<V, WorkerOrMainThreadTokenKind> for T
    {
    }

    unsafe impl<T: Token<Kind = WorkerOrMainThreadTokenKind>, V: ?Sized + Send>
        UnJailMutTokenDisamb<V, WorkerOrMainThreadTokenKind> for T
    {
    }

    unsafe impl<T: Token + UnJailRefTokenDisamb<V, T::Kind>, V: ?Sized> UnJailRefToken<V> for T {}

    unsafe impl<T: Token + UnJailMutTokenDisamb<V, T::Kind>, V: ?Sized> UnJailMutToken<V> for T {}
}

// === Token Trait Aliases === //

/// An alias for [`ExclusiveTokenHint<T>`] and [`UnJailRefToken<T>`].
pub trait BorrowToken<T: ?Sized>: ExclusiveTokenHint<T> + UnJailRefToken<T> {}

impl<T: ?Sized + ExclusiveTokenHint<V> + UnJailRefToken<V>, V: ?Sized> BorrowToken<V> for T {}

/// An alias for [`ExclusiveTokenHint<T>`] and [`UnJailMutToken<T>`].
pub trait BorrowMutToken<T: ?Sized>: ExclusiveTokenHint<T> + UnJailMutToken<T> {}

impl<T: ?Sized + ExclusiveTokenHint<V> + UnJailMutToken<V>, V: ?Sized> BorrowMutToken<V> for T {}

/// An alias for [`SharedTokenHint<T>`] and [`UnJailRefToken<T>`].
pub trait GetToken<T: ?Sized>: SharedTokenHint<T> + UnJailRefToken<T> {}

impl<T: ?Sized + SharedTokenHint<V> + UnJailRefToken<V>, V: ?Sized> GetToken<V> for T {}

// === Blessing === //

static HAS_MAIN_THREAD: AtomicBool = AtomicBool::new(false);

thread_local! {
    static IS_MAIN_THREAD: Cell<bool> = const { Cell::new(false) };
}

pub fn is_main_thread() -> bool {
    IS_MAIN_THREAD.with(|v| v.get())
}

#[must_use]
pub fn try_become_main_thread() -> bool {
    if is_main_thread() {
        return true;
    }

    if HAS_MAIN_THREAD
        .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
    {
        IS_MAIN_THREAD.with(|v| v.set(true));
        true
    } else {
        false
    }
}

pub(crate) fn ensure_main_thread(action: impl fmt::Display) {
    assert!(
        try_become_main_thread(),
        "{action} on non-main thread. See the \"multi-threading\" section of \
		 the module documentation for details.",
    );
}

// === MainThreadToken === //

#[derive(Debug, Copy, Clone)]
pub struct MainThreadToken {
    _no_send_or_sync: PhantomData<*const ()>,
}

impl Default for MainThreadToken {
    fn default() -> Self {
        *Self::acquire()
    }
}

impl MainThreadToken {
    pub fn try_acquire() -> Option<&'static Self> {
        if try_become_main_thread() {
            Some(&Self {
                _no_send_or_sync: PhantomData,
            })
        } else {
            None
        }
    }

    pub fn acquire() -> &'static Self {
        ensure_main_thread("Attempted to acquire MainThreadToken");

        &Self {
            _no_send_or_sync: PhantomData,
        }
    }

    pub fn exclusive_token<T: ?Sized + 'static>(&self) -> TypeExclusiveToken<'static, T> {
        TypeExclusiveToken {
            _no_send_or_sync: PhantomData,
            _ty: PhantomData,
            session: None,
        }
    }

    pub fn parallelize<F, R>(&self, f: F) -> R
    where
        F: Send + FnOnce(&mut ParallelTokenSource) -> R,
        R: Send,
    {
        let mut result = None;

        thread::scope(|s| {
            // Spawn a new thread and join it immediately.
            //
            // This ensures that, while borrows originating from our `MainThreadToken`
            // could still be ongoing, they will never be acted upon until the
            // "reborrowing" tokens expire, which live for at most the lifetime of
            // `ParallelTokenSource`.
            s.spawn(|| {
                result = Some(f(&mut ParallelTokenSource {
                    borrows: Default::default(),
                }));
            });
        });

        result.unwrap()
    }
}

unsafe impl Token for MainThreadToken {
    type Kind = MainThreadTokenKind;
}

unsafe impl<T: ?Sized> TokenFor<T> for MainThreadToken {
    fn check_access(&self, _namespace: Option<Namespace>) -> Option<ThreadAccess> {
        Some(ThreadAccess::Exclusive)
    }
}

impl<T: ?Sized> ExclusiveTokenHint<T> for MainThreadToken {}

// === ParallelTokenSource === //

const TOO_MANY_EXCLUSIVE_ERR: &str = "too many TypeExclusiveTokens!";
const TOO_MANY_READ_ERR: &str = "too many TypeSharedTokens!";

type BorrowMap = FxHashMap<(TypeId, Option<Namespace>), (isize, Option<Thread>)>;

#[derive(Debug)]
pub struct ParallelTokenSource {
    borrows: Mutex<BorrowMap>,
}

impl ParallelTokenSource {
    fn borrows(&self) -> MutexGuard<BorrowMap> {
        unpoison(self.borrows.lock())
    }

    pub fn exclusive_token<T: ?Sized + 'static>(&self) -> TypeExclusiveToken<'_, T> {
        // Increment reference count
        match self.borrows().entry((TypeId::of::<T>(), None)) {
            HashMapEntry::Occupied(mut entry) => {
                let (rc, Some(owner)) = entry.get_mut() else {
					unreachable!();
				};

                // Validate thread ID
                let ty_name = type_name::<T>();
                let current = current();

                assert_eq!(
                    owner.id(),
                    current.id(),
                    "Cannot acquire a `TypeExclusiveToken<{ty_name}>` to a type already acquired by
					 the thread {owner:?} (current thread: {current:?})",
                );

                // Increment rc
                *rc = rc.checked_add(1).expect(TOO_MANY_EXCLUSIVE_ERR);
            }
            HashMapEntry::Vacant(entry) => {
                entry.insert((1, Some(current())));
            }
        }

        // Construct token
        TypeExclusiveToken {
            _no_send_or_sync: PhantomData,
            _ty: PhantomData,
            session: Some(self),
        }
    }

    pub fn read_token<T: ?Sized + 'static>(&self) -> TypeSharedToken<'_, T> {
        // Increment reference count
        match self.borrows().entry((TypeId::of::<T>(), None)) {
            HashMapEntry::Occupied(mut entry) => {
                let (rc, owner) = entry.get_mut();
                debug_assert!(owner.is_none());

                // Decrement rc
                *rc = rc.checked_sub(1).expect(TOO_MANY_READ_ERR);
            }
            HashMapEntry::Vacant(entry) => {
                entry.insert((-1, None));
            }
        }

        // Construct token
        TypeSharedToken {
            _ty: PhantomData,
            session: self,
        }
    }

    // TODO: Implement namespaced token acquisition
}

// === TypeExclusiveToken === //

pub struct TypeExclusiveToken<'a, T: ?Sized + 'static> {
    _no_send_or_sync: PhantomData<*const ()>,
    _ty: PhantomData<fn() -> T>,
    session: Option<&'a ParallelTokenSource>,
}

impl<T: ?Sized + 'static> fmt::Debug for TypeExclusiveToken<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TypeExclusiveToken")
            .field("session", &self.session)
            .finish_non_exhaustive()
    }
}

impl<T: ?Sized + 'static> Clone for TypeExclusiveToken<'_, T> {
    fn clone(&self) -> Self {
        if let Some(session) = self.session {
            let mut borrows = session.borrows();
            let (rc, _) = borrows.get_mut(&(TypeId::of::<T>(), None)).unwrap();
            *rc = rc.checked_add(1).expect(TOO_MANY_EXCLUSIVE_ERR);
        }

        Self {
            _no_send_or_sync: PhantomData,
            _ty: PhantomData,
            session: self.session,
        }
    }
}

impl<T: ?Sized + 'static> Drop for TypeExclusiveToken<'_, T> {
    fn drop(&mut self) {
        if let Some(session) = self.session {
            let mut borrows = session.borrows();
            let HashMapEntry::Occupied(mut entry) = borrows.entry((TypeId::of::<T>(), None)) else {
				unreachable!()
			};

            let (rc, _) = entry.get_mut();
            *rc -= 1;
            if *rc == 0 {
                entry.remove();
            }
        }
    }
}

unsafe impl<T: ?Sized + 'static> Token for TypeExclusiveToken<'_, T> {
    type Kind = WorkerOrMainThreadTokenKind;
}

unsafe impl<T: ?Sized + 'static> TokenFor<T> for TypeExclusiveToken<'_, T> {
    fn check_access(&self, _namespace: Option<Namespace>) -> Option<ThreadAccess> {
        Some(ThreadAccess::Exclusive)
    }
}

impl<T: ?Sized + 'static> ExclusiveTokenHint<T> for TypeExclusiveToken<'_, T> {}

// === TypeSharedToken === //

pub struct TypeSharedToken<'a, T: ?Sized + 'static> {
    _ty: PhantomData<fn() -> T>,
    session: &'a ParallelTokenSource,
}

impl<T: ?Sized + 'static> fmt::Debug for TypeSharedToken<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TypeSharedToken")
            .field("session", &self.session)
            .finish_non_exhaustive()
    }
}

impl<T: ?Sized + 'static> Clone for TypeSharedToken<'_, T> {
    fn clone(&self) -> Self {
        let mut borrows = self.session.borrows();
        let (rc, _) = borrows.get_mut(&(TypeId::of::<T>(), None)).unwrap();
        *rc = rc.checked_sub(1).expect(TOO_MANY_READ_ERR);

        Self {
            _ty: PhantomData,
            session: self.session,
        }
    }
}

impl<T: ?Sized + 'static> Drop for TypeSharedToken<'_, T> {
    fn drop(&mut self) {
        let mut borrows = self.session.borrows();
        let HashMapEntry::Occupied(mut entry) = borrows.entry((TypeId::of::<T>(), None)) else {
			unreachable!()
		};

        let (rc, _) = entry.get_mut();
        *rc += 1;
        if *rc == 0 {
            entry.remove();
        }
    }
}

unsafe impl<T: ?Sized + 'static> Token for TypeSharedToken<'_, T> {
    type Kind = WorkerOrMainThreadTokenKind;
}

unsafe impl<T: ?Sized + 'static> TokenFor<T> for TypeSharedToken<'_, T> {
    fn check_access(&self, _namespace: Option<Namespace>) -> Option<ThreadAccess> {
        Some(ThreadAccess::Shared)
    }
}

impl<T: ?Sized + 'static> SharedTokenHint<T> for TypeSharedToken<'_, T> {}
