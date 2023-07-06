use std::{
    any::{Any, TypeId},
    borrow::Borrow,
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

#[derive(Copy, Clone)]
pub struct ListFmt<I>(pub I);

impl<I> fmt::Debug for ListFmt<I>
where
    I: Clone + IntoIterator,
    I::Item: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.0.clone().into_iter()).finish()
    }
}

#[derive(Copy, Clone)]
pub struct MapFmt<I>(pub I);

impl<I, A, B> fmt::Debug for MapFmt<I>
where
    I: Clone + IntoIterator<Item = (A, B)>,
    A: fmt::Debug,
    B: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.0.clone().into_iter()).finish()
    }
}

// === NamedTypeId === //

#[derive(Copy, Clone)]
#[cfg_attr(not(debug_assertions), derive(Eq, PartialEq, Ord, PartialOrd, Hash))]
#[cfg_attr(
    debug_assertions,
    derive_where::derive_where(Eq, PartialEq, Ord, PartialOrd, Hash)
)]
pub struct NamedTypeId {
    id: TypeId,
    #[cfg(debug_assertions)]
    #[derive_where(skip)]
    name: Option<&'static str>,
}

impl fmt::Debug for NamedTypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        #[cfg(debug_assertions)]
        if let Some(name) = self.name {
            return write!(f, "TypeId<{name}>");
        }

        self.id.fmt(f)
    }
}

impl NamedTypeId {
    pub fn of<T: ?Sized + 'static>() -> Self {
        Self {
            id: TypeId::of::<T>(),
            #[cfg(debug_assertions)]
            name: Some(std::any::type_name::<T>()),
        }
    }

    pub fn from_raw(id: TypeId) -> Self {
        Self {
            id,
            #[cfg(debug_assertions)]
            name: None,
        }
    }

    pub fn raw(self) -> TypeId {
        self.id
    }
}

impl Borrow<TypeId> for NamedTypeId {
    fn borrow(&self) -> &TypeId {
        &self.id
    }
}

impl From<NamedTypeId> for TypeId {
    fn from(id: NamedTypeId) -> Self {
        id.raw()
    }
}

impl From<TypeId> for NamedTypeId {
    fn from(raw: TypeId) -> Self {
        Self::from_raw(raw)
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
