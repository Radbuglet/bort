use std::any::{type_name, TypeId};

use crate::{
    core::{
        cell::{OptRef, OptRefMut},
        heap::Slot,
        token::{self, MainThreadToken, ParallelTokenSource, TypeExclusiveToken, TypeReadToken},
    },
    entity::{Entity, StorageData, StorageDb, StorageInner, STORAGES},
    util::AnyDowncastExt,
};

// Re-export `is_main_thread`
pub use token::is_main_thread;

pub fn become_main_thread() {
    token::ensure_main_thread("Tried to become the main thread");
}

pub fn parallelize<F, R>(f: F) -> R
where
    F: Send + FnOnce(ParallelismSession<'_>) -> R,
    R: Send,
{
    MainThreadToken::acquire().parallelize(|cx| f(ParallelismSession::new(cx)))
}

// ParallelismSession
#[derive(Debug, Clone)]
pub struct ParallelismSession<'a> {
    cx: &'a ParallelTokenSource,
    db_token: TypeReadToken<'a, StorageDb>,
}

impl<'a> ParallelismSession<'a> {
    pub fn new(cx: &'a ParallelTokenSource) -> Self {
        Self {
            cx,
            db_token: cx.read_token(),
        }
    }

    pub fn token_source(&self) -> &'a ParallelTokenSource {
        self.cx
    }

    pub fn storage<T: 'static + Send + Sync>(&self) -> ReadStorageView<T> {
        let storage = STORAGES
            .get(&self.db_token)
            .storages
            .get(&TypeId::of::<T>())
            .map(|inner| inner.downcast_ref::<StorageData<T>>().unwrap());

        ReadStorageView {
            inner: storage.map(|storage| ReadStorageViewInner {
                storage,
                storage_token: self.cx.read_token(),
                value_token: self.cx.read_token(),
            }),
        }
    }

    pub fn storage_mut<T: 'static + Send + Sync>(&self) -> MutStorageView<T> {
        let storage = STORAGES
            .get(&self.db_token)
            .storages
            .get(&TypeId::of::<T>())
            .map(|inner| inner.downcast_ref::<StorageData<T>>().unwrap());

        MutStorageView {
            inner: storage.map(|storage| MutStorageViewInner {
                storage,
                storage_token: self.cx.read_token(),
                value_token: self.cx.exclusive_token(),
            }),
        }
    }
}

// MutStorageView
#[derive(Debug, Clone)]
pub struct MutStorageView<'a, T: 'static> {
    inner: Option<MutStorageViewInner<'a, T>>,
}

#[derive(Debug, Clone)]
struct MutStorageViewInner<'a, T: 'static> {
    storage: &'a StorageData<T>,
    storage_token: TypeReadToken<'a, StorageInner<T>>,
    value_token: TypeExclusiveToken<'a, T>,
}

impl<T: 'static + Send + Sync> MutStorageView<'_, T> {
    fn inner(&self) -> &MutStorageViewInner<'_, T> {
        self.inner.as_ref().unwrap()
    }

    pub fn try_get_slot(&self, entity: Entity) -> Option<Slot<T>> {
        let inner = self.inner.as_ref()?;

        inner
            .storage
            .get(&inner.storage_token)
            .mappings
            .get(&entity)
            .map(|mapping| mapping.slot)
    }

    pub fn get_slot(&self, entity: Entity) -> Slot<T> {
        self.try_get_slot(entity).unwrap_or_else(|| {
            panic!(
                "failed to find component of type {} for {:?}",
                type_name::<T>(),
                entity,
            )
        })
    }

    pub fn try_get(&self, entity: Entity) -> Option<OptRef<T>> {
        self.try_get_slot(entity)
            .map(|slot| slot.borrow(&self.inner().value_token))
    }

    pub fn try_get_mut(&self, entity: Entity) -> Option<OptRefMut<T>> {
        self.try_get_slot(entity)
            .map(|slot| slot.borrow_mut(&self.inner().value_token))
    }

    pub fn get(&self, entity: Entity) -> OptRef<T> {
        self.get_slot(entity).borrow(&self.inner().value_token)
    }

    pub fn get_mut(&self, entity: Entity) -> OptRefMut<T> {
        self.get_slot(entity).borrow_mut(&self.inner().value_token)
    }

    pub fn has(&self, entity: Entity) -> bool {
        self.try_get_slot(entity).is_some()
    }
}

// ReadStorageView
#[derive(Debug, Clone)]
pub struct ReadStorageView<'a, T: 'static> {
    inner: Option<ReadStorageViewInner<'a, T>>,
}

#[derive(Debug, Clone)]
struct ReadStorageViewInner<'a, T: 'static> {
    storage: &'a StorageData<T>,
    storage_token: TypeReadToken<'a, StorageInner<T>>,
    value_token: TypeReadToken<'a, T>,
}

impl<T: 'static + Send + Sync> ReadStorageView<'_, T> {
    fn inner(&self) -> &ReadStorageViewInner<'_, T> {
        self.inner.as_ref().unwrap()
    }

    pub fn try_get_slot(&self, entity: Entity) -> Option<Slot<T>> {
        let inner = self.inner.as_ref()?;

        inner
            .storage
            .get(&inner.storage_token)
            .mappings
            .get(&entity)
            .map(|mapping| mapping.slot)
    }

    pub fn get_slot(&self, entity: Entity) -> Slot<T> {
        self.try_get_slot(entity).unwrap_or_else(|| {
            panic!(
                "failed to find component of type {} for {:?}",
                type_name::<T>(),
                entity,
            )
        })
    }

    pub fn try_get(&self, entity: Entity) -> Option<&T> {
        Some(self.try_get_slot(entity)?.get(&self.inner().value_token))
    }

    pub fn get(&self, entity: Entity) -> &T {
        self.get_slot(entity).get(&self.inner().value_token)
    }

    pub fn has(&self, entity: Entity) -> bool {
        self.try_get_slot(entity).is_some()
    }
}
