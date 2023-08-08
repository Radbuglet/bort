#![deny(unsafe_code)] // Unsafe code is only permitted in `core`.

pub mod behavior;
pub mod core;
mod database;
pub mod debug;
pub mod entity;
pub mod event;
pub mod obj;
pub mod query;
pub mod reborrow;
mod util;

pub mod prelude {
    pub use crate::{
        behavior::{
            delegate, derive_behavior_delegate, derive_event_handler, derive_multiplexed_handler,
            BehaviorRegistry, ComponentInjector, ContextlessEventHandler, ContextlessQueryHandler,
            HasBehavior, NamespacedQueryHandler,
        },
        entity::{storage, CompMut, CompRef, Entity, HeapMut, HeapRef, OwnedEntity, Storage},
        event::{EventTarget, ProcessableEventList, QueryableEventList, VecEventList},
        obj::{Obj, OwnedObj},
        query::{
            flush, query, GlobalTag, GlobalVirtualTag, HasGlobalManagedTag, HasGlobalVirtualTag,
            RawTag, Tag, VirtualTag,
        },
        reborrow::arb,
    };
}

pub use prelude::*;
