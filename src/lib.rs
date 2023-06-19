#![deny(unsafe_code)] // Unsafe code is only permitted in `core`.

pub mod core;
pub mod database;
pub mod debug;
pub mod entity;
pub mod obj;
mod util;

pub mod prelude {
    pub use crate::{
        core::cell::{OptRef, OptRefMut},
        entity::{storage, CompMut, CompRef, Entity, OwnedEntity, Storage},
        obj::{Obj, OwnedObj},
    };
}

pub use prelude::*;
