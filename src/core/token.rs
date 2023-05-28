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

// === Traits === //

// Namespace
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

// Access Tokens
#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub enum ThreadAccess {
    Exclusive,
    Shared,
}

pub unsafe trait Token<T: ?Sized>: fmt::Debug {
    fn check_access(&self, namespace: Option<Namespace>) -> Option<ThreadAccess>;
}

pub trait ReadTokenHint<T: ?Sized>: Token<T> {}

pub trait ExclusiveTokenHint<T: ?Sized>: Token<T> {}

// Unjailing Tokens
pub unsafe trait UnJailRefToken<T: ?Sized> {}

pub unsafe trait UnJailMutToken<T: ?Sized> {}

pub unsafe trait UnJailAllToken<T: ?Sized>: UnJailMutToken<T> + UnJailRefToken<T> {}

unsafe impl<T, U> UnJailAllToken<U> for T
where
    T: ?Sized + UnJailRefToken<U> + UnJailMutToken<U>,
    U: ?Sized,
{
}

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

unsafe impl<T: ?Sized> Token<T> for MainThreadToken {
    fn check_access(&self, _namespace: Option<Namespace>) -> Option<ThreadAccess> {
        Some(ThreadAccess::Exclusive)
    }
}

impl<T: ?Sized> ExclusiveTokenHint<T> for MainThreadToken {}

unsafe impl<T: ?Sized> UnJailRefToken<T> for MainThreadToken {}

unsafe impl<T: ?Sized> UnJailMutToken<T> for MainThreadToken {}

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

    // TODO: Implement namespaced tokens
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

unsafe impl<T: ?Sized + 'static> Token<T> for TypeExclusiveToken<'_, T> {
    fn check_access(&self, _namespace: Option<Namespace>) -> Option<ThreadAccess> {
        Some(ThreadAccess::Exclusive)
    }
}

impl<T: ?Sized + 'static> ExclusiveTokenHint<T> for TypeExclusiveToken<'_, T> {}

unsafe impl<T, U> UnJailRefToken<T> for TypeExclusiveToken<'_, U>
where
    T: ?Sized + Sync,
    U: ?Sized,
{
}

unsafe impl<T, U> UnJailMutToken<T> for TypeExclusiveToken<'_, U>
where
    T: ?Sized + Send,
    U: ?Sized,
{
}

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

unsafe impl<T: ?Sized + 'static> Token<T> for TypeReadToken<'_, T> {
    fn check_access(&self, _namespace: Option<Namespace>) -> Option<ThreadAccess> {
        Some(ThreadAccess::Shared)
    }
}

impl<T: ?Sized + 'static> ReadTokenHint<T> for TypeReadToken<'_, T> {}

unsafe impl<T, U> UnJailRefToken<T> for TypeReadToken<'_, U>
where
    T: ?Sized + Sync,
    U: ?Sized,
{
}

unsafe impl<T, U> UnJailMutToken<T> for TypeReadToken<'_, U>
where
    T: ?Sized + Send,
    U: ?Sized,
{
}

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

// Safety: you can only get a reference to `T` if you're on an un-jailing thread.
unsafe impl<T> Sync for MainThreadJail<T> {}

impl<T> MainThreadJail<T> {
    pub fn new(value: T) -> Self {
        MainThreadJail(value)
    }

    pub fn into_inner(self) -> T {
        self.0
    }

    pub fn get(&self) -> &T
    where
        T: Sync,
    {
        &self.0
    }

    pub fn get_with_unjail(&self, _: &impl UnJailRefToken<T>) -> &T {
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

// Safety: every getter is guarded by some token.
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

    pub fn is_empty(&self, token: &impl Token<T>) -> bool {
        self.assert_accessible_by(token, None);

        // Safety: we have either shared or exclusive access to this token and, in both cases, we
        // know that `state` cannot be mutated until the token expires thus precluding a race
        // condition.
        self.value.is_empty()
    }

    pub fn set(&mut self, value: Option<T>) -> Option<T> {
        // Safety: this is a method that takes exclusive ownership of the object. Hence, it is
        // not impacted by our potentially dangerous `Sync` impl.
        self.value.set(value)
    }

    // === Namespace management === //

    pub fn namespace(&self) -> Option<Namespace> {
        NonZeroU64::new(self.namespace.load(Ordering::Relaxed)).map(Namespace)
    }

    fn assert_accessible_by(&self, token: &impl Token<T>, access: Option<ThreadAccess>) {
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

    // === Borrow methods === //

    pub fn try_get<'a, U>(&'a self, token: &'a U) -> Result<Option<&'a T>, BorrowError>
    where
        U: ReadTokenHint<T> + UnJailRefToken<T>,
    {
        self.assert_accessible_by(token, Some(ThreadAccess::Shared));

        // Safety: we know we can read from the `value`'s borrow count safely because this method
        // can only be run so long as we have `ReadToken`s alive and we can't cause reads
        // until we get a `ExclusiveToken`s, which is only possible once `token` is dead.
        unsafe {
            // Safety: additionally, we know nobody can borrow this cell mutably until all
            // `ReadToken`s die out so this is safe as well.
            self.value.try_borrow_unguarded()
        }
    }

    pub fn get_or_none<'a, U>(&'a self, token: &'a U) -> Option<&'a T>
    where
        U: ReadTokenHint<T> + UnJailRefToken<T>,
    {
        self.assert_accessible_by(token, Some(ThreadAccess::Shared));

        // Safety: we know we can read from the `value`'s borrow count safely because this method
        // can only be run so long as we have `ReadToken`s alive and we can't cause reads
        // until we get a `ExclusiveToken`s, which is only possible once `token` is dead.
        unsafe {
            // Safety: additionally, we know nobody can borrow this cell mutably until all
            // `ReadToken`s die out so this is safe as well.
            self.value.borrow_unguarded_or_none()
        }
    }

    pub fn get<'a, U>(&'a self, token: &'a U) -> &'a T
    where
        U: ReadTokenHint<T> + UnJailRefToken<T>,
    {
        self.assert_accessible_by(token, Some(ThreadAccess::Shared));

        // Safety: we know we can read from the `value`'s borrow count safely because this method
        // can only be run so long as we have `ReadToken`s alive and we can't cause reads
        // until we get a `ExclusiveToken`s, which is only possible once `token` is dead.
        unsafe {
            // Safety: additionally, we know nobody can borrow this cell mutably until all
            // `ReadToken`s die out so this is safe as well.
            self.value.borrow_unguarded()
        }
    }

    pub fn try_borrow<'a, U>(&'a self, token: &'a U) -> Result<Option<OptRef<'a, T>>, BorrowError>
    where
        U: ExclusiveTokenHint<T> + UnJailRefToken<T>,
    {
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
        self.value.try_borrow()
    }

    pub fn borrow_or_none<'a, U>(&'a self, token: &'a U) -> Option<OptRef<'a, T>>
    where
        U: ExclusiveTokenHint<T> + UnJailRefToken<T>,
    {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.borrow_or_none()
    }

    pub fn borrow<'a, U>(&'a self, token: &'a U) -> OptRef<'a, T>
    where
        U: ExclusiveTokenHint<T> + UnJailRefToken<T>,
    {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.borrow()
    }

    pub fn try_borrow_mut<'a, U>(
        &'a self,
        token: &'a U,
    ) -> Result<Option<OptRefMut<'a, T>>, BorrowMutError>
    where
        U: ExclusiveTokenHint<T> + UnJailMutToken<T>,
    {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.try_borrow_mut()
    }

    pub fn borrow_mut_or_none<'a, U>(&'a self, token: &'a U) -> Option<OptRefMut<'a, T>>
    where
        U: ExclusiveTokenHint<T> + UnJailMutToken<T>,
    {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.borrow_mut_or_none()
    }

    pub fn borrow_mut<'a, U>(&'a self, token: &'a U) -> OptRefMut<'a, T>
    where
        U: ExclusiveTokenHint<T> + UnJailMutToken<T>,
    {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.borrow_mut()
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
