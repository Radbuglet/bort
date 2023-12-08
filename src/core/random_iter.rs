use std::{marker::PhantomData, ptr::NonNull};

use derive_where::derive_where;

// === RandomAccessMapper === //

// Public traits
pub trait RandomAccessMapper<I>: Sized {
    type Output;

    fn map(&self, idx: usize, input: I) -> Self::Output;
}

pub trait RandomAccessMapperUntied {
    type UntiedOutput;
}

pub trait RandomAccessMapperUntiedUsingInput<I>:
    RandomAccessMapperUntied + RandomAccessMapper<I, Output = Self::UntiedOutput>
{
}

impl<F: Fn(usize, I) -> O, I, O> RandomAccessMapper<I> for F {
    type Output = O;

    fn map(&self, idx: usize, input: I) -> Self::Output {
        self(idx, input)
    }
}

// === MaybeContainerTied === //

type Invariant<T> = PhantomData<fn(T) -> T>;

// Core
type MctResolve<'a, M> = <M as MaybeContainerTied<'a>>::Borrowed;
type MctResolveUntied<M> = <M as NotContainedTied>::BorrowedUntied;

pub trait MaybeContainerTied<'a>: Sized {
    type Borrowed;
}

pub trait NotContainedTied: UMaybeContainerTied {
    type BorrowedUntied;

    fn cast_untie<'a>(borrowed: MctResolve<'a, Self>) -> Self::BorrowedUntied
    where
        Self: 'a;
}

// HRTB helpers
mod mct_with_bind {
    use super::MaybeContainerTied;

    pub trait MaybeContainerTiedWithBind<'a, Bound: ?Sized>: MaybeContainerTied<'a> {}

    impl<'a, T: MaybeContainerTied<'a>, Bound: ?Sized> MaybeContainerTiedWithBind<'a, Bound> for T {}
}

pub trait UMaybeContainerTied:
    for<'a> mct_with_bind::MaybeContainerTiedWithBind<'a, [&'a Self; 0]>
{
}

impl<T> UMaybeContainerTied for T where
    T: for<'a> mct_with_bind::MaybeContainerTiedWithBind<'a, [&'a Self; 0]>
{
}

// NotContainerTied
pub struct TrivialNotContainerTied<T>(Invariant<T>);

impl<'a, T> MaybeContainerTied<'a> for TrivialNotContainerTied<T> {
    type Borrowed = T;
}

impl<T> NotContainedTied for TrivialNotContainerTied<T> {
    type BorrowedUntied = T;

    fn cast_untie<'a>(borrowed: MctResolve<'a, Self>) -> Self::BorrowedUntied
    where
        Self: 'a,
    {
        borrowed
    }
}

// ZipTied
pub struct ZipTied<A, B>(Invariant<(A, B)>);

impl<'a, A: UMaybeContainerTied, B: UMaybeContainerTied> MaybeContainerTied<'a> for ZipTied<A, B> {
    type Borrowed = (MctResolve<'a, A>, MctResolve<'a, B>);
}

impl<A: NotContainedTied, B: NotContainedTied> NotContainedTied for ZipTied<A, B> {
    type BorrowedUntied = (A::BorrowedUntied, B::BorrowedUntied);

    fn cast_untie<'a>((left, right): MctResolve<'a, Self>) -> Self::BorrowedUntied
    where
        Self: 'a,
    {
        (A::cast_untie(left), B::cast_untie(right))
    }
}

// MapTied
pub struct MapTied<I, F>(Invariant<(I, F)>);

impl<'a, I, F> MaybeContainerTied<'a> for MapTied<I, F>
where
    I: UMaybeContainerTied,
    F: for<'b> RandomAccessMapper<MctResolve<'b, I>>,
{
    type Borrowed = <F as RandomAccessMapper<MctResolve<'a, I>>>::Output;
}

impl<I, F> NotContainedTied for MapTied<I, F>
where
    I: UMaybeContainerTied,
    F: for<'a> RandomAccessMapperUntiedUsingInput<MctResolve<'a, I>>,
{
    type BorrowedUntied = F::UntiedOutput;

    fn cast_untie<'a>(borrowed: MctResolve<'a, Self>) -> Self::BorrowedUntied
    where
        Self: 'a,
    {
        borrowed
    }
}

// ContainerTiedRef
pub struct ContainerTiedRef<T: ?Sized>(Invariant<T>);

impl<'a, T: ?Sized + 'a> MaybeContainerTied<'a> for ContainerTiedRef<T> {
    type Borrowed = &'a T;
}

// ContainedTiedMut
pub struct ContainedTiedMut<T: ?Sized>(Invariant<T>);

impl<'a, T: ?Sized + 'a> MaybeContainerTied<'a> for ContainedTiedMut<T> {
    type Borrowed = &'a mut T;
}

// === RandomAccessIter === //

pub trait RandomAccessIter {
    type Item: UMaybeContainerTied;

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
    unsafe fn get_unchecked(&self, i: usize) -> MctResolve<'_, Self::Item>;

    fn get(&mut self, i: usize) -> Option<MctResolve<'_, Self::Item>> {
        (i < self.len()).then(|| unsafe { self.get_unchecked(i) })
    }

    fn iter(&mut self) -> RandomAccessIterAdapter<BorrowedRandomAccessIter<'_, Self>> {
        self.iter_since(0)
    }

    fn iter_since(
        &mut self,
        index: usize,
    ) -> RandomAccessIterAdapter<BorrowedRandomAccessIter<'_, Self>> {
        let len = if Self::IS_FINITE { self.len() } else { 0 };

        RandomAccessIterAdapter {
            iter: BorrowedRandomAccessIter::new(self),
            index,
            len,
        }
    }
}

pub trait RandomAccessIterUntied: RandomAccessIter<Item = Self::ItemUntied> {
    type ItemUntied: NotContainedTied;

    fn into_iter(self) -> RandomAccessIterAdapter<Self>
    where
        Self: Sized,
    {
        self.into_iter_since(0)
    }

    fn into_iter_since(self, index: usize) -> RandomAccessIterAdapter<Self>
    where
        Self: Sized,
    {
        let len = if Self::IS_FINITE { self.len() } else { 0 };

        RandomAccessIterAdapter {
            iter: self,
            index,
            len,
        }
    }
}

impl<I: ?Sized + RandomAccessIter> RandomAccessIterUntied for I
where
    I::Item: NotContainedTied,
{
    type ItemUntied = I::Item;
}

pub struct BorrowedRandomAccessIter<'a, I: ?Sized>(&'a I);

impl<'a, I: ?Sized> BorrowedRandomAccessIter<'a, I> {
    pub fn new(v: &'a mut I) -> Self {
        Self(v)
    }
}

impl<'a, I: ?Sized + RandomAccessIter> RandomAccessIter for BorrowedRandomAccessIter<'a, I>
where
    I::Item: 'a,
{
    type Item = TrivialNotContainerTied<MctResolve<'a, I::Item>>;

    const IS_FINITE: bool = I::IS_FINITE;

    fn len(&self) -> usize {
        self.0.len()
    }

    unsafe fn get_unchecked(&self, i: usize) -> MctResolve<'a, I::Item> {
        self.0.get_unchecked(i)
    }
}

#[derive(Debug, Clone)]
pub struct RandomAccessIterAdapter<I> {
    iter: I,
    index: usize,
    len: usize,
}

impl<I: RandomAccessIter> Iterator for RandomAccessIterAdapter<I>
where
    I::Item: NotContainedTied,
{
    type Item = MctResolveUntied<I::Item>;

    fn next(&mut self) -> Option<Self::Item> {
        (self.index < self.len).then(|| {
            let item = <I::Item>::cast_untie(unsafe { self.iter.get_unchecked(self.index) });
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
    type Item = TrivialNotContainerTied<&'a T>;

    const IS_FINITE: bool = true;

    fn len(&self) -> usize {
        self.0.len()
    }

    unsafe fn get_unchecked(&self, i: usize) -> &'a T {
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
    type Item = TrivialNotContainerTied<&'a mut T>;

    const IS_FINITE: bool = true;

    fn len(&self) -> usize {
        self.ptr.len()
    }

    unsafe fn get_unchecked(&self, i: usize) -> &'a mut T {
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
    type Item = ZipTied<A::Item, B::Item>;

    const IS_FINITE: bool = A::IS_FINITE || B::IS_FINITE;

    fn len(&self) -> usize {
        self.0.len().min(self.1.len())
    }

    unsafe fn get_unchecked(&self, i: usize) -> MctResolve<'_, Self::Item> {
        (self.0.get_unchecked(i), self.1.get_unchecked(i))
    }
}

// === Map === //

#[derive(Clone)]
pub struct RandomAccessMap<I, F>(I, F);

impl<I, F> RandomAccessMap<I, F> {
    pub fn new(iter: I, mapper: F) -> Self {
        Self(iter, mapper)
    }
}

impl<I, F> RandomAccessIter for RandomAccessMap<I, F>
where
    I: RandomAccessIter,
    F: for<'a> RandomAccessMapper<MctResolve<'a, I::Item>>,
{
    type Item = MapTied<I::Item, F>;

    const IS_FINITE: bool = I::IS_FINITE;

    fn len(&self) -> usize {
        self.0.len()
    }

    unsafe fn get_unchecked(&self, i: usize) -> MctResolve<'_, Self::Item> {
        self.1.map(i, self.0.get_unchecked(i))
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

impl<T: Clone> RandomAccessIter for RandomAccessRepeat<T> {
    type Item = TrivialNotContainerTied<T>;

    const IS_FINITE: bool = false;

    fn len(&self) -> usize {
        usize::MAX
    }

    unsafe fn get_unchecked(&self, _i: usize) -> T {
        self.0.clone()
    }
}

// === Enumerate === //

#[derive(Debug, Clone)]
pub struct RandomAccessEnumerate;

impl RandomAccessIter for RandomAccessEnumerate {
    type Item = TrivialNotContainerTied<usize>;

    const IS_FINITE: bool = false;

    fn len(&self) -> usize {
        usize::MAX
    }

    unsafe fn get_unchecked(&self, i: usize) -> usize {
        i
    }
}

// === Take === //

#[derive(Debug, Clone)]
pub struct RandomAccessTake<T>(T, usize);

impl<I> RandomAccessTake<I> {
    pub fn new(iter: I, len: usize) -> Self {
        Self(iter, len)
    }
}

impl<I: RandomAccessIter> RandomAccessIter for RandomAccessTake<I> {
    type Item = I::Item;

    const IS_FINITE: bool = true;

    fn len(&self) -> usize {
        self.0.len().min(self.1)
    }

    unsafe fn get_unchecked(&self, i: usize) -> MctResolve<'_, I::Item> {
        self.0.get_unchecked(i)
    }
}
