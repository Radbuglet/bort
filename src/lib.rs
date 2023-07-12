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

pub mod prelude {
    pub use crate::{
        core::cell::{OptRef, OptRefMut},
        entity::{storage, CompMut, CompRef, Entity, OwnedEntity, Storage},
        event::{EventTarget, ProcessableEventList, QueryableEventList, VecEventList},
        obj::{Obj, OwnedObj},
        query::{flush, query, ManagedGlobalTag, RawTag, Tag, VirtualGlobalTag, VirtualTag},
    };
}

pub use prelude::*;
