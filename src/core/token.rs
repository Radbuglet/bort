//! Mechanisms to exclusively or cooperatively acquire a set of instances on a thread and prove one's
//! access to them using tokens.

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

use super::cell::{BorrowError, BorrowMutError, OptRef, OptRefCell, OptRefMut};
use crate::util::{unpoison, FxHashMap, NOT_ON_MAIN_THREAD_MSG};

// === Access Token Traits === //

// Namespaces
#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct Namespace(NonZeroU64);

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

pub struct MainThreadTokenKind {
    _private: (),
}

impl TokenKindMarker for MainThreadTokenKind {}

pub struct WorkerOrMainThreadTokenKind {
    _private: (),
}

impl TokenKindMarker for WorkerOrMainThreadTokenKind {}

// Access Tokens
#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub enum ThreadAccess {
    Exclusive,
    Shared,
}

pub unsafe trait Token: Sized {
    type Kind: TokenKindMarker;
}

pub unsafe trait TokenFor<T: ?Sized>: fmt::Debug + Token {
    fn check_access(&self, namespace: Option<Namespace>) -> Option<ThreadAccess>;
}

pub trait ReadTokenHint<T: ?Sized>: TokenFor<T> {}

pub trait ExclusiveTokenHint<T: ?Sized>: TokenFor<T> {}

// === Unjailing Token Traits === //

/// Both [`NOptRefCell`] and [`MainThreadJail`] implement an "unjailing" system where users can only
/// get references to non-`Sync` values on the main thread. This trait asserts that a given token can
/// get an immutable reference to the specified type `T` given an immutable potentially-cross-thread
/// reference to the owner assuming all other accesses were also mediated by this trait.
///
/// This trait cannot be implemented directly — its implementation is derived from one's implementation
/// of [`Token`].
pub unsafe trait UnJailRefToken<T: ?Sized>: Token {}

/// Both [`NOptRefCell`] and [`MainThreadJail`] implement an "unjailing" system where users can only
/// get references to non-`Sync` values on the main thread.  This trait asserts that a given token can
/// get a mutable reference to the specified type `T` given an immutable potentially-cross-thread
/// reference to the owner assuming all other accesses were also mediated by this trait.
///
/// This trait cannot be implemented directly — its implementation is derived from one's implementation
/// of [`Token`].
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

pub trait BorrowToken<T: ?Sized>: ExclusiveTokenHint<T> + UnJailRefToken<T> {}

impl<T: ?Sized + ExclusiveTokenHint<V> + UnJailRefToken<V>, V: ?Sized> BorrowToken<V> for T {}

pub trait BorrowMutToken<T: ?Sized>: ExclusiveTokenHint<T> + UnJailMutToken<T> {}

impl<T: ?Sized + ExclusiveTokenHint<V> + UnJailMutToken<V>, V: ?Sized> BorrowMutToken<V> for T {}

pub trait GetToken<T: ?Sized>: ReadTokenHint<T> + UnJailRefToken<T> {}

impl<T: ?Sized + ReadTokenHint<V> + UnJailRefToken<V>, V: ?Sized> GetToken<V> for T {}

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
const TOO_MANY_READ_ERR: &str = "too many TypeReadTokens!";

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

    pub fn read_token<T: ?Sized + 'static>(&self) -> TypeReadToken<'_, T> {
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
        TypeReadToken {
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

// === TypeReadToken === //

pub struct TypeReadToken<'a, T: ?Sized + 'static> {
    _ty: PhantomData<fn() -> T>,
    session: &'a ParallelTokenSource,
}

impl<T: ?Sized + 'static> fmt::Debug for TypeReadToken<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TypeReadToken")
            .field("session", &self.session)
            .finish_non_exhaustive()
    }
}

impl<T: ?Sized + 'static> Clone for TypeReadToken<'_, T> {
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

impl<T: ?Sized + 'static> Drop for TypeReadToken<'_, T> {
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

unsafe impl<T: ?Sized + 'static> Token for TypeReadToken<'_, T> {
    type Kind = WorkerOrMainThreadTokenKind;
}

unsafe impl<T: ?Sized + 'static> TokenFor<T> for TypeReadToken<'_, T> {
    fn check_access(&self, _namespace: Option<Namespace>) -> Option<ThreadAccess> {
        Some(ThreadAccess::Shared)
    }
}

impl<T: ?Sized + 'static> ReadTokenHint<T> for TypeReadToken<'_, T> {}

// === MainThreadJail === //

#[derive(Default)]
pub struct MainThreadJail<T: ?Sized>(T);

impl<T: ?Sized + fmt::Debug> fmt::Debug for MainThreadJail<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if is_main_thread() {
            f.debug_tuple("MainThreadJail").field(&&self.0).finish()
        } else {
            f.debug_tuple("MainThreadJail")
                .field(&NOT_ON_MAIN_THREAD_MSG)
                .finish()
        }
    }
}

// Safety: you can only get a reference to `T` if you're on the un-jailing main thread or if `T` is,
// itself, `Sync`.
unsafe impl<T> Sync for MainThreadJail<T> {}

impl<T> MainThreadJail<T> {
    pub const fn new(value: T) -> Self {
        MainThreadJail(value)
    }

    pub fn into_inner(self) -> T {
        self.0
    }

    pub fn get(&self, _token: &impl UnJailRefToken<T>) -> &T {
        &self.0
    }

    pub fn get_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

// === NOptRefCell === //

pub struct NOptRefCell<T> {
    namespace: AtomicU64,
    value: OptRefCell<T>,
}

// FIXME: This is probably unsound.
unsafe impl<T> Sync for NOptRefCell<T> {}

impl<T> NOptRefCell<T> {
    // === Constructors === //

    pub fn new(value: Option<T>) -> Self {
        match value {
            Some(value) => Self::new_full(value),
            None => Self::new_empty(),
        }
    }

    pub fn new_namespaced(value: Option<T>, namespace: Option<Namespace>) -> Self {
        match value {
            Some(value) => Self::new_namespaced_full(value, namespace),
            None => Self::new_namespaced_empty(namespace),
        }
    }

    pub const fn new_full(value: T) -> Self {
        Self::new_namespaced_full(value, None)
    }

    pub const fn new_empty() -> Self {
        Self::new_namespaced_empty(None)
    }

    pub const fn new_namespaced_full(value: T, namespace: Option<Namespace>) -> Self {
        Self {
            value: OptRefCell::new_full(value),
            namespace: AtomicU64::new(match namespace {
                Some(Namespace(id)) => id.get(),
                None => 0,
            }),
        }
    }

    pub const fn new_namespaced_empty(namespace: Option<Namespace>) -> Self {
        Self {
            value: OptRefCell::new_empty(),
            namespace: AtomicU64::new(match namespace {
                Some(Namespace(id)) => id.get(),
                None => 0,
            }),
        }
    }

    // === Zero-cost conversions === //

    pub fn into_inner(self) -> Option<T> {
        // Safety: this is a method that takes exclusive access to the object. Hence, it is
        // not impacted by our potentially dangerous `Sync` impl.
        self.value.into_inner()
    }

    pub fn get_mut(&mut self) -> Option<&mut T> {
        // Safety: this is a method that takes exclusive access to the object. Hence, it is
        // not impacted by our potentially dangerous `Sync` impl.
        self.value.get_mut()
    }

    pub fn as_ptr(&self) -> *mut T {
        self.value.as_ptr()
    }

    pub fn is_empty(&self, token: &impl TokenFor<T>) -> bool {
        self.assert_accessible_by(token, None);

        // Safety: we have either shared or exclusive access to this token and, in both cases, we
        // know that `state` cannot be mutated until the token expires thus precluding a race
        // condition.
        self.value.is_empty()
    }

    pub fn is_empty_mut(&mut self) -> bool {
        // Safety: this is a method that takes exclusive access to the object. Hence, it is
        // not impacted by our potentially dangerous `Sync` impl.
        self.value.is_empty()
    }

    pub fn set(&mut self, value: Option<T>) -> Option<T> {
        // Safety: this is a method that takes exclusive ownership of the object. Hence, it is
        // not impacted by our potentially dangerous `Sync` impl.
        self.value.set(value)
    }

    pub fn undo_leak(&mut self) {
        // Safety: this is a method that takes exclusive ownership of the object. Hence, it is
        // not impacted by our potentially dangerous `Sync` impl.
        self.value.undo_leak()
    }

    // === Namespace management === //

    pub fn namespace(&self) -> Option<Namespace> {
        NonZeroU64::new(self.namespace.load(Ordering::Relaxed)).map(Namespace)
    }

    fn assert_accessible_by(&self, token: &impl TokenFor<T>, access: Option<ThreadAccess>) {
        let owner = self.namespace();
        let can_access = match access {
            Some(access) => token.check_access(owner) == Some(access),
            None => token.check_access(owner).is_some(),
        };

        assert!(
            can_access,
            "{token:?} cannot access NOptRefCell under namespace {owner:?}.",
        );
    }

    pub fn set_namespace(&mut self, namespace: Option<Namespace>) {
        *self.namespace.get_mut() = namespace.map_or(0, |Namespace(id)| id.get());
    }

    pub fn set_namespace_ref(
        &self,
        token: &impl ExclusiveTokenHint<T>,
        namespace: Option<Namespace>,
    ) {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: we can read and mutate the `value`'s borrow count safely because we are the
        // only thread with "write" namespace access and we know that fact will not change during
        // this operation because *this is the operation we're using to change that fact!*
        match self.value.try_borrow_mut() {
            Ok(_) => {}
            Err(err) => {
                panic!(
                    "Failed to release NOptRefCell from namespace {:?}; dynamic borrows are still \
				     ongoing: {err}",
                    self.namespace(),
                );
            }
        }

        // Safety: It is forget our namespace because all write tokens on this thread acting on
        // this object have relinquished their dynamic borrows.
        self.namespace.store(
            match namespace {
                Some(Namespace(id)) => id.get(),
                None => 0,
            },
            Ordering::Relaxed,
        );
    }

    // === Borrowing === //

    pub fn try_get<'a>(
        &'a self,
        token: &'a impl GetToken<T>,
    ) -> Result<Option<&'a T>, BorrowError> {
        self.assert_accessible_by(token, Some(ThreadAccess::Shared));

        // Safety: we know we can read from the `value`'s borrow count safely because this method
        // can only be run so long as we have `ReadToken`s alive and we can't cause reads
        // until we get a `ExclusiveToken`s, which is only possible once `token` is dead.
        //
        // We know that it is safe to give this thread immutable access to `T` because `UnJailRefToken`
        // asserts as much.
        unsafe {
            // Safety: additionally, we know nobody can borrow this cell mutably until all
            // `ReadToken`s die out so this is safe as well.
            self.value.try_borrow_unguarded()
        }
    }

    pub fn get_or_none<'a>(&'a self, token: &'a impl GetToken<T>) -> Option<&'a T> {
        self.assert_accessible_by(token, Some(ThreadAccess::Shared));

        unsafe {
            // Safety: see `try_get`
            self.value.borrow_unguarded_or_none()
        }
    }

    pub fn get<'a>(&'a self, token: &'a impl GetToken<T>) -> &'a T {
        self.assert_accessible_by(token, Some(ThreadAccess::Shared));

        unsafe {
            // Safety: see `try_get`
            self.value.borrow_unguarded()
        }
    }

    pub fn try_borrow<'a>(
        &'a self,
        token: &'a impl BorrowToken<T>,
    ) -> Result<Option<OptRef<'a, T>>, BorrowError> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: we know we can read and write from the `value`'s borrow count safely because
        // this method can only be run so long as we have `ExclusiveToken`s alive and we
        // can't...
        //
        // a) construct `ReadToken`s to use on other threads until `token` dies out
        // b) move this `token` to another thread because it's neither `Send` nor `Sync`
        // c) change the owner to admit new namespaces on other threads until all the borrows
        //    expire.
        //
        // We know that it is safe to give this thread access to `T` because `UnJailXyzToken`
        // asserts as much.
        self.value.try_borrow()
    }

    pub fn borrow_or_none<'a>(&'a self, token: &'a impl BorrowToken<T>) -> Option<OptRef<'a, T>> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.borrow_or_none()
    }

    pub fn borrow<'a>(&'a self, token: &'a impl BorrowToken<T>) -> OptRef<'a, T> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.borrow()
    }

    pub fn try_borrow_mut<'a>(
        &'a self,
        token: &'a impl BorrowMutToken<T>,
    ) -> Result<Option<OptRefMut<'a, T>>, BorrowMutError> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.try_borrow_mut()
    }

    pub fn borrow_mut_or_none<'a>(
        &'a self,
        token: &'a impl BorrowMutToken<T>,
    ) -> Option<OptRefMut<'a, T>> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.borrow_mut_or_none()
    }

    pub fn borrow_mut<'a>(&'a self, token: &'a impl BorrowMutToken<T>) -> OptRefMut<'a, T> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.borrow_mut()
    }

    // === Replace === //

    pub fn try_replace_with<F>(
        &self,
        token: &impl BorrowMutToken<T>,
        f: F,
    ) -> Result<Option<T>, BorrowMutError>
    where
        F: FnOnce(Option<&mut T>) -> Option<T>,
    {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.try_replace_with(f)
    }

    pub fn replace_with<F>(&self, token: &impl BorrowMutToken<T>, f: F) -> Option<T>
    where
        F: FnOnce(Option<&mut T>) -> Option<T>,
    {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.replace_with(f)
    }

    pub fn try_replace(
        &self,
        token: &impl BorrowMutToken<T>,
        t: Option<T>,
    ) -> Result<Option<T>, BorrowMutError> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.try_replace(t)
    }

    pub fn replace(&self, token: &impl BorrowMutToken<T>, t: Option<T>) -> Option<T> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.replace(t)
    }

    pub fn take(&self, token: &impl BorrowMutToken<T>) -> Option<T> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.take()
    }

    pub fn swap_multi_token(
        &self,
        my_token: &impl BorrowMutToken<T>,
        other_token: &impl BorrowMutToken<T>,
        other: &NOptRefCell<T>,
    ) {
        self.assert_accessible_by(my_token, Some(ThreadAccess::Exclusive));
        other.assert_accessible_by(other_token, Some(ThreadAccess::Exclusive));

        self.value.swap(&other.value)
    }

    pub fn swap(&self, token: &impl BorrowMutToken<T>, other: &NOptRefCell<T>) {
        self.swap_multi_token(token, token, other)
    }
}

impl<T: fmt::Debug> fmt::Debug for NOptRefCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if is_main_thread() {
            f.debug_struct("NOptRefCell")
                .field("namespace", &self.namespace())
                .field("value", &self.value)
                .finish()
        } else {
            f.debug_struct("NOptRefCell")
                .field("namespace", &self.namespace())
                .field("value", &NOT_ON_MAIN_THREAD_MSG)
                .finish()
        }
    }
}

impl<T> Default for NOptRefCell<T> {
    fn default() -> Self {
        Self::new(None)
    }
}
