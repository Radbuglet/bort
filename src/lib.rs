#![deny(unsafe_code)] // Unsafe code is only permitted in `core`.
#![allow(clippy::missing_safety_doc)] // TODO: Remove this

pub mod behavior;
pub mod core;
mod database;
pub mod debug;
pub mod entity;
pub mod event;
pub mod obj;
pub mod query;
mod util;

pub use autoken;

pub mod prelude {
    pub use crate::{
        autoken,
        behavior::{
            behavior, delegate, Behavior, BehaviorListFor, BehaviorRegistry, ComponentInjector,
            InitializerBehaviorList, OrderedBehaviorList, PartialEntity, SimpleBehaviorList,
        },
        entity::{storage, CompMut, CompRef, Entity, OwnedEntity, Storage},
        event::{
            CountingEvent, EventGroup, EventGroupMarkerWith, EventGroupMarkerWithSeparated,
            EventTarget, ProcessableEventList, QueryableEventList, VecEventList,
        },
        obj::{Obj, OwnedObj},
        query::{
            flush, query, GlobalTag, GlobalVirtualTag, HasGlobalManagedTag, HasGlobalVirtualTag,
            RawTag, Tag, VirtualTag,
        },
    };
}

pub use prelude::*;
