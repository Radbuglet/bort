//! An implementation of the Rust standard library's [`RefCell`](std::cell::RefCell) that is specialized
//! to hold an `Option<T>` instead of a plain `T` instance.
//!
//! [`OptRefCell`] provides several performance benefits over `RefCell<Option<T>>`:
//!
//! 1. The size of the structure is guaranteed to be equal to `RefCell<T>`, unlike `RefCell<Option<T>>`,
//!    which may be made larger to accommodate the discriminator.
//!
//! 2. Borrowing an `OptRefCell` and unwrapping its `Option` can be done in a single comparison instead
//!    of two, shrinking the assembly output and micro-optimizing this exceedingly common operation.
//!    (remember: every single component access has to go through this process so even small
//!    optimizations here are worthwhile!)
//!
//! 3. Mapping an `OptRefCell` to a readonly zeroed page will still let it be safely borrowable as an
//!    empty cell. This aspect is critical to implementing indirection-less [`Slot`](super::heap::Slot)s
//!    on platforms supporting manual memory mapping management.
//!
//! This code was largely copied from the Rust standard library's implementation of `RefCell` with
//! some very minor tweaks. Thanks, Rust standard library team!

use std::{
    cell::{Cell, UnsafeCell},
    cmp::Ordering,
    error::Error,
    fmt,
    marker::PhantomData,
    mem::{self, MaybeUninit},
    ops::{Deref, DerefMut},
    ptr::NonNull,
};

use crate::util::{unwrap_error, RawFmt};

// === Borrow state === ///

// Format:
//
// - A value of `EMPTY` means that the value is empty.
// - A value less than `NEUTRAL` means that the value is mutably borrowed.
// - A value equal to `NEUTRAL` means that the value is present and unborrowed.
// - A value greater than `NEUTRAL` means that the value is immutably borrowed.
//
const EMPTY: usize = 0;
const NEUTRAL: usize = usize::MAX / 2;

type CellBorrowRef<'b> = CellBorrow<'b, false>;
type CellBorrowMut<'a> = CellBorrow<'a, true>;

#[derive(Debug)]
struct CellBorrow<'b, const MUTABLE: bool> {
    state: &'b Cell<usize>,
}

impl<'b> CellBorrowRef<'b> {
    #[inline(always)]
    fn acquire(state_cell: &'b Cell<usize>, location: &BorrowTracker) -> Option<Self> {
        let state = state_cell.get();

        // Increment the state unconditionally
        let state = state.wrapping_add(1);

        // If the state ended up being greater than `NEUTRAL`, this implies that we were `>= NEUTRAL`
        // before the increment, which is the more traditional way of checking this. Additionally,
        // because we know that we're in reading mode *after* we did the increment, we know that
        // we couldn't have possibly overflowed the reader counter, avoiding that nasty source of
        // UB without an additional branch.
        if state > NEUTRAL {
            // If we're the first reader, mark our location.
            if state == NEUTRAL + 1 {
                location.set();
            }

            state_cell.set(state);

            Some(Self { state: state_cell })
        } else {
            None
        }
    }
}

impl<'b> CellBorrowMut<'b> {
    #[inline(always)]
    fn acquire(state_cell: &'b Cell<usize>, location: &BorrowTracker) -> Option<Self> {
        let state = state_cell.get();
        if state == NEUTRAL {
            location.set();
            state_cell.set(NEUTRAL - 1);

            Some(Self { state: state_cell })
        } else {
            None
        }
    }
}

impl<const MUTABLE: bool> Clone for CellBorrow<'_, MUTABLE> {
    fn clone(&self) -> Self {
        let state = self.state.get();
        let state = if MUTABLE {
            assert_ne!(state, EMPTY + 1, "too many mutable borrows");
            state - 1
        } else {
            assert_ne!(state, usize::MAX, "too many immutable borrows");
            state + 1
        };
        self.state.set(state);

        Self { state: self.state }
    }
}

impl<const MUTABLE: bool> Drop for CellBorrow<'_, MUTABLE> {
    fn drop(&mut self) {
        self.state.set(if MUTABLE {
            self.state.get() + 1
        } else {
            self.state.get() - 1
        });
    }
}

// === Borrow tracker === //

cfgenius::define!(pub tracks_borrow_location = cfg(debug_assertions));

cfgenius::cond! {
    if macro(tracks_borrow_location) {
        use std::panic::Location;

        #[derive(Debug, Clone)]
        struct BorrowTracker(Cell<Option<&'static Location<'static>>>);

        impl BorrowTracker {
            pub const fn new() -> Self {
                Self(Cell::new(None))
            }

            #[inline(always)]
            pub fn set(&self) {
                self.0.set(Some(Location::caller()));
            }
        }
    } else {
        #[derive(Debug, Clone)]
        struct BorrowTracker(());

        impl BorrowTracker {
            pub const fn new() -> Self {
                Self(())
            }

            #[inline(always)]
            pub fn set(&self) {}
        }
    }
}

// === Borrow error === //

// Public
#[derive(Debug, Clone)]
pub struct BorrowError(CommonBorrowError<false>);

impl Error for BorrowError {}

impl fmt::Display for BorrowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone)]
pub struct BorrowMutError(CommonBorrowError<true>);

impl Error for BorrowMutError {}

impl fmt::Display for BorrowMutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

// Internal
fn fmt_borrow_error_prefix(f: &mut fmt::Formatter, state: usize, mutably: bool) -> fmt::Result {
    write!(
        f,
        "failed to borrow cell {}: ",
        if mutably { "mutably" } else { "immutably" }
    )?;

    if state == EMPTY {
        write!(f, "cell is empty")
    } else {
        // If this subtraction fails, it means that we're already borrowed in the state we wanted
        // to be in, which would imply that the borrow failed because we have too many guards of
        // the same type.
        let blockers = if mutably {
            NEUTRAL.checked_sub(state)
        } else {
            state.checked_sub(NEUTRAL)
        };

        if let Some(blockers) = blockers {
            write!(
                f,
                "cell is borrowed by {blockers} {}{}",
                if mutably { "reader" } else { "writer" },
                if blockers == 1 { "" } else { "s" },
            )
        } else {
            write!(f, "too many {}s", if mutably { "writer" } else { "reader" },)
        }
    }
}

cfgenius::cond! {
    if macro(tracks_borrow_location) {
        #[derive(Clone)]
        struct CommonBorrowError<const MUTABLY: bool> {
            state: usize,
            location: Option<&'static Location<'static>>,
        }

        impl<const MUTABLY: bool> CommonBorrowError<MUTABLY> {
            pub fn new<T>(cell: &OptRefCell<T>) -> Self {
                Self {
                    state: cell.state.get(),
                    location: cell.borrowed_at.0.get(),
                }
            }
        }

        impl<const MUTABLY: bool> fmt::Debug for CommonBorrowError<MUTABLY> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct("CommonBorrowError")
                    .field("mutably", &MUTABLY)
                    .field("state", &self.state)
                    .field("location", &self.location)
                    .finish()
            }
        }

        impl<const MUTABLY: bool> Error for CommonBorrowError<MUTABLY> {}

        impl<const MUTABLY: bool> fmt::Display for CommonBorrowError<MUTABLY> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt_borrow_error_prefix(f, self.state, MUTABLY)?;

                if let Some(location) = self.location {
                    write!(
                        f,
                        " (first borrow location: {} at {}:{})",
                        location.file(),
                        location.line(),
                        location.column(),
                    )?;
                }

                Ok(())
            }
        }
    } else {
        #[derive(Clone)]
        struct CommonBorrowError<const MUTABLY: bool> {
            state: usize,
        }

        impl<const MUTABLY: bool> CommonBorrowError<MUTABLY> {
            pub fn new<T>(cell: &OptRefCell<T>) -> Self {
                Self { state: cell.state.get() }
            }
        }

        impl<const MUTABLY: bool> fmt::Debug for CommonBorrowError<MUTABLY> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct("CommonBorrowError")
                    .field("mutably", &MUTABLY)
                    .field("state", &self.state)
                    .finish()
            }
        }

        impl<const MUTABLY: bool> Error for CommonBorrowError<MUTABLY> {}

        impl<const MUTABLY: bool> fmt::Display for CommonBorrowError<MUTABLY> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt_borrow_error_prefix(f, self.state, MUTABLY)
            }
        }
    }
}

// === OptRefCell === //

pub struct OptRefCell<T> {
    state: Cell<usize>,
    borrowed_at: BorrowTracker,
    value: UnsafeCell<MaybeUninit<T>>,
}

impl<T> OptRefCell<T> {
    pub fn new(value: Option<T>) -> Self {
        match value {
            Some(value) => Self::new_full(value),
            None => Self::new_empty(),
        }
    }

    pub const fn new_full(value: T) -> Self {
        Self {
            state: Cell::new(NEUTRAL),
            borrowed_at: BorrowTracker::new(),
            value: UnsafeCell::new(MaybeUninit::new(value)),
        }
    }

    pub const fn new_empty() -> Self {
        Self {
            state: Cell::new(EMPTY),
            borrowed_at: BorrowTracker::new(),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    pub fn into_inner(mut self) -> Option<T> {
        if self.is_empty() {
            None
        } else {
            let value = unsafe { self.value.get_mut().assume_init_read() };
            mem::forget(self);
            Some(value)
        }
    }

    pub fn get_mut(&mut self) -> Option<&mut T> {
        if self.is_empty() {
            None
        } else {
            Some(unsafe { self.value.get_mut().assume_init_mut() })
        }
    }

    pub fn as_ptr(&self) -> *mut T {
        // Safety: `MaybeUninit<T>` is `repr(transparent)` w.r.t `T`.
        self.value.get().cast()
    }

    pub fn is_empty(&self) -> bool {
        self.state.get() == EMPTY
    }

    pub fn set(&mut self, value: Option<T>) -> Option<T> {
        self.undo_leak();
        self.replace(value)
    }

    // === Borrowing === //

    #[cold]
    #[inline(never)]
    fn failed_to_borrow<const MUTABLY: bool>(&self) -> ! {
        panic!("{}", CommonBorrowError::<MUTABLY>::new(self));
    }

    #[track_caller]
    #[inline(always)]
    pub fn try_borrow(&self) -> Result<Option<OptRef<T>>, BorrowError> {
        if let Some(borrow) = CellBorrowRef::acquire(&self.state, &self.borrowed_at) {
            Ok(Some(OptRef {
                value: NonNull::from(unsafe { (*self.value.get()).assume_init_ref() }),
                borrow,
            }))
        } else if self.is_empty() {
            Ok(None)
        } else {
            Err(BorrowError(CommonBorrowError::new(self)))
        }
    }

    #[track_caller]
    #[inline(always)]
    pub fn borrow_or_none(&self) -> Option<OptRef<T>> {
        if let Some(borrow) = CellBorrowRef::acquire(&self.state, &self.borrowed_at) {
            Some(OptRef {
                value: NonNull::from(unsafe { (*self.value.get()).assume_init_ref() }),
                borrow,
            })
        } else if self.is_empty() {
            None
        } else {
            self.failed_to_borrow::<false>();
        }
    }

    #[track_caller]
    #[inline(always)]
    pub fn borrow(&self) -> OptRef<T> {
        if let Some(borrow) = CellBorrowRef::acquire(&self.state, &self.borrowed_at) {
            OptRef {
                value: NonNull::from(unsafe { (*self.value.get()).assume_init_ref() }),
                borrow,
            }
        } else {
            self.failed_to_borrow::<false>();
        }
    }

    #[track_caller]
    #[inline(always)]
    pub fn try_borrow_mut(&self) -> Result<Option<OptRefMut<T>>, BorrowMutError> {
        if let Some(borrow) = CellBorrowMut::acquire(&self.state, &self.borrowed_at) {
            Ok(Some(OptRefMut {
                value: NonNull::from(unsafe { (*self.value.get()).assume_init_mut() }),
                borrow,
                marker: PhantomData,
            }))
        } else if self.is_empty() {
            Ok(None)
        } else {
            Err(BorrowMutError(CommonBorrowError::new(self)))
        }
    }

    #[track_caller]
    #[inline(always)]
    pub fn borrow_mut_or_none(&self) -> Option<OptRefMut<T>> {
        if let Some(borrow) = CellBorrowMut::acquire(&self.state, &self.borrowed_at) {
            Some(OptRefMut {
                value: NonNull::from(unsafe { (*self.value.get()).assume_init_mut() }),
                borrow,
                marker: PhantomData,
            })
        } else if self.is_empty() {
            None
        } else {
            self.failed_to_borrow::<true>();
        }
    }

    #[track_caller]
    #[inline(always)]
    pub fn borrow_mut(&self) -> OptRefMut<T> {
        if let Some(borrow) = CellBorrowMut::acquire(&self.state, &self.borrowed_at) {
            OptRefMut {
                value: NonNull::from(unsafe { (*self.value.get()).assume_init_mut() }),
                borrow,
                marker: PhantomData,
            }
        } else {
            self.failed_to_borrow::<true>();
        }
    }

    // === Unguarded borrowing === //

    pub unsafe fn try_borrow_unguarded(&self) -> Result<Option<&T>, BorrowError> {
        let state = self.state.get();

        if state == NEUTRAL {
            Ok(Some(unsafe { (*self.value.get()).assume_init_ref() }))
        } else if state == EMPTY {
            Ok(None)
        } else {
            Err(BorrowError(CommonBorrowError::new(self)))
        }
    }

    pub unsafe fn borrow_unguarded_or_none(&self) -> Option<&T> {
        let state = self.state.get();

        if state == NEUTRAL {
            Some(unsafe { (*self.value.get()).assume_init_ref() })
        } else if state == EMPTY {
            None
        } else {
            self.failed_to_borrow::<false>();
        }
    }

    pub unsafe fn borrow_unguarded(&self) -> &T {
        let state = self.state.get();

        if state == NEUTRAL {
            unsafe { (*self.value.get()).assume_init_ref() }
        } else {
            self.failed_to_borrow::<false>();
        }
    }

    // === Replace === //

    pub fn try_replace_with<F>(&self, f: F) -> Result<Option<T>, BorrowMutError>
    where
        F: FnOnce(Option<&mut T>) -> Option<T>,
    {
        let state = self.state.get();
        if state == NEUTRAL {
            let value_container = unsafe { &mut *self.value.get() };
            let curr_value = unsafe { value_container.assume_init_mut() };

            if let Some(value) = f(Some(curr_value)) {
                Ok(Some(mem::replace(curr_value, value)))
            } else {
                self.state.set(EMPTY);
                drop(curr_value);
                Ok(Some(unsafe { value_container.assume_init_read() }))
            }
        } else if state == EMPTY {
            if let Some(value) = f(None) {
                self.state.set(NEUTRAL);
                unsafe { *self.value.get() = MaybeUninit::new(value) };
            }
            Ok(None)
        } else {
            Err(BorrowMutError(CommonBorrowError::new(self)))
        }
    }

    pub fn replace_with<F>(&self, f: F) -> Option<T>
    where
        F: FnOnce(Option<&mut T>) -> Option<T>,
    {
        unwrap_error(self.try_replace_with(f))
    }

    pub fn try_replace(&self, t: Option<T>) -> Result<Option<T>, BorrowMutError> {
        self.try_replace_with(|_| t)
    }

    pub fn replace(&self, t: Option<T>) -> Option<T> {
        self.replace_with(|_| t)
    }

    // === Extra utilities === //

    pub fn take(&self) -> Option<T> {
        self.replace(None)
    }

    pub fn swap(&self, other: &OptRefCell<T>) {
        let value = self.take();
        self.replace(other.replace(value));
    }

    pub fn undo_leak(&mut self) {
        if self.state.get() != EMPTY {
            self.state.set(NEUTRAL);
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for OptRefCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.try_borrow() {
            Ok(borrow) => f.debug_struct("RefCell").field("value", &borrow).finish(),
            Err(_) => f
                .debug_struct("RefCell")
                .field("value", &RawFmt("<borrowed>"))
                .finish(),
        }
    }
}

impl<T: Clone> Clone for OptRefCell<T> {
    #[inline]
    #[track_caller]
    fn clone(&self) -> Self {
        Self::new(self.borrow_or_none().map(|v| v.clone()))
    }
}

impl<T> Default for OptRefCell<T> {
    fn default() -> Self {
        OptRefCell::new_empty()
    }
}

impl<T: PartialEq> PartialEq for OptRefCell<T> {
    fn eq(&self, other: &Self) -> bool {
        self.borrow_or_none().as_deref() == other.borrow_or_none().as_deref()
    }
}

impl<T: Eq> Eq for OptRefCell<T> {}

impl<T: PartialOrd> PartialOrd for OptRefCell<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.borrow_or_none()
            .as_deref()
            .partial_cmp(&other.borrow_or_none().as_deref())
    }
}

impl<T: Ord> Ord for OptRefCell<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.borrow_or_none()
            .as_deref()
            .cmp(&other.borrow_or_none().as_deref())
    }
}

impl<T> From<Option<T>> for OptRefCell<T> {
    fn from(value: Option<T>) -> Self {
        Self::new(value)
    }
}

unsafe impl<T: Send> Send for OptRefCell<T> {}

impl<T> Drop for OptRefCell<T> {
    fn drop(&mut self) {
        if !self.is_empty() {
            unsafe { self.value.get_mut().assume_init_drop() };
        }
    }
}

// === OptRef === //

pub struct OptRef<'b, T: ?Sized> {
    value: NonNull<T>,
    borrow: CellBorrowRef<'b>,
}

impl<'b, T: ?Sized> OptRef<'b, T> {
    pub fn clone(orig: &Self) -> Self {
        Self {
            value: orig.value,
            borrow: orig.borrow.clone(),
        }
    }

    pub fn map<U: ?Sized, F>(orig: OptRef<'b, T>, f: F) -> OptRef<'b, U>
    where
        F: FnOnce(&T) -> &U,
    {
        OptRef {
            value: NonNull::from(f(&*orig)),
            borrow: orig.borrow,
        }
    }

    pub fn filter_map<U: ?Sized, F>(orig: OptRef<'b, T>, f: F) -> Result<OptRef<'b, U>, Self>
    where
        F: FnOnce(&T) -> Option<&U>,
    {
        match f(&*orig) {
            Some(value) => Ok(OptRef {
                value: NonNull::from(value),
                borrow: orig.borrow,
            }),
            None => Err(orig),
        }
    }

    pub fn map_split<U: ?Sized, V: ?Sized, F>(
        orig: OptRef<'b, T>,
        f: F,
    ) -> (OptRef<'b, U>, OptRef<'b, V>)
    where
        F: FnOnce(&T) -> (&U, &V),
    {
        let (a, b) = f(&*orig);
        let borrow = orig.borrow.clone();
        (
            OptRef {
                value: NonNull::from(a),
                borrow,
            },
            OptRef {
                value: NonNull::from(b),
                borrow: orig.borrow,
            },
        )
    }

    pub fn leak(orig: OptRef<'b, T>) -> &'b T {
        mem::forget(orig.borrow);
        unsafe { orig.value.as_ref() }
    }
}

impl<T: ?Sized> Deref for OptRef<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.value.as_ref() }
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for OptRef<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T: ?Sized + fmt::Display> fmt::Display for OptRef<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

// === OptRefMut === //

pub struct OptRefMut<'b, T: ?Sized> {
    // NB: we use a pointer instead of `&'b mut T` to avoid `noalias` violations, because a
    // `RefMut` argument doesn't hold exclusivity for its whole scope, only until it drops.
    value: NonNull<T>,
    borrow: CellBorrowMut<'b>,
    // `NonNull` is covariant over `T`, so we need to reintroduce invariance.
    marker: PhantomData<&'b mut T>,
}

impl<'b, T: ?Sized> OptRefMut<'b, T> {
    pub fn map<U: ?Sized, F>(mut orig: OptRefMut<'b, T>, f: F) -> OptRefMut<'b, U>
    where
        F: FnOnce(&mut T) -> &mut U,
    {
        let value = NonNull::from(f(&mut *orig));
        OptRefMut {
            value,
            borrow: orig.borrow,
            marker: PhantomData,
        }
    }

    pub fn filter_map<U: ?Sized, F>(
        mut orig: OptRefMut<'b, T>,
        f: F,
    ) -> Result<OptRefMut<'b, U>, Self>
    where
        F: FnOnce(&mut T) -> Option<&mut U>,
    {
        match f(&mut *orig) {
            Some(value) => Ok(OptRefMut {
                value: NonNull::from(value),
                borrow: orig.borrow,
                marker: PhantomData,
            }),
            None => Err(orig),
        }
    }

    pub fn map_split<U: ?Sized, V: ?Sized, F>(
        mut orig: OptRefMut<'b, T>,
        f: F,
    ) -> (OptRefMut<'b, U>, OptRefMut<'b, V>)
    where
        F: FnOnce(&mut T) -> (&mut U, &mut V),
    {
        let borrow = orig.borrow.clone();
        let (a, b) = f(&mut *orig);
        (
            OptRefMut {
                value: NonNull::from(a),
                borrow,
                marker: PhantomData,
            },
            OptRefMut {
                value: NonNull::from(b),
                borrow: orig.borrow,
                marker: PhantomData,
            },
        )
    }

    pub fn leak(mut orig: OptRefMut<'b, T>) -> &'b mut T {
        mem::forget(orig.borrow);
        unsafe { orig.value.as_mut() }
    }
}

impl<T: ?Sized> Deref for OptRefMut<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.value.as_ref() }
    }
}

impl<T: ?Sized> DerefMut for OptRefMut<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.value.as_mut() }
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for OptRefMut<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T: ?Sized + fmt::Display> fmt::Display for OptRefMut<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}
