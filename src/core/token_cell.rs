use std::{cell::Cell, fmt};

use autoken::{
    ImmutableBorrow, MutableBorrow, Nothing, PotentialImmutableBorrow, PotentialMutableBorrow,
};

use crate::util::misc::NOT_ON_MAIN_THREAD_MSG;

use super::{
    cell::{
        BorrowError, BorrowMutError, MultiOptRef, MultiOptRefCell, MultiOptRefMut,
        MultiRefCellIndex, OptRef, OptRefCell, OptRefMut,
    },
    token::{
        is_main_thread, BorrowMutToken, BorrowToken, GetToken, MainThreadToken, ThreadAccess,
        Token, TokenFor, UnJailRefToken,
    },
};

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

// N.B. `Send` is still structural for `MainThreadJail`.

// Safety: this is always safe to implement because either:
//
// - `T` is `Sync`, which is intrinsically safe.
// - `T` is `!Sync`. In this case, either:
//     - `T` is `Send` and was constructed on a non-main thread. Since it can only be referenced
//       immutably on the main thread, one could think of this operation as temporarily moving the
//       value to the main thread, borrowing it immutably, and then moving it back once the borrow
//       expires.
//     - `T` was constructed on the main thread. Since it can only be accessed on the main thread,
//       the property that `T` be `Sync` was never relied upon so we're fine.
//
unsafe impl<T> Sync for MainThreadJail<T> {}

impl<T> MainThreadJail<T> {
    pub const fn new_sync(value: T) -> Self
    where
        T: Sync,
    {
        Self(value)
    }

    pub const fn new_send(value: T) -> Self
    where
        T: Send,
    {
        Self(value)
    }

    pub fn new_unjail(_token: &impl UnJailRefToken<T>, value: T) -> Self {
        // `UnJailRefToken` means we're either on the main thread or `T` is `Sync`, as desired.
        Self(value)
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

// === NMainCell === //

pub struct NMainCell<T> {
    value: Cell<T>,
}

unsafe impl<T: Send> Sync for NMainCell<T> {}

impl<T: fmt::Debug + Copy> fmt::Debug for NMainCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if is_main_thread() {
            f.debug_struct("NMainCell")
                .field("value", &self.value)
                .finish()
        } else {
            f.debug_struct("NMainCell")
                .field("value", &NOT_ON_MAIN_THREAD_MSG)
                .finish()
        }
    }
}

impl<T> NMainCell<T> {
    pub const fn new(value: T) -> Self {
        Self {
            value: Cell::new(value),
        }
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.value.get_mut()
    }

    pub fn set_mut(&mut self, value: T) {
        *self.get_mut() = value;
    }

    pub fn into_inner(self) -> T {
        self.value.into_inner()
    }

    pub fn set(&self, _token: &'static MainThreadToken, value: T) {
        self.value.set(value);
    }

    pub fn replace(&self, _token: &'static MainThreadToken, value: T) -> T {
        self.value.replace(value)
    }

    pub fn swap(&self, _token: &'static MainThreadToken, other: &NMainCell<T>) {
        self.value.swap(&other.value)
    }

    pub fn get(&self, _token: &impl Token) -> T
    where
        T: Copy,
    {
        self.value.get()
    }
}

// === NOptRefCell === //

pub struct NOptRefCell<T> {
    value: OptRefCell<T>,
}

unsafe impl<T> Sync for NOptRefCell<T> {}

impl<T> NOptRefCell<T> {
    // === Constructors === //

    pub fn new(token: &impl UnJailRefToken<T>, value: Option<T>) -> Self {
        match value {
            Some(value) => Self::new_full(token, value),
            None => Self::new_empty(),
        }
    }

    pub const fn new_full(token: &impl UnJailRefToken<T>, value: T) -> Self {
        let _ = token;

        Self {
            value: OptRefCell::new_full(value),
        }
    }

    pub const fn new_empty() -> Self {
        Self {
            value: OptRefCell::new_empty(),
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

    fn assert_accessible_by(&self, token: &impl TokenFor<T>, access: Option<ThreadAccess>) {
        let can_access = match access {
            Some(access) => token.check_access(None) == Some(access),
            None => token.check_access(None).is_some(),
        };

        assert!(can_access, "{token:?} cannot access NOptRefCell.");
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

    #[track_caller]
    pub fn try_borrow<'a, 'l>(
        &'a self,
        token: &'a impl BorrowToken<T>,
        loaner: &'l PotentialImmutableBorrow<T>,
    ) -> Result<Option<OptRef<'a, T, Nothing<'l>>>, BorrowError> {
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
        self.value.try_borrow(loaner)
    }

    #[track_caller]
    pub fn borrow_or_none<'a, 'l>(
        &'a self,
        token: &'a impl BorrowToken<T>,
        loaner: &'l ImmutableBorrow<T>,
    ) -> Option<OptRef<'a, T, Nothing<'l>>> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.borrow_or_none(loaner)
    }

    #[track_caller]
    pub fn borrow<'a>(&'a self, token: &'a impl BorrowToken<T>) -> OptRef<'a, T, T> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.borrow()
    }

    #[track_caller]
    pub fn borrow_on_loan<'a, 'l>(
        &'a self,
        token: &'a impl BorrowToken<T>,
        loaner: &'l ImmutableBorrow<T>,
    ) -> OptRef<'a, T, Nothing<'l>> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.borrow_on_loan(loaner)
    }

    #[track_caller]
    pub fn try_borrow_mut<'a, 'l>(
        &'a self,
        token: &'a impl BorrowMutToken<T>,
        loaner: &'l mut PotentialMutableBorrow<T>,
    ) -> Result<Option<OptRefMut<'a, T, Nothing<'l>>>, BorrowMutError> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.try_borrow_mut(loaner)
    }

    #[track_caller]
    pub fn borrow_mut_or_none<'a, 'l>(
        &'a self,
        token: &'a impl BorrowMutToken<T>,
        loaner: &'l mut MutableBorrow<T>,
    ) -> Option<OptRefMut<'a, T, Nothing<'l>>> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.borrow_mut_or_none(loaner)
    }

    #[track_caller]
    pub fn borrow_mut<'a>(&'a self, token: &'a impl BorrowMutToken<T>) -> OptRefMut<'a, T, T> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.borrow_mut()
    }

    #[track_caller]
    pub fn borrow_mut_on_loan<'a, 'l>(
        &'a self,
        token: &'a impl BorrowMutToken<T>,
        loaner: &'l mut MutableBorrow<T>,
    ) -> OptRefMut<'a, T, Nothing<'l>> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.borrow_mut_on_loan(loaner)
    }

    // === Replace === //

    #[track_caller]
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

    #[track_caller]
    pub fn replace_with<F>(&self, token: &impl BorrowMutToken<T>, f: F) -> Option<T>
    where
        F: FnOnce(Option<&mut T>) -> Option<T>,
    {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.replace_with(f)
    }

    #[track_caller]
    pub fn try_replace(
        &self,
        token: &impl BorrowMutToken<T>,
        t: Option<T>,
    ) -> Result<Option<T>, BorrowMutError> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.try_replace(t)
    }

    #[track_caller]
    pub fn replace(&self, token: &impl BorrowMutToken<T>, t: Option<T>) -> Option<T> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.replace(t)
    }

    #[track_caller]
    pub fn take(&self, token: &impl BorrowMutToken<T>) -> Option<T> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.take()
    }

    #[track_caller]
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

    #[track_caller]
    pub fn swap(&self, token: &impl BorrowMutToken<T>, other: &NOptRefCell<T>) {
        self.swap_multi_token(token, token, other)
    }
}

impl<T: fmt::Debug> fmt::Debug for NOptRefCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if is_main_thread() {
            f.debug_struct("NOptRefCell")
                .field("value", &self.value)
                .finish()
        } else {
            f.debug_struct("NOptRefCell")
                .field("value", &NOT_ON_MAIN_THREAD_MSG)
                .finish()
        }
    }
}

// === NMultiOptRefCell === //

pub struct NMultiOptRefCell<T> {
    value: MultiOptRefCell<T>,
}

unsafe impl<T> Sync for NMultiOptRefCell<T> {}

impl<T> NMultiOptRefCell<T> {
    // === Constructors === //

    pub fn new() -> Self {
        Self {
            value: MultiOptRefCell::new(),
        }
    }

    // === Zero-cost conversions === //

    pub fn into_inner(self) -> [Option<T>; MultiRefCellIndex::COUNT] {
        // Safety: this is a method that takes exclusive access to the object. Hence, it is
        // not impacted by our potentially dangerous `Sync` impl.
        self.value.into_inner()
    }

    pub fn get_mut(&mut self) -> [Option<&mut T>; MultiRefCellIndex::COUNT] {
        // Safety: this is a method that takes exclusive access to the object. Hence, it is
        // not impacted by our potentially dangerous `Sync` impl.
        self.value.get_mut()
    }

    pub fn as_ptr(&self) -> *mut [T; MultiRefCellIndex::COUNT] {
        self.value.as_ptr()
    }

    pub fn is_empty(&self, token: &impl TokenFor<T>, i: MultiRefCellIndex) -> bool {
        self.assert_accessible_by(token, None);

        // Safety: we have either shared or exclusive access to this token and, in both cases, we
        // know that `state` cannot be mutated until the token expires thus precluding a race
        // condition.
        self.value.is_empty(i)
    }

    pub fn is_empty_mut(&mut self, i: MultiRefCellIndex) -> bool {
        // Safety: this is a method that takes exclusive access to the object. Hence, it is
        // not impacted by our potentially dangerous `Sync` impl.
        self.value.is_empty(i)
    }

    pub fn set(&mut self, i: MultiRefCellIndex, value: Option<T>) -> Option<T> {
        // Safety: this is a method that takes exclusive ownership of the object. Hence, it is
        // not impacted by our potentially dangerous `Sync` impl.
        self.value.set(i, value)
    }

    pub fn undo_leak(&mut self) {
        // Safety: this is a method that takes exclusive ownership of the object. Hence, it is
        // not impacted by our potentially dangerous `Sync` impl.
        self.value.undo_leak()
    }

    fn assert_accessible_by(&self, token: &impl TokenFor<T>, access: Option<ThreadAccess>) {
        let can_access = match access {
            Some(access) => token.check_access(None) == Some(access),
            None => token.check_access(None).is_some(),
        };

        assert!(can_access, "{token:?} cannot access NMultiOptRefCell.");
    }

    // === Borrowing === //

    pub fn try_get<'a>(
        &'a self,
        token: &'a impl GetToken<T>,
        i: MultiRefCellIndex,
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
            self.value.try_borrow_unguarded(i)
        }
    }

    pub fn get_or_none<'a>(
        &'a self,
        token: &'a impl GetToken<T>,
        i: MultiRefCellIndex,
    ) -> Option<&'a T> {
        self.assert_accessible_by(token, Some(ThreadAccess::Shared));

        unsafe {
            // Safety: see `try_get`
            self.value.borrow_unguarded_or_none(i)
        }
    }

    pub fn get<'a>(&'a self, token: &'a impl GetToken<T>, i: MultiRefCellIndex) -> &'a T {
        self.assert_accessible_by(token, Some(ThreadAccess::Shared));

        unsafe {
            // Safety: see `try_get`
            self.value.borrow_unguarded(i)
        }
    }

    #[track_caller]
    pub fn try_borrow<'a, 'l>(
        &'a self,
        token: &'a impl BorrowToken<T>,
        i: MultiRefCellIndex,
        loaner: &'l PotentialImmutableBorrow<T>,
    ) -> Result<Option<OptRef<'a, T, Nothing<'l>>>, BorrowError> {
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
        self.value.try_borrow(i, loaner)
    }

    #[track_caller]
    pub fn borrow_or_none<'a, 'l>(
        &'a self,
        token: &'a impl BorrowToken<T>,
        i: MultiRefCellIndex,
        loaner: &'l ImmutableBorrow<T>,
    ) -> Option<OptRef<'a, T, Nothing<'l>>> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.borrow_or_none(i, loaner)
    }

    #[track_caller]
    pub fn borrow<'a>(
        &'a self,
        token: &'a impl BorrowToken<T>,
        i: MultiRefCellIndex,
    ) -> OptRef<'a, T, T> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.borrow(i)
    }

    #[track_caller]
    pub fn borrow_on_loan<'a, 'l>(
        &'a self,
        token: &'a impl BorrowToken<T>,
        i: MultiRefCellIndex,
        loaner: &'l ImmutableBorrow<T>,
    ) -> OptRef<'a, T, Nothing<'l>> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.borrow_on_loan(i, loaner)
    }

    #[track_caller]
    pub fn try_borrow_mut<'a, 'l>(
        &'a self,
        token: &'a impl BorrowMutToken<T>,
        i: MultiRefCellIndex,
        loaner: &'l mut PotentialMutableBorrow<T>,
    ) -> Result<Option<OptRefMut<'a, T, Nothing<'l>>>, BorrowMutError> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.try_borrow_mut(i, loaner)
    }

    #[track_caller]
    pub fn borrow_mut_or_none<'a, 'l>(
        &'a self,
        token: &'a impl BorrowMutToken<T>,
        i: MultiRefCellIndex,
        loaner: &'l mut MutableBorrow<T>,
    ) -> Option<OptRefMut<'a, T, Nothing<'l>>> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.borrow_mut_or_none(i, loaner)
    }

    #[track_caller]
    pub fn borrow_mut<'a>(
        &'a self,
        token: &'a impl BorrowMutToken<T>,
        i: MultiRefCellIndex,
    ) -> OptRefMut<'a, T, T> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.borrow_mut(i)
    }

    #[track_caller]
    pub fn borrow_mut_on_loan<'a, 'l>(
        &'a self,
        token: &'a impl BorrowMutToken<T>,
        i: MultiRefCellIndex,
        loaner: &'l mut MutableBorrow<T>,
    ) -> OptRefMut<'a, T, Nothing<'l>> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.borrow_mut_on_loan(i, loaner)
    }

    // === Multi-borrows === //

    pub fn try_borrow_all<'a>(
        &'a self,
        token: &'a impl BorrowToken<T>,
        loaner: &'a PotentialImmutableBorrow<T>,
    ) -> Option<MultiOptRef<'a, T>> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.try_borrow_all(loaner)
    }

    pub fn try_borrow_all_mut<'a>(
        &'a self,
        token: &'a impl BorrowMutToken<T>,
        loaner: &'a mut PotentialMutableBorrow<T>,
    ) -> Option<MultiOptRefMut<'a, T>> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.try_borrow_all_mut(loaner)
    }

    // === Replace === //

    #[track_caller]
    pub fn try_replace_with<F>(
        &self,
        token: &impl BorrowMutToken<T>,
        i: MultiRefCellIndex,
        f: F,
    ) -> Result<Option<T>, BorrowMutError>
    where
        F: FnOnce(Option<&mut T>) -> Option<T>,
    {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.try_replace_with(i, f)
    }

    #[track_caller]
    pub fn replace_with<F>(
        &self,
        token: &impl BorrowMutToken<T>,
        i: MultiRefCellIndex,
        f: F,
    ) -> Option<T>
    where
        F: FnOnce(Option<&mut T>) -> Option<T>,
    {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.replace_with(i, f)
    }

    #[track_caller]
    pub fn try_replace(
        &self,
        token: &impl BorrowMutToken<T>,
        i: MultiRefCellIndex,
        t: Option<T>,
    ) -> Result<Option<T>, BorrowMutError> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.try_replace(i, t)
    }

    #[track_caller]
    pub fn replace(
        &self,
        token: &impl BorrowMutToken<T>,
        i: MultiRefCellIndex,
        t: Option<T>,
    ) -> Option<T> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.replace(i, t)
    }

    #[track_caller]
    pub fn take(&self, token: &impl BorrowMutToken<T>, i: MultiRefCellIndex) -> Option<T> {
        self.assert_accessible_by(token, Some(ThreadAccess::Exclusive));

        // Safety: see `try_borrow`.
        self.value.take(i)
    }

    #[track_caller]
    pub fn swap_multi_token(
        &self,
        my_token: &impl BorrowMutToken<T>,
        other_token: &impl BorrowMutToken<T>,
        other: &NMultiOptRefCell<T>,
        i_me: MultiRefCellIndex,
        i_other: MultiRefCellIndex,
    ) {
        self.assert_accessible_by(my_token, Some(ThreadAccess::Exclusive));
        other.assert_accessible_by(other_token, Some(ThreadAccess::Exclusive));

        self.value.swap(&other.value, i_me, i_other)
    }

    #[track_caller]
    pub fn swap(
        &self,
        token: &impl BorrowMutToken<T>,
        other: &NMultiOptRefCell<T>,
        i_me: MultiRefCellIndex,
        i_other: MultiRefCellIndex,
    ) {
        self.swap_multi_token(token, token, other, i_me, i_other)
    }
}

impl<T: fmt::Debug> fmt::Debug for NMultiOptRefCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if is_main_thread() {
            f.debug_struct("NMultiOptRefCell")
                .field("value", &self.value)
                .finish()
        } else {
            f.debug_struct("NMultiOptRefCell")
                .field("value", &NOT_ON_MAIN_THREAD_MSG)
                .finish()
        }
    }
}

impl<T> Default for NMultiOptRefCell<T> {
    fn default() -> Self {
        Self::new()
    }
}
