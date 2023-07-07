#![deny(unsafe_code)] // Unsafe code is only permitted in `core`.

pub mod core;
mod database;
pub mod debug;
pub mod entity;
pub mod obj;
pub mod query;
mod util;

pub mod prelude {
    pub use crate::{
        core::cell::{OptRef, OptRefMut},
        entity::{event_set, storage, CompMut, CompRef, Entity, EventSet, OwnedEntity, Storage},
        obj::{Obj, OwnedObj},
        query::{flush, query, RawTag, Tag, VirtualTag},
    };
}

pub use prelude::*;
