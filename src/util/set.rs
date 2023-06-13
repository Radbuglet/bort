use std::{
    hash, iter,
    ops::{Deref, DerefMut},
    slice,
};

use derive_where::derive_where;
use hashbrown::raw::RawTable;

use super::map::{FxHashBuilder, FxHashMap};

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

// === LocalHeap === //

// Traits
pub type LocalHeapPtr<H, V> = <<H as LocalHeap>::For<V> as SpecLocalHeap<V>>::Ptr;

pub trait LocalHeap {
    type For<V>: SpecLocalHeap<V>;
}

pub trait SpecLocalHeap<V> {
    type Ptr: Clone;
    type Ref<'a>: Deref<Target = V>
    where
        Self: 'a;

    type Mut<'a>: DerefMut<Target = V>
    where
        Self: 'a;

    fn alloc(&mut self, value: V) -> Self::Ptr;

    fn get<'a>(&'a self, key: &'a Self::Ptr) -> Self::Ref<'a>;

    fn get_mut<'a>(&'a mut self, key: &'a Self::Ptr) -> Self::Mut<'a>;

    fn cmp_ptr(&self, a: &Self::Ptr, b: &Self::Ptr) -> bool;
}

pub trait SpecLocalHeapWithDealloc<V>: SpecLocalHeap<V> {
    fn dealloc(&mut self, key: &Self::Ptr) -> V;
}

// Leaky
// pub struct LeakyObjHeap;
//
// impl LocalHeap for LeakyObjHeap {
//     type For<V> = LeakyObjHeap;
// }
//
// impl<V: 'static> SpecLocalHeap<V> for LeakyObjHeap {
//     type Ptr = &'static RefCell<V>;
//     type Ref<'a> = Ref<'a, V>
//     where
//         Self: 'a;
//
//     type Mut<'a>  = RefMut<'a, V>
//     where
//         Self: 'a;
//
//     fn alloc(&mut self, value: V) -> Self::Ptr {
//         leak(RefCell::new(value))
//     }
//
//     fn get<'a>(&'a self, key: &'a Self::Ptr) -> Self::Ref<'a> {
//         key.borrow()
//     }
//
//     fn get_mut<'a>(&'a mut self, key: &'a Self::Ptr) -> Self::Mut<'a> {
//         key.borrow_mut()
//     }
//
//     fn cmp_ptr(&self, a: &Self::Ptr, b: &Self::Ptr) -> bool {
//         (*a) as *const RefCell<V> == (*b) as *const RefCell<V>
//     }
// }

// FreeList
pub struct FreeListHeap;

impl LocalHeap for FreeListHeap {
    type For<V> = SpecFreeListHeap<V>;
}

#[derive_where(Default)]
pub struct SpecFreeListHeap<V> {
    values: Vec<Option<V>>,
    free: Vec<usize>,
}

impl<V> SpecLocalHeap<V> for SpecFreeListHeap<V> {
    type Ptr = usize;

    type Ref<'a> = &'a V
    where
        Self: 'a;

    type Mut<'a> = &'a mut V
    where
        Self: 'a;

    fn alloc(&mut self, value: V) -> Self::Ptr {
        if let Some(free) = self.free.pop() {
            self.values[free] = Some(value);
            free
        } else {
            let index = self.values.len();
            self.values.push(Some(value));
            index
        }
    }

    fn get<'a>(&'a self, key: &'a Self::Ptr) -> Self::Ref<'a> {
        self.values[*key].as_ref().unwrap()
    }

    fn get_mut<'a>(&'a mut self, key: &'a Self::Ptr) -> Self::Mut<'a> {
        self.values[*key].as_mut().unwrap()
    }

    fn cmp_ptr(&self, a: &Self::Ptr, b: &Self::Ptr) -> bool {
        a == b
    }
}

// === SetMap === //

pub type SetMapRef<K, V, H> = LocalHeapPtr<H, SetMapEntry<K, V, H>>;

pub struct SetMap<K, V, H: LocalHeap> {
    root: SetMapRef<K, V, H>,
    map: RawTable<(u64, SetMapRef<K, V, H>)>,
    hasher: FxHashBuilder,
    heap: H::For<SetMapEntry<K, V, H>>,
}

pub struct SetMapEntry<K, V, H: LocalHeap> {
    keys: Box<[K]>,
    extensions: FxHashMap<K, SetMapRef<K, V, H>>,
    de_extensions: FxHashMap<K, SetMapRef<K, V, H>>,
    value: V,
}

impl<K, V, H> Default for SetMap<K, V, H>
where
    K: 'static + Ord + hash::Hash + Copy,
    V: Default,
    H: LocalHeap,
    H::For<SetMapEntry<K, V, H>>: Default,
{
    fn default() -> Self {
        Self::new(Default::default())
    }
}

impl<K, V, H> SetMap<K, V, H>
where
    K: 'static + Ord + hash::Hash + Copy,
    V: Default,
    H: LocalHeap,
{
    pub fn new(mut heap: H::For<SetMapEntry<K, V, H>>) -> Self {
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

    pub fn root(&self) -> &SetMapRef<K, V, H> {
        &self.root
    }

    fn lookup_extension_common(
        &mut self,
        base_ptr: Option<&SetMapRef<K, V, H>>,
        key: K,
        positive_getter_ref: impl Fn(&SetMapEntry<K, V, H>) -> &FxHashMap<K, SetMapRef<K, V, H>>,
        positive_getter_mut: impl Fn(&mut SetMapEntry<K, V, H>) -> &mut FxHashMap<K, SetMapRef<K, V, H>>,
        negative_getter_mut: impl Fn(&mut SetMapEntry<K, V, H>) -> &mut FxHashMap<K, SetMapRef<K, V, H>>,
        iter_ctor: impl for<'a> GoofyIterCtorHack<'a, K>,
    ) -> SetMapRef<K, V, H> {
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
        base: Option<&SetMapRef<K, V, H>>,
        key: K,
    ) -> SetMapRef<K, V, H> {
        fn iter_ctor<K: Copy>(
            a: &[K],
            b: K,
        ) -> IterMerger<iter::Copied<slice::Iter<'_, K>>, iter::Once<K>> {
            merge_iters(a.iter().copied(), iter::once(b))
        }

        self.lookup_extension_common(
            base,
            key,
            |a: &SetMapEntry<K, V, H>| &a.extensions,
            |a: &mut SetMapEntry<K, V, H>| &mut a.extensions,
            |a: &mut SetMapEntry<K, V, H>| &mut a.de_extensions,
            iter_ctor,
        )
    }

    pub fn lookup_de_extension(&mut self, base: &SetMapRef<K, V, H>, key: K) -> SetMapRef<K, V, H> {
        fn iter_ctor<K: Copy>(a: &[K], b: K) -> IterFilter<iter::Copied<slice::Iter<'_, K>>> {
            IterFilter(a.iter().copied(), b)
        }

        self.lookup_extension_common(
            Some(base),
            key,
            |a: &SetMapEntry<K, V, H>| &a.de_extensions,
            |a: &mut SetMapEntry<K, V, H>| &mut a.extensions,
            |a: &mut SetMapEntry<K, V, H>| &mut a.extensions,
            iter_ctor,
        )
    }

    pub fn remove(&mut self, removed_ptr: &SetMapRef<K, V, H>)
    where
        H::For<SetMapEntry<K, V, H>>: SpecLocalHeapWithDealloc<SetMapEntry<K, V, H>>,
    {
        let removed_data = self.heap.dealloc(removed_ptr);

        for (key, referencer) in &removed_data.de_extensions {
            if self.heap.cmp_ptr(removed_ptr, referencer) {
                continue;
            }

            self.heap.get_mut(&referencer).extensions.remove(key);
        }

        for (key, referencer) in &removed_data.extensions {
            if self.heap.cmp_ptr(removed_ptr, referencer) {
                continue;
            }

            self.heap.get_mut(&referencer).de_extensions.remove(key);
        }

        let removed_hash = hash_iter(&mut self.hasher, removed_data.keys.iter());
        self.map.remove_entry(removed_hash, |(_, candidate_ptr)| {
            self.heap.cmp_ptr(candidate_ptr, removed_ptr)
        });
    }
}

pub trait GoofyIterCtorHack<'a, K: 'static> {
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
