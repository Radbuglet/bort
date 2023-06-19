use std::{hash, iter, slice};

use derive_where::derive_where;
use hashbrown::raw::RawTable;

use super::{
    arena::{Arena, ArenaKind, ArenaPtr, FreeableArenaKind, SpecArena, StorableIn},
    hash_map::{FxHashBuilder, FxHashMap},
};

// === Helpers === //

pub fn hash_iter<H, E, I>(hasher: &mut H, iter: I) -> u64
where
    H: hash::BuildHasher,
    E: hash::Hash,
    I: IntoIterator<Item = E>,
{
    let mut state = hasher.build_hasher();
    hash_iter_write(&mut state, iter);
    hash::Hasher::finish(&state)
}

pub fn hash_iter_write<H, E, I>(state: &mut H, iter: I)
where
    H: hash::Hasher,
    E: hash::Hash,
    I: IntoIterator<Item = E>,
{
    for item in iter {
        item.hash(state);
    }
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

// === SetMap === //

pub type SetMapPtr<K, V, A> = ArenaPtr<SetMapEntry<K, V, A>, A>;

pub struct SetMap<K, V, A: ArenaKind>
where
    SetMapEntry<K, V, A>: StorableIn<A>,
{
    root: SetMapPtr<K, V, A>,
    map: RawTable<(u64, SetMapPtr<K, V, A>)>,
    hasher: FxHashBuilder,
    heap: Arena<SetMapEntry<K, V, A>, A>,
}

pub struct SetMapEntry<K, V, A: ArenaKind>
where
    SetMapEntry<K, V, A>: StorableIn<A>,
{
    keys: Box<[K]>,
    extensions: FxHashMap<K, SetMapPtr<K, V, A>>,
    de_extensions: FxHashMap<K, SetMapPtr<K, V, A>>,
    value: V,
}

impl<K, V, A: ArenaKind> Default for SetMap<K, V, A>
where
    SetMapEntry<K, V, A>: StorableIn<A>,
    K: 'static + Ord + hash::Hash + Copy,
    V: Default,
{
    fn default() -> Self {
        Self::new(Default::default())
    }
}

impl<K, V, A: ArenaKind> SetMap<K, V, A>
where
    SetMapEntry<K, V, A>: StorableIn<A>,
    K: 'static + Ord + hash::Hash + Copy,
    V: Default,
{
    pub fn new(mut heap: Arena<SetMapEntry<K, V, A>, A>) -> Self {
        let mut hasher = FxHashBuilder::new();
        let root = heap.alloc(SetMapEntry {
            keys: Box::from_iter([]),
            extensions: FxHashMap::default(),
            de_extensions: FxHashMap::default(),
            value: V::default(),
        });

        let mut map = RawTable::default();
        let root_hash = hash_iter(&mut hasher, None::<K>);
        map.insert(root_hash, (root_hash, root.clone()), |(hash, _)| *hash);

        Self {
            root,
            map,
            hasher,
            heap,
        }
    }

    pub fn root(&self) -> &SetMapPtr<K, V, A> {
        &self.root
    }

    fn lookup_extension_common(
        &mut self,
        base_ptr: Option<&SetMapPtr<K, V, A>>,
        key: K,
        positive_getter_ref: impl Fn(&SetMapEntry<K, V, A>) -> &FxHashMap<K, SetMapPtr<K, V, A>>,
        positive_getter_mut: impl Fn(&mut SetMapEntry<K, V, A>) -> &mut FxHashMap<K, SetMapPtr<K, V, A>>,
        negative_getter_mut: impl Fn(&mut SetMapEntry<K, V, A>) -> &mut FxHashMap<K, SetMapPtr<K, V, A>>,
        iter_ctor: impl for<'a> GoofyIterCtorHack<'a, K>,
    ) -> SetMapPtr<K, V, A> {
        // Attempt to get the extension from the base element's extension edges.
        let base_ptr = base_ptr.unwrap_or(&self.root);
        let base_data = self.heap.get(base_ptr);

        if let Some(extension) = (positive_getter_ref)(&base_data).get(&key) {
            return extension.clone();
        }

        // Otherwise, fetch the target set from the primary map and cache the edge.
        let keys = iter_ctor.make_iter(&base_data.keys, key);
        let target_hash = hash_iter(&mut self.hasher, keys.clone());

        if let Some((_, target_ptr)) =
            self.map
                .get(target_hash, |(candidate_hash, candidate_ptr)| {
                    if target_hash != *candidate_hash {
                        return false;
                    }

                    eq_iter(
                        keys.clone(),
                        self.heap.get(candidate_ptr).keys.iter(),
                        |a, b| a == *b,
                    )
                })
        {
            drop(keys);
            drop(base_data);

            (positive_getter_mut)(&mut self.heap.get_mut(base_ptr)).insert(key, target_ptr.clone());
            (negative_getter_mut)(&mut self.heap.get_mut(target_ptr)).insert(key, base_ptr.clone());

            return target_ptr.clone();
        }

        // If all that failed, create the new set.
        let keys = Box::from_iter(keys);
        drop(base_data);

        let target_ptr = {
            let mut target_entry = SetMapEntry {
                keys,
                extensions: FxHashMap::default(),
                de_extensions: FxHashMap::default(),
                value: V::default(),
            };
            (negative_getter_mut)(&mut target_entry).insert(key, base_ptr.clone());

            self.heap.alloc(target_entry)
        };

        (positive_getter_mut)(&mut self.heap.get_mut(base_ptr)).insert(key, target_ptr.clone());

        self.map.insert(
            target_hash,
            (target_hash, target_ptr.clone()),
            |(hash, _)| *hash,
        );

        target_ptr
    }

    pub fn lookup_extension(
        &mut self,
        base: Option<&SetMapPtr<K, V, A>>,
        key: K,
    ) -> SetMapPtr<K, V, A> {
        fn iter_ctor<K: Copy>(
            a: &[K],
            b: K,
        ) -> IterMerger<iter::Copied<slice::Iter<'_, K>>, iter::Once<K>> {
            merge_iters(a.iter().copied(), iter::once(b))
        }

        self.lookup_extension_common(
            base,
            key,
            |a: &SetMapEntry<K, V, A>| &a.extensions,
            |a: &mut SetMapEntry<K, V, A>| &mut a.extensions,
            |a: &mut SetMapEntry<K, V, A>| &mut a.de_extensions,
            iter_ctor,
        )
    }

    pub fn lookup_de_extension(&mut self, base: &SetMapPtr<K, V, A>, key: K) -> SetMapPtr<K, V, A> {
        fn iter_ctor<K: Copy>(a: &[K], b: K) -> IterFilter<iter::Copied<slice::Iter<'_, K>>> {
            IterFilter(a.iter().copied(), b)
        }

        self.lookup_extension_common(
            Some(base),
            key,
            |a: &SetMapEntry<K, V, A>| &a.de_extensions,
            |a: &mut SetMapEntry<K, V, A>| &mut a.extensions,
            |a: &mut SetMapEntry<K, V, A>| &mut a.extensions,
            iter_ctor,
        )
    }

    pub fn remove(&mut self, removed_ptr: SetMapPtr<K, V, A>)
    where
        A: FreeableArenaKind,
    {
        let removed_ptr_2 = removed_ptr.clone();
        let removed_data = self.heap.dealloc(removed_ptr);

        for (key, referencer) in &removed_data.de_extensions {
            if &removed_ptr_2 == referencer {
                continue;
            }

            self.heap.get_mut(&referencer).extensions.remove(key);
        }

        for (key, referencer) in &removed_data.extensions {
            if &removed_ptr_2 == referencer {
                continue;
            }

            self.heap.get_mut(&referencer).de_extensions.remove(key);
        }

        let removed_hash = hash_iter(&mut self.hasher, removed_data.keys.iter());
        self.map.remove_entry(removed_hash, |(_, candidate_ptr)| {
            &removed_ptr_2 == candidate_ptr
        });
    }

    pub fn arena(&self) -> &Arena<SetMapEntry<K, V, A>, A> {
        &self.heap
    }

    pub fn arena_mut(&mut self) -> &mut Arena<SetMapEntry<K, V, A>, A> {
        &mut self.heap
    }
}

trait GoofyIterCtorHack<'a, K: 'static> {
    type Iter: IntoIterator<Item = K> + Clone;

    fn make_iter(&self, base: &'a [K], add: K) -> Self::Iter;
}

impl<'a, K: 'static, F, I> GoofyIterCtorHack<'a, K> for F
where
    F: Fn(&'a [K], K) -> I,
    I: IntoIterator<Item = K> + Clone,
{
    type Iter = I;

    fn make_iter(&self, base: &'a [K], add: K) -> Self::Iter {
        (self)(base, add)
    }
}

impl<K, V, A: ArenaKind> SetMapEntry<K, V, A>
where
    Self: StorableIn<A>,
{
    pub fn keys(&self) -> &[K] {
        &self.keys
    }

    pub fn value(&self) -> &V {
        &self.value
    }

    pub fn value_mut(&mut self) -> &mut V {
        &mut self.value
    }

    pub fn pair_mut(&mut self) -> (&[K], &mut V) {
        (&self.keys, &mut self.value)
    }
}