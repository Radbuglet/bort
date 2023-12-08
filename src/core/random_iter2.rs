use std::{marker::PhantomData, ptr::NonNull};

use derive_where::derive_where;

// === RandomAccessMapper === //

// Public traits
pub trait RandomAccessMapper<I> {
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

// === MaybeContainerTied === //

type Invariant<T> = PhantomData<fn(T) -> T>;

// Core
type MctResolve<'a, M> = <M as MaybeContainerTied>::Borrowed<'a>;
type MctResolveUntied<M> = <M as ActuallyNotContainerTied>::BorrowedUntied;

pub trait MaybeContainerTied: Sized {
    type Borrowed<'a>
    where
        Self: 'a;
}

pub trait ActuallyNotContainerTied: MaybeContainerTied {
    type BorrowedUntied;

    fn cast_untie<'a>(borrowed: Self::Borrowed<'a>) -> Self::BorrowedUntied
    where
        Self: 'a;
}

// NotContainerTied
pub struct NotContainerTied<T>(Invariant<T>);

impl<T> MaybeContainerTied for NotContainerTied<T> {
    type Borrowed<'a> = T where Self: 'a;
}

impl<T> ActuallyNotContainerTied for NotContainerTied<T> {
    type BorrowedUntied = T;

    fn cast_untie<'a>(borrowed: Self::Borrowed<'a>) -> Self::BorrowedUntied
    where
        Self: 'a,
    {
        borrowed
    }
}

// ZipTied
pub struct ZipTied<A, B>(Invariant<(A, B)>);

impl<A: MaybeContainerTied, B: MaybeContainerTied> MaybeContainerTied for ZipTied<A, B> {
    type Borrowed<'a> = (A::Borrowed<'a>, B::Borrowed<'a>)
    where
        Self: 'a;
}

impl<A: ActuallyNotContainerTied, B: ActuallyNotContainerTied> ActuallyNotContainerTied
    for ZipTied<A, B>
{
    type BorrowedUntied = (A::BorrowedUntied, B::BorrowedUntied);

    fn cast_untie<'a>((left, right): Self::Borrowed<'a>) -> Self::BorrowedUntied
    where
        Self: 'a,
    {
        (A::cast_untie(left), B::cast_untie(right))
    }
}

// MapTied
pub struct MapTied<I, F>(Invariant<(I, F)>);

impl<I, F> MaybeContainerTied for MapTied<I, F>
where
    I: MaybeContainerTied,
    F: for<'a> RandomAccessMapper<MctResolve<'a, I>>,
{
    type Borrowed<'a> = <F as RandomAccessMapper<MctResolve<'a, I>>>::Output
    where
        Self: 'a;
}

impl<I, F> ActuallyNotContainerTied for MapTied<I, F>
where
    I: MaybeContainerTied,
    F: for<'a> RandomAccessMapperUntiedUsingInput<MctResolve<'a, I>>,
{
    type BorrowedUntied = F::UntiedOutput;

    fn cast_untie<'a>(borrowed: Self::Borrowed<'a>) -> Self::BorrowedUntied
    where
        Self: 'a,
    {
        borrowed
    }
}

// ContainerTiedRef
pub struct ContainerTiedRef<T: ?Sized>(Invariant<T>);

impl<T: ?Sized> MaybeContainerTied for ContainerTiedRef<T> {
    type Borrowed<'a> = &'a T where Self: 'a;
}

// ContainedTiedMut
pub struct ContainedTiedMut<T: ?Sized>(Invariant<T>);

impl<T: ?Sized> MaybeContainerTied for ContainedTiedMut<T> {
    type Borrowed<'a> = &'a mut T where Self: 'a;
}

// === RandomAccessIter === //

pub trait RandomAccessIter {
    type Item: MaybeContainerTied;

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
    type Item = NotContainerTied<MctResolve<'a, I::Item>>;

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
    I::Item: ActuallyNotContainerTied,
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
    type Item = NotContainerTied<&'a T>;

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
    type Item = NotContainerTied<&'a mut T>;

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
    type Item = NotContainerTied<T>;

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
    type Item = NotContainerTied<usize>;

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
