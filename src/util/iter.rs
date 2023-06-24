use std::{hash, iter, sync::Arc};

use derive_where::derive_where;

use super::misc::impl_tuples;

pub fn hash_iter<H, E, I>(hasher: &mut H, iter: I) -> u64
where
    H: hash::BuildHasher,
    E: hash::Hash,
    I: IntoIterator<Item = E>,
{
    let mut state = hasher.build_hasher();
    for item in iter {
        item.hash(&mut state);
    }
    hash::Hasher::finish(&state)
}

pub fn eq_iter<A, B, F>(a: A, b: B, mut f: F) -> bool
where
    A: IntoIterator,
    B: IntoIterator,
    F: FnMut(A::Item, B::Item) -> bool,
{
    let mut a = a.into_iter();
    let mut b = b.into_iter();

    loop {
        match (a.next(), b.next()) {
            (Some(a), Some(b)) => {
                if f(a, b) {
                    continue;
                } else {
                    return false;
                }
            }
            (None, None) => return true,
            _ => return false,
        }
    }
}

#[derive_where(Clone; A: Clone, B: Clone, A::Item: Clone, B::Item: Clone)]
pub struct IterMerger<A: Iterator, B: Iterator> {
    a_iter: iter::Peekable<A>,
    b_iter: iter::Peekable<B>,
}

impl<I, A, B> Iterator for IterMerger<A, B>
where
    I: Ord,
    A: Iterator<Item = I>,
    B: Iterator<Item = I>,
{
    type Item = I;

    fn next(&mut self) -> Option<Self::Item> {
        // Unfortunately, `Option`'s default Ord impl isn't suitable for this.
        match (self.a_iter.peek(), self.b_iter.peek()) {
            (Some(a), Some(b)) => {
                if a < b {
                    self.a_iter.next()
                } else {
                    self.b_iter.next()
                }
            }
            (Some(_), None) => self.a_iter.next(),
            (None, Some(_)) => self.b_iter.next(),
            (None, None) => None,
        }
    }
}

pub fn merge_iters<I, A, B>(a: A, b: B) -> IterMerger<A::IntoIter, B::IntoIter>
where
    A: IntoIterator<Item = I>,
    B: IntoIterator<Item = I>,
{
    IterMerger {
        a_iter: a.into_iter().peekable(),
        b_iter: b.into_iter().peekable(),
    }
}

pub fn filter_duplicates<T: PartialEq>(
    iter: impl IntoIterator<Item = T>,
) -> impl Iterator<Item = T> {
    let mut iter = iter.into_iter().peekable();

    iter::from_fn(move || {
        let mut next = iter.next()?;

        // Skip forward so long as our `next` element equals the element after it.
        while Some(&next) == iter.peek() {
            next = iter.next().unwrap();
        }

        Some(next)
    })
}

#[derive(Clone)]
pub struct IterFilter<I: Iterator>(pub I, pub I::Item);

impl<I> Iterator for IterFilter<I>
where
    I: Iterator,
    I::Item: PartialEq,
{
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        (&mut self.0).find(|v| v != &self.1)
    }
}

pub fn arc_into_iter<T, V>(
    arc: Arc<[T]>,
    len: usize,
    mut f: impl FnMut(&T) -> V,
) -> impl Iterator<Item = V> {
    debug_assert!(len <= arc.len());

    let mut index = 0;

    iter::from_fn(move || {
        if index < len {
            let element = f(&arc[index]);
            index += 1;
            Some(element)
        } else {
            None
        }
    })
}

#[derive(Debug, Clone)]
pub struct ZipIter<T>(pub T);

macro_rules! impl_zip_iter {
	($($ty:ident:$field:tt),*) => {
		impl<$($ty: Iterator),*> Iterator for ZipIter<($($ty,)*)> {
			type Item = ($($ty::Item,)*);

			fn next(&mut self) -> Option<Self::Item> {
				Some(($(self.0.$field.next()?,)*))
			}
		}
	};
}

impl_tuples!(impl_zip_iter);
