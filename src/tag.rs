use std::{fmt, marker::PhantomData, num::NonZeroU64};

use crate::util::random_uid;

// === Tag === //

pub struct Tag<T> {
    _ty: PhantomData<fn() -> T>,
    id: NonZeroU64,
}

impl<T> Tag<T> {
    pub fn new() -> Self {
        Self {
            _ty: PhantomData,
            id: random_uid(),
        }
    }
}

impl<T> fmt::Debug for Tag<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tag").field("id", &self.id).finish()
    }
}

impl<T> Copy for Tag<T> {}

impl<T> Clone for Tag<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Eq for Tag<T> {}

impl<T> PartialEq for Tag<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
