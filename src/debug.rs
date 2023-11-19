use std::{borrow::Cow, fmt, sync::atomic};

use crate::{
    core::{
        heap::{DEBUG_HEAP_COUNTER, DEBUG_SLOT_COUNTER},
        token::MainThreadToken,
    },
    database::{DbRoot, InertEntity},
    entity::Entity,
};

pub fn alive_entity_count() -> usize {
    DbRoot::get(MainThreadToken::acquire_fmt("fetch entity diagnostics"))
        .debug_alive_list()
        .len()
}

pub fn alive_entities() -> Vec<Entity> {
    DbRoot::get(MainThreadToken::acquire_fmt("fetch entity diagnostics"))
        .debug_alive_list()
        .map(InertEntity::into_dangerous_entity)
        .collect()
}

pub fn spawned_entity_count() -> u64 {
    DbRoot::get(MainThreadToken::acquire_fmt("fetch entity diagnostics")).debug_total_spawns()
}

pub fn heap_count() -> u64 {
    DEBUG_HEAP_COUNTER.load(atomic::Ordering::Relaxed)
}

pub fn slot_count() -> u64 {
    DEBUG_SLOT_COUNTER.load(atomic::Ordering::Relaxed)
}

pub fn archetype_count() -> u64 {
    DbRoot::get(MainThreadToken::acquire_fmt("fetch entity diagnostics")).debug_archetype_count()
}

pub fn dump_database_state() -> String {
    format!(
        "{:#?}",
        DbRoot::get(MainThreadToken::acquire_fmt("dump the database state"))
    )
}

#[derive(Debug, Clone)]
pub struct DebugLabel(pub Cow<'static, str>);

impl<L: AsDebugLabel> From<L> for DebugLabel {
    fn from(value: L) -> Self {
        Self(AsDebugLabel::reify(value))
    }
}

pub trait AsDebugLabel {
    fn reify(me: Self) -> Cow<'static, str>;
}

impl AsDebugLabel for &'static str {
    fn reify(me: Self) -> Cow<'static, str> {
        Cow::Borrowed(me)
    }
}

impl AsDebugLabel for String {
    fn reify(me: Self) -> Cow<'static, str> {
        Cow::Owned(me)
    }
}

impl AsDebugLabel for fmt::Arguments<'_> {
    fn reify(me: Self) -> Cow<'static, str> {
        if let Some(str) = me.as_str() {
            Cow::Borrowed(str)
        } else {
            Cow::Owned(me.to_string())
        }
    }
}

impl AsDebugLabel for Cow<'static, str> {
    fn reify(me: Self) -> Cow<'static, str> {
        me
    }
}
