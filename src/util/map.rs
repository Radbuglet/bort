use std::{hash, marker::PhantomData};

pub type NopHashBuilder = ConstSafeBuildHasherDefault<NoOpHasher>;
pub type NopHashMap<K, V> = hashbrown::HashMap<K, V, NopHashBuilder>;
pub type NopHashSet<T> = hashbrown::HashSet<T, NopHashBuilder>;

pub type FxHashBuilder = ConstSafeBuildHasherDefault<rustc_hash::FxHasher>;
pub type FxHashMap<K, V> = hashbrown::HashMap<K, V, FxHashBuilder>;
pub type FxHashSet<T> = hashbrown::HashSet<T, FxHashBuilder>;

pub struct ConstSafeBuildHasherDefault<T>(PhantomData<fn(T) -> T>);

impl<T> ConstSafeBuildHasherDefault<T> {
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<T> Default for ConstSafeBuildHasherDefault<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: hash::Hasher + Default> hash::BuildHasher for ConstSafeBuildHasherDefault<T> {
    type Hasher = T;

    fn build_hasher(&self) -> Self::Hasher {
        T::default()
    }
}

#[derive(Default)]
pub struct NoOpHasher(u64);

impl hash::Hasher for NoOpHasher {
    fn write_u64(&mut self, i: u64) {
        debug_assert_eq!(self.0, 0);
        self.0 = i;
    }

    fn write(&mut self, _bytes: &[u8]) {
        unimplemented!("This is only supported for `u64`s.")
    }

    fn finish(&self) -> u64 {
        self.0
    }
}
