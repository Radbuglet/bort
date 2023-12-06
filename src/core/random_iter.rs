//! A re-implementation of the standard library's `TrustedRandomAccess` trait such that non-nightly
//! users like us can implement it.

use std::{marker::PhantomData, ptr::NonNull};

use derive_where::derive_where;

// === Core === //

pub trait RandomAccessIter {
    type Item;

    const IS_FINITE: bool;

    /// Returns the length of the iterator. This value should not change unless this iterator is
    /// borrowed mutably.
    fn len(&self) -> usize;

    /// Fetches a random element in the iterator without bounds or alias checking.
    ///
    /// ## Safety
    ///
    /// - `i` must be less than the most recent value returned by `len` which has not been
    ///   invalidated by some form of explicit container mutation.
    /// - `i` must not be actively borrowed. Note that clones of this object are assumed to have a
    ///   separate set of objects to borrow or are set up in such a way that this fact is effectively
    ///   true (i.e. either the underlying container is cloned or all references are actually immutable).
    ///
    #[allow(clippy::mut_from_ref)]
    unsafe fn get_unchecked(&self, i: usize) -> Self::Item;

    fn iter(&mut self) -> RandomAccessIterAdapter<&mut Self> {
        let len = if Self::IS_FINITE { self.len() } else { 0 };

        RandomAccessIterAdapter {
            iter: self,
            index: 0,
            len,
        }
    }

    fn into_iter(self) -> RandomAccessIterAdapter<Self>
    where
        Self: Sized,
    {
        let len = if Self::IS_FINITE { self.len() } else { 0 };

        RandomAccessIterAdapter {
            iter: self,
            index: 0,
            len,
        }
    }
}

impl<'a, I: RandomAccessIter> RandomAccessIter for &'a mut I {
    type Item = I::Item;

    const IS_FINITE: bool = I::IS_FINITE;

    fn len(&self) -> usize {
        (**self).len()
    }

    unsafe fn get_unchecked(&self, i: usize) -> Self::Item {
        (**self).get_unchecked(i)
    }
}

#[derive(Debug, Clone)]
pub struct RandomAccessIterAdapter<I> {
    iter: I,
    index: usize,
    len: usize,
}

impl<I: RandomAccessIter> Iterator for RandomAccessIterAdapter<I> {
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        (self.index < self.len).then(|| {
            let item = unsafe { self.iter.get_unchecked(self.index) };
            self.index += 1;
            item
        })
    }
}

// === Slice === //

#[derive(Debug)]
#[derive_where(Clone)]
pub struct RandomAccessSliceRef<'a, T>(&'a [T]);

impl<'a, T> RandomAccessSliceRef<'a, T> {
    pub fn new(slice: &'a [T]) -> Self {
        Self(slice)
    }
}

impl<'a, T> RandomAccessIter for RandomAccessSliceRef<'a, T> {
    type Item = &'a T;

    const IS_FINITE: bool = true;

    fn len(&self) -> usize {
        self.0.len()
    }

    unsafe fn get_unchecked(&self, i: usize) -> Self::Item {
        self.0.get_unchecked(i)
    }
}

pub struct RandomAccessSliceMut<'a, T> {
    _ty: PhantomData<&'a mut [T]>,
    ptr: NonNull<[T]>,
}

impl<'a, T> RandomAccessSliceMut<'a, T> {
    pub fn new(slice: &'a mut [T]) -> Self {
        Self {
            _ty: PhantomData,
            ptr: NonNull::from(slice),
        }
    }
}

impl<'a, T> RandomAccessIter for RandomAccessSliceMut<'a, T> {
    type Item = &'a mut T;

    const IS_FINITE: bool = true;

    fn len(&self) -> usize {
        self.ptr.len()
    }

    unsafe fn get_unchecked(&self, i: usize) -> Self::Item {
        unsafe { &mut *self.ptr.as_ptr().cast::<T>().add(i) }
    }
}

// === Zip === //

#[derive(Clone)]
pub struct RandomAccessZip<A, B>(A, B);

impl<A, B> RandomAccessZip<A, B> {
    pub fn new(a: A, b: B) -> Self {
        Self(a, b)
    }
}

impl<A: RandomAccessIter, B: RandomAccessIter> RandomAccessIter for RandomAccessZip<A, B> {
    type Item = (A::Item, B::Item);

    const IS_FINITE: bool = A::IS_FINITE || B::IS_FINITE;

    fn len(&self) -> usize {
        self.0.len().min(self.1.len())
    }

    unsafe fn get_unchecked(&self, i: usize) -> Self::Item {
        (self.0.get_unchecked(i), self.1.get_unchecked(i))
    }
}

// === Map === //

#[derive(Clone)]
pub struct RandomAccessMap<I, F>(I, F);

pub trait RandomAccessMapper<I> {
    type Output;

    fn map(&self, i: I) -> Self::Output;
}

impl<F, I, O> RandomAccessMapper<I> for F
where
    F: Fn(I) -> O,
{
    type Output = O;

    fn map(&self, i: I) -> Self::Output {
        self(i)
    }
}

impl<I, F> RandomAccessMap<I, F> {
    pub fn new(iter: I, mapper: F) -> Self {
        Self(iter, mapper)
    }
}

impl<I, F> RandomAccessIter for RandomAccessMap<I, F>
where
    I: RandomAccessIter,
    F: RandomAccessMapper<I::Item>,
{
    type Item = F::Output;

    const IS_FINITE: bool = I::IS_FINITE;

    fn len(&self) -> usize {
        self.0.len()
    }

    unsafe fn get_unchecked(&self, i: usize) -> Self::Item {
        self.1.map(self.0.get_unchecked(i))
    }
}
