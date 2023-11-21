use std::{
    cell::{Cell, UnsafeCell},
    error::Error,
    fmt,
    marker::PhantomData,
    mem::{self, MaybeUninit},
    ops::{Deref, DerefMut},
    ptr::NonNull,
};

use autoken::{
    ImmutableBorrow, MutableBorrow, Nothing, PotentialImmutableBorrow, PotentialMutableBorrow,
};

use crate::util::misc::{unwrap_error, RawFmt};

// === Magic === //

fn cell_u64_to_cell_u8(cell: &Cell<u64>) -> &[Cell<u8>; 8] {
    unsafe { std::mem::transmute(cell) }
}

// === Borrow state === ///

// Format:
//
// - A value of `EMPTY` means that the value is empty.
// - A value less than `NEUTRAL` means that the value is mutably borrowed.
// - A value equal to `NEUTRAL` means that the value is present and unborrowed.
// - A value greater than `NEUTRAL` means that the value is immutably borrowed.
//
const EMPTY: u8 = 0;
const NEUTRAL: u8 = u8::MAX / 2;

type CellBorrowRef<'b> = CellBorrow<'b, false>;
type CellBorrowMut<'a> = CellBorrow<'a, true>;

#[derive(Debug)]
struct CellBorrow<'b, const MUTABLE: bool> {
    state: &'b Cell<u8>,
}

impl<'b> CellBorrowRef<'b> {
    #[inline(always)]
    #[track_caller]
    fn acquire(state_cell: &'b Cell<u8>, location: &BorrowTracker) -> Option<Self> {
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
    #[track_caller]
    fn acquire(state_cell: &'b Cell<u8>, location: &BorrowTracker) -> Option<Self> {
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
            assert_ne!(state, u8::MAX, "too many immutable borrows");
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
            #[track_caller]
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
fn fmt_borrow_error_prefix(f: &mut fmt::Formatter, state: u8, mutably: bool) -> fmt::Result {
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
            state: u8,
            location: Option<&'static Location<'static>>,
        }

        impl<const MUTABLY: bool> CommonBorrowError<MUTABLY> {
            pub fn new(state: &Cell<u8>, borrowed_at: &BorrowTracker) -> Self {
                Self {
                    state: state.get(),
                    location: borrowed_at.0.get(),
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
            state: u8,
        }

        impl<const MUTABLY: bool> CommonBorrowError<MUTABLY> {
            pub fn new<T>(state: &Cell<u8>, _borrowed_at: &BorrowTracker) -> Self {
                Self { state: state.get() }
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

// === MultiOptRefCell === //

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub enum MultiRefCellIndex {
    Slot0 = 0,
    Slot1 = 1,
    Slot2 = 2,
    Slot3 = 3,
    Slot4 = 4,
    Slot5 = 5,
    Slot6 = 6,
    Slot7 = 7,
}

impl MultiRefCellIndex {
    pub const VALUES: [Self; 8] = [
        Self::Slot0,
        Self::Slot1,
        Self::Slot2,
        Self::Slot3,
        Self::Slot4,
        Self::Slot5,
        Self::Slot6,
        Self::Slot7,
    ];

    pub fn from_index(v: usize) -> Self {
        Self::VALUES[v]
    }

    pub fn iter() -> impl Iterator<Item = Self> {
        Self::VALUES.into_iter()
    }
}

pub struct MultiOptRefCell<T> {
    states: Cell<u64>,
    borrowed_ats: [BorrowTracker; 8],
    values: [UnsafeCell<MaybeUninit<T>>; 8],
}

impl<T> MultiOptRefCell<T> {
    // === Constructor === //

    pub fn new() -> Self {
        Self {
            states: Cell::new(u64::from_ne_bytes([EMPTY; 8])),
            borrowed_ats: std::array::from_fn(|_| BorrowTracker::new()),
            values: std::array::from_fn(|_| UnsafeCell::new(MaybeUninit::uninit())),
        }
    }

    // === Zero-cost queries === //

    pub fn into_inner(mut self) -> [Option<T>; 8] {
        let states = cell_u64_to_cell_u8(&self.states);

        let arr = std::array::from_fn(|i| {
            if states[i].get() != EMPTY {
                unsafe { Some(self.values[i].get_mut().assume_init_read()) }
            } else {
                None
            }
        });
        std::mem::forget(self);
        arr
    }

    pub fn get_mut(&mut self) -> [Option<&mut T>; 8] {
        let states = cell_u64_to_cell_u8(&self.states);

        let mut values = self.values.iter_mut();

        std::array::from_fn(|i| {
            if states[i].get() != EMPTY {
                unsafe { Some(values.next().unwrap().get_mut().assume_init_mut()) }
            } else {
                None
            }
        })
    }

    pub fn as_ptr(&self) -> *mut [T; 8] {
        let ptr = &self.values as *const [UnsafeCell<MaybeUninit<T>>; 8];
        let ptr = ptr as *const UnsafeCell<MaybeUninit<[T; 8]>>;
        let ptr = ptr as *mut MaybeUninit<[T; 8]>;
        ptr as *mut [T; 8]
    }

    pub fn is_empty(&self, i: MultiRefCellIndex) -> bool {
        cell_u64_to_cell_u8(&self.states)[i as usize].get() == EMPTY
    }

    pub fn set(&mut self, i: MultiRefCellIndex, value: Option<T>) -> Option<T> {
        self.undo_leak();
        self.replace(i, value)
    }

    pub fn undo_leak(&mut self) {
        for cell in cell_u64_to_cell_u8(&self.states) {
            if cell.get() != EMPTY {
                cell.set(NEUTRAL);
            }
        }
    }

    // === Borrowing === //

    #[cold]
    #[inline(never)]
    fn failed_to_borrow<const MUTABLY: bool>(&self, i: MultiRefCellIndex) -> ! {
        panic!(
            "{}",
            CommonBorrowError::<MUTABLY>::new(
                &cell_u64_to_cell_u8(&self.states)[i as usize],
                &self.borrowed_ats[i as usize],
            ),
        );
    }

    #[track_caller]
    #[inline(always)]
    pub fn try_borrow<'l>(
        &self,
        i: MultiRefCellIndex,
        loaner: &'l PotentialImmutableBorrow<T>,
    ) -> Result<Option<OptRef<T, Nothing<'l>>>, BorrowError> {
        let state = &cell_u64_to_cell_u8(&self.states)[i as usize];
        let borrowed_at = &self.borrowed_ats[i as usize];
        let value = &self.values[i as usize];

        if let Some(borrow) = CellBorrowRef::acquire(state, borrowed_at) {
            Ok(Some(OptRef {
                value: NonNull::from(unsafe { (*value.get()).assume_init_ref() }),
                autoken: loaner.loan(),
                borrow,
            }))
        } else if state.get() == EMPTY {
            Ok(None)
        } else {
            Err(BorrowError(CommonBorrowError::new(state, borrowed_at)))
        }
    }

    #[track_caller]
    #[inline(always)]
    pub fn borrow_or_none<'l>(
        &self,
        i: MultiRefCellIndex,
        loaner: &'l ImmutableBorrow<T>,
    ) -> Option<OptRef<T, Nothing<'l>>> {
        let state = &cell_u64_to_cell_u8(&self.states)[i as usize];
        let borrowed_at = &self.borrowed_ats[i as usize];
        let value = &self.values[i as usize];

        if let Some(borrow) = CellBorrowRef::acquire(state, borrowed_at) {
            Some(OptRef {
                value: NonNull::from(unsafe { (*value.get()).assume_init_ref() }),
                autoken: loaner.loan(),
                borrow,
            })
        } else if state.get() == EMPTY {
            None
        } else {
            self.failed_to_borrow::<false>(i);
        }
    }

    #[track_caller]
    #[inline(always)]
    pub fn borrow(&self, i: MultiRefCellIndex) -> OptRef<T, T> {
        let state = &cell_u64_to_cell_u8(&self.states)[i as usize];
        let borrowed_at = &self.borrowed_ats[i as usize];
        let value = &self.values[i as usize];

        if let Some(borrow) = CellBorrowRef::acquire(state, borrowed_at) {
            OptRef {
                value: NonNull::from(unsafe { (*value.get()).assume_init_ref() }),
                autoken: ImmutableBorrow::new(),
                borrow,
            }
        } else {
            self.failed_to_borrow::<false>(i);
        }
    }

    #[track_caller]
    #[inline(always)]
    pub fn borrow_on_loan<'l>(
        &self,
        i: MultiRefCellIndex,
        loaner: &'l ImmutableBorrow<T>,
    ) -> OptRef<T, Nothing<'l>> {
        let _ = loaner;
        OptRef::strip_lifetime_analysis(autoken::assume_no_alias(|| self.borrow(i)))
    }

    #[track_caller]
    #[inline(always)]
    pub fn try_borrow_mut<'l>(
        &self,
        i: MultiRefCellIndex,
        loaner: &'l mut PotentialMutableBorrow<T>,
    ) -> Result<Option<OptRefMut<T, Nothing<'l>>>, BorrowMutError> {
        let state = &cell_u64_to_cell_u8(&self.states)[i as usize];
        let borrowed_at = &self.borrowed_ats[i as usize];
        let value = &self.values[i as usize];

        if let Some(borrow) = CellBorrowMut::acquire(state, borrowed_at) {
            Ok(Some(OptRefMut {
                value: NonNull::from(unsafe { (*value.get()).assume_init_mut() }),
                autoken: loaner.loan(),
                borrow,
                marker: PhantomData,
            }))
        } else if state.get() == EMPTY {
            Ok(None)
        } else {
            Err(BorrowMutError(CommonBorrowError::new(state, borrowed_at)))
        }
    }

    #[track_caller]
    #[inline(always)]
    pub fn borrow_mut_or_none<'l>(
        &self,
        i: MultiRefCellIndex,
        loaner: &'l mut MutableBorrow<T>,
    ) -> Option<OptRefMut<T, Nothing<'l>>> {
        let state = &cell_u64_to_cell_u8(&self.states)[i as usize];
        let borrowed_at = &self.borrowed_ats[i as usize];
        let value = &self.values[i as usize];

        if let Some(borrow) = CellBorrowMut::acquire(state, borrowed_at) {
            Some(OptRefMut {
                value: NonNull::from(unsafe { (*value.get()).assume_init_mut() }),
                autoken: loaner.loan(),
                borrow,
                marker: PhantomData,
            })
        } else if state.get() == EMPTY {
            None
        } else {
            self.failed_to_borrow::<true>(i);
        }
    }

    #[track_caller]
    #[inline(always)]
    pub fn borrow_mut(&self, i: MultiRefCellIndex) -> OptRefMut<T, T> {
        let state = &cell_u64_to_cell_u8(&self.states)[i as usize];
        let borrowed_at = &self.borrowed_ats[i as usize];
        let value = &self.values[i as usize];

        if let Some(borrow) = CellBorrowMut::acquire(state, borrowed_at) {
            OptRefMut {
                value: NonNull::from(unsafe { (*value.get()).assume_init_mut() }),
                autoken: MutableBorrow::new(),
                borrow,
                marker: PhantomData,
            }
        } else {
            self.failed_to_borrow::<true>(i);
        }
    }

    #[track_caller]
    #[inline(always)]
    pub fn borrow_mut_on_loan<'l>(
        &self,
        i: MultiRefCellIndex,
        loaner: &'l mut MutableBorrow<T>,
    ) -> OptRefMut<T, Nothing<'l>> {
        let _ = loaner;
        OptRefMut::strip_lifetime_analysis(autoken::assume_no_alias(|| self.borrow_mut(i)))
    }

    // === Unguarded borrowing === //

    pub unsafe fn try_borrow_unguarded(
        &self,
        i: MultiRefCellIndex,
    ) -> Result<Option<&T>, BorrowError> {
        let state = &cell_u64_to_cell_u8(&self.states)[i as usize];
        let borrowed_at = &self.borrowed_ats[i as usize];
        let value = &self.values[i as usize];

        if state.get() == NEUTRAL {
            Ok(Some(unsafe { (*value.get()).assume_init_ref() }))
        } else if state.get() == EMPTY {
            Ok(None)
        } else {
            Err(BorrowError(CommonBorrowError::new(state, borrowed_at)))
        }
    }

    pub unsafe fn borrow_unguarded_or_none(&self, i: MultiRefCellIndex) -> Option<&T> {
        let state = &cell_u64_to_cell_u8(&self.states)[i as usize];
        let value = &self.values[i as usize];

        if state.get() == NEUTRAL {
            Some(unsafe { (*value.get()).assume_init_ref() })
        } else if state.get() == EMPTY {
            None
        } else {
            self.failed_to_borrow::<false>(i);
        }
    }

    pub unsafe fn borrow_unguarded(&self, i: MultiRefCellIndex) -> &T {
        let state = &cell_u64_to_cell_u8(&self.states)[i as usize];
        let value = &self.values[i as usize];

        if state.get() == NEUTRAL {
            unsafe { (*value.get()).assume_init_ref() }
        } else {
            self.failed_to_borrow::<false>(i);
        }
    }

    // === Replace === //

    // FIXME: This is currently unsound and incorrect
    #[track_caller]
    pub fn try_replace_with<F>(
        &self,
        i: MultiRefCellIndex,
        f: F,
    ) -> Result<Option<T>, BorrowMutError>
    where
        F: FnOnce(Option<&mut T>) -> Option<T>,
    {
        autoken::assert_mutably_borrowable::<T>();

        let state = &cell_u64_to_cell_u8(&self.states)[i as usize];
        let borrowed_at = &self.borrowed_ats[i as usize];
        let value = &self.values[i as usize];

        if state.get() == NEUTRAL {
            let value_container = unsafe { &mut *value.get() };
            let curr_value = unsafe { value_container.assume_init_mut() };

            if let Some(value) = f(Some(curr_value)) {
                Ok(Some(mem::replace(curr_value, value)))
            } else {
                state.set(EMPTY);
                Ok(Some(unsafe { value_container.assume_init_read() }))
            }
        } else if state.get() == EMPTY {
            if let Some(taken_value) = f(None) {
                state.set(NEUTRAL);
                unsafe { *value.get() = MaybeUninit::new(taken_value) };
            }
            Ok(None)
        } else {
            Err(BorrowMutError(CommonBorrowError::new(state, borrowed_at)))
        }
    }

    #[track_caller]
    pub fn replace_with<F>(&self, i: MultiRefCellIndex, f: F) -> Option<T>
    where
        F: FnOnce(Option<&mut T>) -> Option<T>,
    {
        unwrap_error(self.try_replace_with(i, f))
    }

    #[track_caller]
    pub fn try_replace(
        &self,
        i: MultiRefCellIndex,
        t: Option<T>,
    ) -> Result<Option<T>, BorrowMutError> {
        self.try_replace_with(i, |_| t)
    }

    #[track_caller]
    pub fn replace(&self, i: MultiRefCellIndex, t: Option<T>) -> Option<T> {
        self.replace_with(i, |_| t)
    }

    #[track_caller]
    pub fn take(&self, i: MultiRefCellIndex) -> Option<T> {
        self.replace(i, None)
    }

    #[track_caller]
    pub fn swap(
        &self,
        other: &MultiOptRefCell<T>,
        i_me: MultiRefCellIndex,
        i_other: MultiRefCellIndex,
    ) {
        // This check is necessary because, if the cell is the same full cell, `value_from_other`
        // will resolve to `None` as it places the value back in, causing `self.replace` to set the
        // value back to null.
        if self.as_ptr() == other.as_ptr() {
            return;
        }

        let value_from_me = self.take(i_me);
        let value_from_other = other.replace(i_other, value_from_me);
        self.replace(i_me, value_from_other);
    }
}

impl<T: fmt::Debug> fmt::Debug for MultiOptRefCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut list = f.debug_list();

        for i in MultiRefCellIndex::iter() {
            let loaner = PotentialImmutableBorrow::new();

            // For some weird reason, rust thinks destructors are run before the last return statement?
            match self.try_borrow(i, &loaner) {
                Ok(borrow) => list.entry(&borrow),
                Err(_) => list.entry(&RawFmt("<borrowed>")),
            };
        }

        list.finish()
    }
}

impl<T> Default for MultiOptRefCell<T> {
    fn default() -> Self {
        MultiOptRefCell::new()
    }
}

unsafe impl<T: Send> Send for MultiOptRefCell<T> {}

impl<T> Drop for MultiOptRefCell<T> {
    fn drop(&mut self) {
        let states = cell_u64_to_cell_u8(&self.states);

        for (state, value) in states.iter().zip(self.values.iter_mut()) {
            if state.get() == EMPTY {
                unsafe { value.get_mut().assume_init_drop() };
            }
        }
    }
}

// === OptRef === //

pub struct OptRef<'b, T: ?Sized, B: ?Sized = T> {
    value: NonNull<T>,
    autoken: ImmutableBorrow<B>,
    borrow: CellBorrowRef<'b>,
}

impl<'b, T: ?Sized, B: ?Sized> OptRef<'b, T, B> {
    #[allow(clippy::should_implement_trait)] // (follows standard library conventions)
    pub fn clone(orig: &Self) -> Self {
        Self {
            value: orig.value,
            autoken: orig.autoken.clone(),
            borrow: orig.borrow.clone(),
        }
    }

    pub fn map<U: ?Sized, F>(orig: OptRef<'b, T, B>, f: F) -> OptRef<'b, U, B>
    where
        F: FnOnce(&T) -> &U,
    {
        OptRef {
            value: NonNull::from(f(&*orig)),
            autoken: orig.autoken,
            borrow: orig.borrow,
        }
    }

    pub fn filter_map<U: ?Sized, F>(orig: OptRef<'b, T, B>, f: F) -> Result<OptRef<'b, U, B>, Self>
    where
        F: FnOnce(&T) -> Option<&U>,
    {
        match f(&*orig) {
            Some(value) => Ok(OptRef {
                value: NonNull::from(value),
                autoken: orig.autoken,
                borrow: orig.borrow,
            }),
            None => Err(orig),
        }
    }

    pub fn map_split<U: ?Sized, V: ?Sized, F>(
        orig: OptRef<'b, T, B>,
        f: F,
    ) -> (OptRef<'b, U, B>, OptRef<'b, V, B>)
    where
        F: FnOnce(&T) -> (&U, &V),
    {
        let (a, b) = f(&*orig);
        let borrow = orig.borrow.clone();
        (
            OptRef {
                value: NonNull::from(a),
                autoken: orig.autoken.clone(),
                borrow,
            },
            OptRef {
                value: NonNull::from(b),
                autoken: orig.autoken,
                borrow: orig.borrow,
            },
        )
    }

    pub fn leak(orig: OptRef<'b, T, B>) -> &'b T {
        mem::forget(orig.borrow);
        unsafe { orig.value.as_ref() }
    }

    pub fn strip_lifetime_analysis(orig: OptRef<'b, T, B>) -> OptRef<'b, T, Nothing<'static>> {
        OptRef {
            value: orig.value,
            autoken: orig.autoken.strip_lifetime_analysis(),
            borrow: orig.borrow,
        }
    }
}

impl<T: ?Sized, B: ?Sized> Deref for OptRef<'_, T, B> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.value.as_ref() }
    }
}

impl<T: ?Sized + fmt::Debug, B: ?Sized> fmt::Debug for OptRef<'_, T, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T: ?Sized + fmt::Display, B: ?Sized> fmt::Display for OptRef<'_, T, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

// === OptRefMut === //

pub struct OptRefMut<'b, T: ?Sized, B: ?Sized = T> {
    autoken: MutableBorrow<B>,
    // NB: we use a pointer instead of `&'b mut T` to avoid `noalias` violations, because a
    // `RefMut` argument doesn't hold exclusivity for its whole scope, only until it drops.
    value: NonNull<T>,
    borrow: CellBorrowMut<'b>,
    // `NonNull` is covariant over `T`, so we need to reintroduce invariance.
    marker: PhantomData<&'b mut T>,
}

impl<'b, T: ?Sized, B: ?Sized> OptRefMut<'b, T, B> {
    pub fn map<U: ?Sized, F>(mut orig: OptRefMut<'b, T, B>, f: F) -> OptRefMut<'b, U, B>
    where
        F: FnOnce(&mut T) -> &mut U,
    {
        let value = NonNull::from(f(&mut *orig));
        OptRefMut {
            value,
            autoken: orig.autoken,
            borrow: orig.borrow,
            marker: PhantomData,
        }
    }

    pub fn filter_map<U: ?Sized, F>(
        mut orig: OptRefMut<'b, T, B>,
        f: F,
    ) -> Result<OptRefMut<'b, U, B>, Self>
    where
        F: FnOnce(&mut T) -> Option<&mut U>,
    {
        match f(&mut *orig) {
            Some(value) => Ok(OptRefMut {
                value: NonNull::from(value),
                autoken: orig.autoken,
                borrow: orig.borrow,
                marker: PhantomData,
            }),
            None => Err(orig),
        }
    }

    pub fn map_split<U: ?Sized, V: ?Sized, F>(
        mut orig: OptRefMut<'b, T, B>,
        f: F,
    ) -> (OptRefMut<'b, U, B>, OptRefMut<'b, V, B>)
    where
        F: FnOnce(&mut T) -> (&mut U, &mut V),
    {
        let autoken = orig.autoken.assume_no_alias_clone();
        let borrow = orig.borrow.clone();
        let (a, b) = f(&mut *orig);
        (
            OptRefMut {
                value: NonNull::from(a),
                autoken,
                borrow,
                marker: PhantomData,
            },
            OptRefMut {
                value: NonNull::from(b),
                autoken: orig.autoken,
                borrow: orig.borrow,
                marker: PhantomData,
            },
        )
    }

    pub fn leak(mut orig: OptRefMut<'b, T, B>) -> &'b mut T {
        mem::forget(orig.borrow);
        unsafe { orig.value.as_mut() }
    }

    pub fn strip_lifetime_analysis(
        orig: OptRefMut<'b, T, B>,
    ) -> OptRefMut<'b, T, Nothing<'static>> {
        OptRefMut {
            autoken: orig.autoken.strip_lifetime_analysis(),
            value: orig.value,
            borrow: orig.borrow,
            marker: orig.marker,
        }
    }
}

impl<T: ?Sized, B: ?Sized> Deref for OptRefMut<'_, T, B> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.value.as_ref() }
    }
}

impl<T: ?Sized, B: ?Sized> DerefMut for OptRefMut<'_, T, B> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.value.as_mut() }
    }
}

impl<T: ?Sized + fmt::Debug, B: ?Sized> fmt::Debug for OptRefMut<'_, T, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T: ?Sized + fmt::Display, B: ?Sized> fmt::Display for OptRefMut<'_, T, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}
