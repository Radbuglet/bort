use std::{
    cell::{Ref, RefCell, RefMut},
    marker::PhantomData,
    num::NonZeroU32,
    ops::{Deref, DerefMut},
};

use derive_where::derive_where;

use super::misc::leak;

// === Core === //

// Aliases
pub type ArenaFor<A, T> = <A as ArenaSupporting<T>>::Arena;
pub type AbaPtrFor<A, T> = <ArenaFor<A, T> as Arena>::AbaPtr;
pub type CheckedPtrFor<A, T> = <ArenaFor<A, T> as CheckedArena>::CheckedPtr;
pub type RefFor<'a, A, T> = <ArenaFor<A, T> as Arena>::Ref<'a>;
pub type RefMutFor<'a, A, T> = <ArenaFor<A, T> as Arena>::RefMut<'a>;

// Kind
pub trait ArenaKind: Sized {}

pub trait ArenaSupporting<T>: ArenaKind {
    type Arena: Arena<Value = T>;
}

// Arena
pub trait Arena: Default {
    type Value;
    type AbaPtr: AbaPtr;

    #[rustfmt::skip]
    type Ref<'a>: ArenaRef<'a, Value = Self::Value> where Self: 'a;

    #[rustfmt::skip]
    type RefMut<'a>: ArenaRefMut<'a, Value = Self::Value> where Self: 'a;

    fn new() -> Self {
        Default::default()
    }

    fn alloc_aba(&mut self, value: Self::Value) -> Self::AbaPtr;

    fn get_aba(&self, ptr: &Self::AbaPtr) -> Self::Ref<'_>;

    fn get_aba_mut(&mut self, ptr: &Self::AbaPtr) -> Self::RefMut<'_>;
}

pub trait FreeingArena: Arena {
    fn dealloc_aba(&mut self, ptr: &Self::AbaPtr) -> Self::Value;
}

pub trait CheckedArena: FreeingArena {
    type CheckedPtr: CheckedPtr<Aba = Self::AbaPtr>;

    fn is_alive(&self, ptr: &Self::CheckedPtr) -> bool;

    fn alloc(&mut self, value: Self::Value) -> Self::CheckedPtr;

    fn get(&self, ptr: &Self::CheckedPtr) -> Self::Ref<'_> {
        assert!(self.is_alive(ptr));
        self.get_aba(&ptr.as_aba())
    }

    fn get_mut(&mut self, ptr: &Self::CheckedPtr) -> Self::RefMut<'_> {
        assert!(self.is_alive(ptr));
        self.get_aba_mut(&ptr.as_aba())
    }

    fn dealloc(&mut self, ptr: &Self::CheckedPtr) -> Option<Self::Value> {
        self.is_alive(ptr).then(|| self.dealloc_aba(&ptr.as_aba()))
    }
}

// Objects
pub trait AbaPtr: Clone + Eq {}

pub trait CheckedPtr: Clone + Eq {
    type Aba: AbaPtr;

    fn as_aba(&self) -> Self::Aba;

    fn to_aba(self) -> Self::Aba {
        self.as_aba()
    }
}

pub trait ArenaRef<'a>: Sized + Deref<Target = Self::Value> {
    type Value: ?Sized + 'a;
    type Mapped<U: ?Sized + 'a>: ArenaRef<'a, Value = U>;

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

pub trait ArenaRefMut<'a>: Sized + DerefMut<Target = Self::Value> {
    type Value: ?Sized + 'a;
    type Mapped<U: ?Sized + 'a>: ArenaRefMut<'a, Value = U>;

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

// Standard impls
impl<'a, T: ?Sized> ArenaRef<'a> for &'a T {
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

impl<'a, T: ?Sized> ArenaRefMut<'a> for &'a mut T {
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

impl<'a, T: ?Sized> ArenaRef<'a> for Ref<'a, T> {
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

impl<'a, T: ?Sized> ArenaRefMut<'a> for RefMut<'a, T> {
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

// Kind
#[non_exhaustive]
pub struct FreeListArenaKind;

impl ArenaKind for FreeListArenaKind {}

impl<T> ArenaSupporting<T> for FreeListArenaKind {
    type Arena = FreeListArena<T>;
}

// Arena
#[derive(Debug, Clone)]
#[derive_where(Default)]
pub struct FreeListArena<T> {
    free: Vec<FreeListAbaPtr<T>>,
    values: Vec<(NonZeroU32, Option<T>)>,
}

impl<T> Arena for FreeListArena<T> {
    type Value = T;
    type AbaPtr = FreeListAbaPtr<T>;

	#[rustfmt::skip]
    type Ref<'a> = &'a T where Self: 'a;

	#[rustfmt::skip]
    type RefMut<'a> = &'a mut T where Self: 'a;

    fn alloc_aba(&mut self, value: Self::Value) -> Self::AbaPtr {
        self.alloc(value).as_aba()
    }

    fn get_aba(&self, ptr: &Self::AbaPtr) -> Self::Ref<'_> {
        self.values[ptr.index as usize]
            .1
            .as_ref()
            .expect("slot is empty")
    }

    fn get_aba_mut(&mut self, ptr: &Self::AbaPtr) -> Self::RefMut<'_> {
        self.values[ptr.index as usize]
            .1
            .as_mut()
            .expect("slot is empty")
    }
}

impl<T> FreeingArena for FreeListArena<T> {
    fn dealloc_aba(&mut self, ptr: &Self::AbaPtr) -> Self::Value {
        let taken = self.values[ptr.index as usize]
            .1
            .take()
            .expect("slot is empty");

        self.free.push(*ptr);
        taken
    }
}

impl<T> CheckedArena for FreeListArena<T> {
    type CheckedPtr = FreeListCheckedPtr<T>;

    fn alloc(&mut self, value: Self::Value) -> Self::CheckedPtr {
        loop {
            if let Some(free) = self.free.pop() {
                let (slot_gen, slot_value) = &mut self.values[free.index as usize];
                let Some(new_gen) = slot_gen.checked_add(1) else {
                    // Forget about this slot—it's been used up.
                    continue;
                };
                *slot_gen = new_gen;
                *slot_value = Some(value);

                return FreeListCheckedPtr {
                    _ty: PhantomData,
                    index: free.index,
                    gen: new_gen,
                };
            } else {
                let slot = self.values.len() as u32;
                let gen = NonZeroU32::new(1).unwrap();
                self.values.push((gen, Some(value)));

                return FreeListCheckedPtr {
                    _ty: PhantomData,
                    index: slot,
                    gen,
                };
            }
        }
    }

    fn is_alive(&self, ptr: &Self::CheckedPtr) -> bool {
        self.values[ptr.index as usize].0 == ptr.gen
    }
}

// Pointers
#[derive_where(Debug, Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct FreeListAbaPtr<T> {
    _ty: PhantomData<fn() -> T>,
    index: u32,
}

impl<T> AbaPtr for FreeListAbaPtr<T> {}

#[derive_where(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct FreeListCheckedPtr<T> {
    _ty: PhantomData<fn() -> T>,
    index: u32,
    gen: NonZeroU32,
}

impl<T> CheckedPtr for FreeListCheckedPtr<T> {
    type Aba = FreeListAbaPtr<T>;

    fn as_aba(&self) -> Self::Aba {
        FreeListAbaPtr {
            _ty: PhantomData,
            index: self.index,
        }
    }
}

// === LeakyArena === //

// Kind
#[non_exhaustive]
pub struct LeakyArenaKind;

impl ArenaKind for LeakyArenaKind {}

impl<T: 'static> ArenaSupporting<T> for LeakyArenaKind {
    type Arena = LeakyArena<T>;
}

// Arena
#[derive_where(Default)]
pub struct LeakyArena<T: 'static> {
    _ty: PhantomData<fn() -> T>,
}

impl<T: 'static> Arena for LeakyArena<T> {
    type Value = T;
    type AbaPtr = LeakyPtr<T>;

	#[rustfmt::skip]
    type Ref<'a> = Ref<'a, T> where Self: 'a;

	#[rustfmt::skip]
    type RefMut<'a> = RefMut<'a, T> where Self: 'a;

    fn alloc_aba(&mut self, value: Self::Value) -> Self::AbaPtr {
        LeakyPtr {
            _ty: PhantomData,
            val: leak(RefCell::new(value)),
        }
    }

    fn get_aba(&self, ptr: &Self::AbaPtr) -> Self::Ref<'_> {
        ptr.val.borrow()
    }

    fn get_aba_mut(&mut self, ptr: &Self::AbaPtr) -> Self::RefMut<'_> {
        ptr.val.borrow_mut()
    }
}

// Pointers
#[derive(Debug)]
#[derive_where(Copy, Clone)]
pub struct LeakyPtr<T: 'static> {
    _ty: PhantomData<fn() -> T>,
    val: &'static RefCell<T>,
}

impl<T: 'static> Eq for LeakyPtr<T> {}

impl<T: 'static> PartialEq for LeakyPtr<T> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.val, other.val)
    }
}

impl<T: 'static> AbaPtr for LeakyPtr<T> {}

impl<T: 'static> LeakyPtr<T> {
    pub fn direct_borrow(&self) -> Ref<'static, T> {
        self.val.borrow()
    }

    pub fn direct_borrow_mut(&self) -> RefMut<'static, T> {
        self.val.borrow_mut()
    }
}
