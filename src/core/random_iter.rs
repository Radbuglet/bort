use std::{marker::PhantomData, mem, ptr::NonNull};

use derive_where::derive_where;

// === Core === //

// RandomAccessIter
// pub type RaiItem<'i, I> = <I as RandomAccessIter<'i>>::Item;

pub trait RandomAccessIter<'i, WhereACannotOutliveSelf = &'i Self> {
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
    unsafe fn get_unchecked(&'i self, i: usize) -> Self::Item;

    fn get(&'i mut self, i: usize) -> Option<Self::Item> {
        (i < self.len()).then(|| unsafe { self.get_unchecked(i) })
    }

    fn by_mut(&'i mut self) -> BorrowedRandomAccessIter<'_, Self> {
        BorrowedRandomAccessIter::new(self)
    }
}

pub trait UnivRandomAccessIter: for<'i> RandomAccessIter<'i> {
    fn iter(&mut self) -> RandomAccessIterAdapter<BorrowedRandomAccessIter<'_, Self>> {
        self.iter_since(0)
    }

    fn iter_since(
        &mut self,
        index: usize,
    ) -> RandomAccessIterAdapter<BorrowedRandomAccessIter<'_, Self>> {
        let len = self.len();

        RandomAccessIterAdapter {
            iter: BorrowedRandomAccessIter::new(self),
            index,
            len,
        }
    }
}

impl<I: for<'i> RandomAccessIter<'i>> UnivRandomAccessIter for I {}

// UntiedRandomAccessIter
pub trait UntiedRandomAccessIter: for<'i> RandomAccessIter<'i, Item = Self::UntiedItem> {
    type UntiedItem;

    fn into_iter(self) -> RandomAccessIterAdapter<Self>
    where
        Self: Sized,
    {
        self.iter_since(0)
    }

    fn iter_since(self, index: usize) -> RandomAccessIterAdapter<Self>
    where
        Self: Sized,
    {
        let len = self.len();

        RandomAccessIterAdapter {
            iter: self,
            index,
            len,
        }
    }
}

// BorrowedRandomAccessIter
pub struct BorrowedRandomAccessIter<'a, I: ?Sized>(&'a I);

impl<'a, I: ?Sized> BorrowedRandomAccessIter<'a, I> {
    pub fn new(v: &'a mut I) -> Self {
        Self(v)
    }
}

impl<'i, 'a, I> RandomAccessIter<'i> for BorrowedRandomAccessIter<'a, I>
where
    I: ?Sized + RandomAccessIter<'a>,
{
    type Item = I::Item;

    const IS_FINITE: bool = I::IS_FINITE;

    fn len(&self) -> usize {
        self.0.len()
    }

    unsafe fn get_unchecked(&'i self, i: usize) -> Self::Item {
        self.0.get_unchecked(i)
    }
}

impl<'a, I> UntiedRandomAccessIter for BorrowedRandomAccessIter<'a, I>
where
    I: ?Sized + RandomAccessIter<'a>,
{
    type UntiedItem = I::Item;
}

// RandomAccessIterAdapter
#[derive(Debug, Clone)]
pub struct RandomAccessIterAdapter<I: UntiedRandomAccessIter> {
    iter: I,
    index: usize,
    len: usize,
}

impl<I: UntiedRandomAccessIter> Iterator for RandomAccessIterAdapter<I> {
    type Item = I::UntiedItem;

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

impl<'i, 'a, T> RandomAccessIter<'i> for RandomAccessSliceRef<'a, T> {
    type Item = &'a T;

    const IS_FINITE: bool = true;

    fn len(&self) -> usize {
        self.0.len()
    }

    unsafe fn get_unchecked(&'i self, i: usize) -> &'a T {
        self.0.get_unchecked(i)
    }
}

impl<'b, T> UntiedRandomAccessIter for RandomAccessSliceRef<'b, T> {
    type UntiedItem = &'b T;
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

impl<'i, 'a, T> RandomAccessIter<'i> for RandomAccessSliceMut<'a, T> {
    type Item = &'a mut T;

    const IS_FINITE: bool = true;

    fn len(&self) -> usize {
        self.ptr.len()
    }

    unsafe fn get_unchecked(&'i self, i: usize) -> &'a mut T {
        unsafe { &mut *self.ptr.as_ptr().cast::<T>().add(i) }
    }
}

impl<'a, T> UntiedRandomAccessIter for RandomAccessSliceMut<'a, T> {
    type UntiedItem = &'a mut T;
}

pub struct RandomAccessVec<T> {
    _ty: PhantomData<Vec<T>>,
    ptr: *mut T,
    len: usize,
    cap: usize,
}

impl<T> RandomAccessVec<T> {
    pub fn new(mut vec: Vec<T>) -> Self {
        let len = vec.len();
        let cap = vec.capacity();
        let ptr = vec.as_mut_ptr();
        mem::forget(vec);

        Self {
            _ty: PhantomData,
            ptr,
            len,
            cap,
        }
    }
}

impl<'i, T> RandomAccessIter<'i> for RandomAccessVec<T> {
    type Item = &'i mut T;

    const IS_FINITE: bool = true;

    fn len(&self) -> usize {
        self.len
    }

    unsafe fn get_unchecked(&'i self, i: usize) -> Self::Item {
        &mut *self.ptr.add(i)
    }
}

impl<T> Drop for RandomAccessVec<T> {
    fn drop(&mut self) {
        drop(unsafe { Vec::from_raw_parts(self.ptr, self.len, self.cap) });
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

impl<'i, A: RandomAccessIter<'i>, B: RandomAccessIter<'i>> RandomAccessIter<'i>
    for RandomAccessZip<A, B>
{
    type Item = (A::Item, B::Item);

    const IS_FINITE: bool = A::IS_FINITE || B::IS_FINITE;

    fn len(&self) -> usize {
        self.0.len().min(self.1.len())
    }

    unsafe fn get_unchecked(&'i self, i: usize) -> Self::Item {
        (self.0.get_unchecked(i), self.1.get_unchecked(i))
    }
}

impl<A, B> UntiedRandomAccessIter for RandomAccessZip<A, B>
where
    A: UntiedRandomAccessIter,
    B: UntiedRandomAccessIter,
{
    type UntiedItem = (A::UntiedItem, B::UntiedItem);
}

// === Map === //

#[derive(Clone)]
pub struct RandomAccessMap<I, F>(I, F);

impl<I, F> RandomAccessMap<I, F> {
    pub fn new(iter: I, mapper: F) -> Self {
        Self(iter, mapper)
    }
}

impl<'i, I, F> RandomAccessIter<'i> for RandomAccessMap<I, F>
where
    I: RandomAccessIter<'i>,
    F: RandomAccessMapper<I::Item>,
{
    type Item = F::Output;

    const IS_FINITE: bool = I::IS_FINITE;

    fn len(&self) -> usize {
        self.0.len()
    }

    unsafe fn get_unchecked(&'i self, i: usize) -> Self::Item {
        self.1.map(i, self.0.get_unchecked(i))
    }
}

impl<I, F> UntiedRandomAccessIter for RandomAccessMap<I, F>
where
    I: UntiedRandomAccessIter,
    F: RandomAccessMapper<I::UntiedItem>,
{
    type UntiedItem = F::Output;
}

// Mappers
pub trait RandomAccessMapper<I> {
    type Output;

    fn map(&self, index: usize, item: I) -> Self::Output;
}

impl<F: ?Sized + Fn(usize, I) -> O, I, O> RandomAccessMapper<I> for F {
    type Output = O;

    fn map(&self, index: usize, item: I) -> Self::Output {
        self(index, item)
    }
}

// === Repeat === //

#[derive(Debug, Clone)]
pub struct RandomAccessRepeat<T>(T);

impl<T> RandomAccessRepeat<T> {
    pub fn new(value: T) -> Self {
        Self(value)
    }
}

impl<'i, T: Clone> RandomAccessIter<'i> for RandomAccessRepeat<T> {
    type Item = T;

    const IS_FINITE: bool = false;

    fn len(&self) -> usize {
        usize::MAX
    }

    unsafe fn get_unchecked(&'i self, _i: usize) -> T {
        self.0.clone()
    }
}

impl<T: Clone> UntiedRandomAccessIter for RandomAccessRepeat<T> {
    type UntiedItem = T;
}

// === Enumerate === //

#[derive(Debug, Clone)]
pub struct RandomAccessEnumerate;

impl<'i> RandomAccessIter<'i> for RandomAccessEnumerate {
    type Item = usize;

    const IS_FINITE: bool = false;

    fn len(&self) -> usize {
        usize::MAX
    }

    unsafe fn get_unchecked(&'i self, i: usize) -> usize {
        i
    }
}

impl UntiedRandomAccessIter for RandomAccessEnumerate {
    type UntiedItem = usize;
}

// === Take === //

#[derive(Debug, Clone)]
pub struct RandomAccessTake<T>(T, usize);

impl<I> RandomAccessTake<I> {
    pub fn new(iter: I, len: usize) -> Self {
        Self(iter, len)
    }
}

impl<'i, I: RandomAccessIter<'i>> RandomAccessIter<'i> for RandomAccessTake<I> {
    type Item = I::Item;

    const IS_FINITE: bool = true;

    fn len(&self) -> usize {
        self.0.len().min(self.1)
    }

    unsafe fn get_unchecked(&'i self, i: usize) -> Self::Item {
        self.0.get_unchecked(i)
    }
}

impl<I: UntiedRandomAccessIter> UntiedRandomAccessIter for RandomAccessTake<I> {
    type UntiedItem = I::UntiedItem;
}
