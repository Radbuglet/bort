#![deny(unsafe_code)] // Unsafe code is only permitted in `core`.

pub mod behavior;
pub mod core;
mod database;
pub mod debug;
pub mod entity;
pub mod event;
pub mod obj;
pub mod query;
mod util;

cfgenius::define! {
    pub HAS_SADDLE_SUPPORT = cfg(feature = "saddle")
}

cfgenius::cond! {
    if macro(HAS_SADDLE_SUPPORT) {
        pub mod saddle;
    }
}

pub mod prelude {
    pub use crate::{
        behavior::{
            behavior_delegate, behavior_kind, delegate, BehaviorKind, BehaviorRegistry,
            ComponentInjector, ContextlessEventHandler, ContextlessQueryHandler,
            NamespacedQueryHandler,
        },
        entity::{storage, CompMut, CompRef, Entity, OwnedEntity, Storage},
        event::{EventTarget, ProcessableEventList, QueryableEventList, VecEventList},
        obj::{Obj, OwnedObj},
        query::{
            flush, query, GlobalTag, GlobalVirtualTag, HasGlobalManagedTag, HasGlobalVirtualTag,
            RawTag, Tag, VirtualTag,
        },
    };

    cfgenius::cond! {
        if macro(super::HAS_SADDLE_SUPPORT) {
            pub use crate::saddle::{
                ProcCollection, proc_collection,
                call_cx, CanCallCollection,
                AccessRef, AccessMut, access_cx,
                proc,
                validate, RootCollectionCallToken,
            };
        }
    }
}

pub use prelude::*;
