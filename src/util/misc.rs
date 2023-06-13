use std::{
    any::Any,
    cell::Cell,
    error::Error,
    fmt,
    num::NonZeroU64,
    sync::{MutexGuard, PoisonError},
};

// === Random IDs === //

pub fn xorshift64(state: NonZeroU64) -> NonZeroU64 {
    // Adapted from: https://en.wikipedia.org/w/index.php?title=Xorshift&oldid=1123949358
    let state = state.get();
    let state = state ^ (state << 13);
    let state = state ^ (state >> 7);
    let state = state ^ (state << 17);
    NonZeroU64::new(state).unwrap()
}

pub fn random_thread_local_uid() -> NonZeroU64 {
    thread_local! {
        static ID_GEN: Cell<NonZeroU64> = const { Cell::new(const_new_nz_u64(1)) };
    }

    ID_GEN.with(|v| {
        // N.B. `xorshift`, like all other well-constructed LSFRs, produces a full cycle of non-zero
        // values before repeating itself. Thus, this is an effective way to generate random but
        // unique IDs without using additional storage.
        let state = xorshift64(v.get());
        v.set(state);
        state
    })
}

// === Downcast === //

pub trait AnyDowncastExt: Any {
    fn as_any(&self) -> &dyn Any;

    fn as_any_mut(&mut self) -> &mut dyn Any;

    fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        self.as_any().downcast_ref()
    }

    fn downcast_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.as_any_mut().downcast_mut()
    }
}

// Rust currently doesn't have inherent downcast impls for `dyn (Any + Sync)`.
impl AnyDowncastExt for dyn Any + Sync {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// === RawFmt === //

pub const NOT_ON_MAIN_THREAD_MSG: RawFmt = RawFmt("<not on main thread>");

#[derive(Copy, Clone)]
pub struct RawFmt<'a>(pub &'a str);

impl fmt::Debug for RawFmt<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

// === Misc === //

pub fn leak<T>(value: T) -> &'static T {
    Box::leak(Box::new(value))
}

pub const fn const_new_nz_u64(v: u64) -> NonZeroU64 {
    match NonZeroU64::new(v) {
        Some(v) => v,
        None => unreachable!(),
    }
}

pub fn unpoison<'a, T: ?Sized>(
    guard: Result<MutexGuard<'a, T>, PoisonError<MutexGuard<'a, T>>>,
) -> MutexGuard<'a, T> {
    match guard {
        Ok(guard) => guard,
        Err(err) => err.into_inner(),
    }
}

pub fn unwrap_error<T, E: Error>(result: Result<T, E>) -> T {
    result.unwrap_or_else(|e| panic!("{e}"))
}
