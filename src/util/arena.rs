use std::{
    cell::{Ref, RefCell, RefMut},
    marker::PhantomData,
    ops::{Deref, DerefMut},
    ptr,
};

use derive_where::derive_where;

use super::misc::leak;

// === Facade === //

pub trait ArenaKind: Sized {}

pub trait FreeableArenaKind: Sized + ArenaKind {}

pub trait CheckedArenaKind: Sized + FreeableArenaKind {}

pub trait StorableIn<A: ArenaKind> {
    type Spec: SpecArena<Kind = A, Value = Self>;
}

pub type Arena<T, A> = <T as StorableIn<A>>::Spec;

pub type ArenaPtr<T, A> = <Arena<T, A> as SpecArena>::Ptr;

pub type ArenaRef<'a, T, A> = <Arena<T, A> as SpecArena>::Ref<'a>;

pub type ArenaRefMapped<'a, T, U, A> = <ArenaRef<'a, T, A> as SpecArenaRef<'a>>::Mapped<U>;

pub type ArenaRefMut<'a, T, A> = <Arena<T, A> as SpecArena>::RefMut<'a>;

pub type ArenaRefMutMapped<'a, T, U, A> = <ArenaRefMut<'a, T, A> as SpecArenaRefMut<'a>>::Mapped<U>;

// === Spec === //

pub trait SpecArena: Default {
    type Kind: ArenaKind;
    type Value;
    type Ptr: SpecArenaPtr<Value = Self::Value>;
    type Ref<'a>: SpecArenaRef<'a, Value = Self::Value>
    where
        Self: 'a;

    type RefMut<'a>: SpecArenaRefMut<'a, Value = Self::Value>
    where
        Self: 'a;

    fn new() -> Self {
        Self::default()
    }

    fn alloc(&mut self, value: Self::Value) -> Self::Ptr;

    fn dealloc(&mut self, ptr: Self::Ptr) -> Self::Value
    where
        Self::Kind: FreeableArenaKind;

    fn is_alive(&self, ptr: &Self::Ptr) -> bool
    where
        Self::Kind: CheckedArenaKind;

    fn try_dealloc(&mut self, ptr: Self::Ptr) -> Option<Self::Value>
    where
        Self::Kind: CheckedArenaKind,
    {
        self.is_alive(&ptr).then(|| self.dealloc(ptr))
    }

    fn get<'a>(&'a self, ptr: &'a Self::Ptr) -> Self::Ref<'a>;

    fn try_get<'a>(&'a self, ptr: &'a Self::Ptr) -> Option<Self::Ref<'a>>
    where
        Self::Kind: CheckedArenaKind,
    {
        self.is_alive(ptr).then(|| self.get(ptr))
    }

    fn get_mut<'a>(&'a mut self, ptr: &'a Self::Ptr) -> Self::RefMut<'a>;

    fn try_get_mut<'a>(&'a mut self, ptr: &'a Self::Ptr) -> Option<Self::RefMut<'a>>
    where
        Self::Kind: CheckedArenaKind,
    {
        self.is_alive(ptr).then(|| self.get_mut(ptr))
    }
}

pub trait SpecArenaPtr: Clone + Eq {
    type Value: ?Sized;
}

pub trait SpecArenaRef<'a>: Sized + Deref<Target = Self::Value> {
    type Value: ?Sized + 'a;
    type Mapped<U: ?Sized + 'a>: SpecArenaRef<'a, Value = U>;

    fn filter_map<U: ?Sized>(
        orig: Self,
        f: impl FnOnce(&Self::Value) -> Option<&U>,
    ) -> Result<Self::Mapped<U>, Self>;

    fn map<U: ?Sized>(orig: Self, f: impl FnOnce(&Self::Value) -> &U) -> Self::Mapped<U> {
        match Self::filter_map(orig, |inp| Some(f(inp))) {
            Ok(v) => v,
            Err(_) => unreachable!(),
        }
    }

    fn map_split<U: ?Sized, V: ?Sized>(
        orig: Self,
        f: impl FnOnce(&Self::Value) -> (&U, &V),
    ) -> (Self::Mapped<U>, Self::Mapped<V>);

    fn clone(orig: &Self) -> Self;
}

pub trait SpecArenaRefMut<'a>: Sized + DerefMut<Target = Self::Value> {
    type Value: ?Sized + 'a;
    type Mapped<U: ?Sized + 'a>: SpecArenaRefMut<'a, Value = U>;

    fn filter_map<O: ?Sized>(
        orig: Self,
        f: impl FnOnce(&mut Self::Value) -> Option<&mut O>,
    ) -> Result<Self::Mapped<O>, Self>;

    fn map<O: ?Sized>(orig: Self, f: impl FnOnce(&mut Self::Value) -> &mut O) -> Self::Mapped<O> {
        match Self::filter_map(orig, |inp| Some(f(inp))) {
            Ok(v) => v,
            Err(_) => unreachable!(),
        }
    }

    fn map_split<U: ?Sized, V: ?Sized>(
        orig: Self,
        f: impl FnOnce(&mut Self::Value) -> (&mut U, &mut V),
    ) -> (Self::Mapped<U>, Self::Mapped<V>);
}

// === Reference types === //

impl<'a, T: ?Sized> SpecArenaRef<'a> for &'a T {
    type Value = T;
    type Mapped<U: ?Sized + 'a> = &'a U;

    fn filter_map<U: ?Sized>(
        orig: Self,
        f: impl FnOnce(&Self::Value) -> Option<&U>,
    ) -> Result<Self::Mapped<U>, Self> {
        if let Some(mapped) = f(orig) {
            Ok(mapped)
        } else {
            Err(orig)
        }
    }

    fn map_split<U: ?Sized, V: ?Sized>(
        orig: Self,
        f: impl FnOnce(&Self::Value) -> (&U, &V),
    ) -> (Self::Mapped<U>, Self::Mapped<V>) {
        f(orig)
    }

    fn clone(me: &Self) -> Self {
        me
    }
}

impl<'a, T: ?Sized> SpecArenaRefMut<'a> for &'a mut T {
    type Value = T;
    type Mapped<U: ?Sized + 'a> = &'a mut U;

    fn filter_map<O: ?Sized>(
        _orig: Self,
        _f: impl FnOnce(&mut Self::Value) -> Option<&mut O>,
    ) -> Result<Self::Mapped<O>, Self> {
        todo!();
    }

    fn map<O: ?Sized>(orig: Self, f: impl FnOnce(&mut Self::Value) -> &mut O) -> Self::Mapped<O> {
        f(orig)
    }

    fn map_split<U: ?Sized, V: ?Sized>(
        orig: Self,
        f: impl FnOnce(&mut Self::Value) -> (&mut U, &mut V),
    ) -> (Self::Mapped<U>, Self::Mapped<V>) {
        f(orig)
    }
}

impl<'a, T: ?Sized> SpecArenaRef<'a> for Ref<'a, T> {
    type Value = T;
    type Mapped<U: ?Sized + 'a> = Ref<'a, U>;

    fn filter_map<U: ?Sized>(
        orig: Self,
        f: impl FnOnce(&Self::Value) -> Option<&U>,
    ) -> Result<Self::Mapped<U>, Self> {
        Self::filter_map(orig, f)
    }

    fn map<U: ?Sized>(orig: Self, f: impl FnOnce(&Self::Value) -> &U) -> Self::Mapped<U> {
        Self::map(orig, f)
    }

    fn map_split<U: ?Sized, V: ?Sized>(
        orig: Self,
        f: impl FnOnce(&Self::Value) -> (&U, &V),
    ) -> (Self::Mapped<U>, Self::Mapped<V>) {
        Self::map_split(orig, f)
    }

    fn clone(orig: &Self) -> Self {
        Self::clone(orig)
    }
}

impl<'a, T: ?Sized> SpecArenaRefMut<'a> for RefMut<'a, T> {
    type Value = T;
    type Mapped<U: ?Sized + 'a> = RefMut<'a, U>;

    fn filter_map<O: ?Sized>(
        orig: Self,
        f: impl FnOnce(&mut Self::Value) -> Option<&mut O>,
    ) -> Result<Self::Mapped<O>, Self> {
        Self::filter_map(orig, f)
    }

    fn map<O: ?Sized>(orig: Self, f: impl FnOnce(&mut Self::Value) -> &mut O) -> Self::Mapped<O> {
        Self::map(orig, f)
    }

    fn map_split<U: ?Sized, V: ?Sized>(
        orig: Self,
        f: impl FnOnce(&mut Self::Value) -> (&mut U, &mut V),
    ) -> (Self::Mapped<U>, Self::Mapped<V>) {
        Self::map_split(orig, f)
    }
}

// === FreeListArena === //

pub struct FreeListArena {
    _never: (),
}

impl ArenaKind for FreeListArena {}

impl FreeableArenaKind for FreeListArena {}

impl<T> StorableIn<FreeListArena> for T {
    type Spec = SpecFreeListArena<T>;
}

#[derive(Debug, Clone)]
#[derive_where(Default)]
pub struct SpecFreeListArena<T> {
    free: Vec<SpecFreeListPtr<T>>,
    values: Vec<Option<T>>,
}

impl<T> SpecArena for SpecFreeListArena<T> {
    type Kind = FreeListArena;
    type Value = T;
    type Ptr = SpecFreeListPtr<T>;
    type Ref<'a> = &'a T
    where
        Self: 'a;

    type RefMut<'a> = &'a mut T
    where
        Self: 'a;

    fn alloc(&mut self, value: Self::Value) -> Self::Ptr {
        if let Some(free) = self.free.pop() {
            self.values[free.loc] = Some(value);
            free
        } else {
            let ptr = SpecFreeListPtr {
                _ty: PhantomData,
                loc: self.values.len(),
            };
            self.values.push(Some(value));
            ptr
        }
    }

    fn dealloc(&mut self, ptr: Self::Ptr) -> Self::Value
    where
        Self::Kind: FreeableArenaKind,
    {
        let taken = self.values[ptr.loc].take().unwrap();
        self.free.push(ptr);
        taken
    }

    fn is_alive(&self, _ptr: &Self::Ptr) -> bool
    where
        Self::Kind: CheckedArenaKind,
    {
        unimplemented!();
    }

    fn get<'a>(&'a self, ptr: &'a Self::Ptr) -> Self::Ref<'a> {
        self.values[ptr.loc].as_ref().unwrap()
    }

    fn get_mut<'a>(&'a mut self, ptr: &'a Self::Ptr) -> Self::RefMut<'a> {
        self.values[ptr.loc].as_mut().unwrap()
    }
}

#[derive_where(Debug, Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct SpecFreeListPtr<T> {
    _ty: PhantomData<fn() -> T>,
    loc: usize,
}

impl<T> SpecArenaPtr for SpecFreeListPtr<T> {
    type Value = T;
}

// === LeakyArena === //

pub struct LeakyArena;

impl ArenaKind for LeakyArena {}

impl<T: 'static> StorableIn<LeakyArena> for T {
    type Spec = SpecLeakyArena<T>;
}

#[derive_where(Debug, Clone, Default)]
pub struct SpecLeakyArena<T: 'static> {
    _ty: PhantomData<fn() -> T>,
}

impl<T> SpecArena for SpecLeakyArena<T> {
    type Kind = LeakyArena;
    type Value = T;
    type Ptr = SpecLeakyPtr<T>;

    type Ref<'a> = Ref<'a, T>
    where
        Self: 'a;

    type RefMut<'a> = RefMut<'a, T>
    where
        Self: 'a;

    fn alloc(&mut self, value: Self::Value) -> Self::Ptr {
        SpecLeakyPtr {
            value: leak(RefCell::new(value)),
        }
    }

    fn dealloc(&mut self, _ptr: Self::Ptr) -> Self::Value
    where
        Self::Kind: FreeableArenaKind,
    {
        unimplemented!()
    }

    fn is_alive(&self, _ptr: &Self::Ptr) -> bool
    where
        Self::Kind: CheckedArenaKind,
    {
        unimplemented!()
    }

    fn get<'a>(&'a self, ptr: &'a Self::Ptr) -> Self::Ref<'a> {
        ptr.value.borrow()
    }

    fn get_mut<'a>(&'a mut self, ptr: &'a Self::Ptr) -> Self::RefMut<'a> {
        ptr.value.borrow_mut()
    }
}

#[derive(Debug)]
#[derive_where(Copy, Clone)]
pub struct SpecLeakyPtr<T: 'static> {
    value: &'static RefCell<T>,
}

impl<T> Eq for SpecLeakyPtr<T> {}

impl<T> PartialEq for SpecLeakyPtr<T> {
    fn eq(&self, other: &Self) -> bool {
        ptr::eq(self.value, other.value)
    }
}

impl<T> SpecArenaPtr for SpecLeakyPtr<T> {
    type Value = T;
}

impl<T> SpecLeakyPtr<T> {
    pub fn direct_borrow(&self) -> Ref<'static, T> {
        self.value.borrow()
    }

    pub fn direct_borrow_mut(&self) -> RefMut<'static, T> {
        self.value.borrow_mut()
    }
}
