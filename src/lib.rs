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
        entity::{storage, CompMut, CompRef, Entity, OwnedEntity, Storage},
        obj::{Obj, OwnedObj},
        query::{flush, query_all, query_all_anon, RawTag, Tag},
    };
}

pub use prelude::*;
