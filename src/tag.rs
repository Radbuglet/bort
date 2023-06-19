use std::{marker::PhantomData, num::NonZeroU64};

use derive_where::derive_where;

use crate::{entity::Entity, util::hash_map::NopHashSet, CompMut, Obj, OwnedEntity};

// === Tag === //

#[derive_where(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Tag<T: 'static> {
    _ty: PhantomData<fn() -> T>,
    raw: RawTag,
}

impl<T> Into<RawTag> for Tag<T> {
    fn into(self) -> RawTag {
        self.raw
    }
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct RawTag {
    id: NonZeroU64,
}

// === Tag API === //

// TODO: Use a real implementation for this.
#[derive(Default)]
struct Tags {
    tags: NopHashSet<RawTag>,
}

impl Entity {
    fn tag_state(self) -> CompMut<Tags> {
        self.try_get_mut::<Tags>()
            .unwrap_or_else(|| Obj::insert(self, Tags::default()).get_mut())
    }

    pub fn tag(self, tag: impl Into<RawTag>) {
        self.tag_state().tags.insert(tag.into());
    }

    pub fn untag(self, tag: impl Into<RawTag>) {
        self.tag_state().tags.remove(&tag.into());
    }

    pub fn has_tag(self, tag: impl Into<RawTag>) -> bool {
        self.tag_state().tags.contains(&tag.into())
    }
}

impl OwnedEntity {
    pub fn tag(self, tag: impl Into<RawTag>) {
        self.entity().tag(tag)
    }

    pub fn untag(self, tag: impl Into<RawTag>) {
        self.entity().untag(tag)
    }

    pub fn has_tag(self, tag: impl Into<RawTag>) -> bool {
        self.entity().has_tag(tag)
    }
}

// === Dispatch === //

#[derive(Debug, Clone)]
#[derive_where(Default)]
pub struct Event<E> {
    targets: Vec<(Entity, E)>,
}

impl<E> Event<E> {
    pub const fn new() -> Self {
        Self {
            targets: Vec::new(),
        }
    }

    pub fn fire(&mut self, entity: Entity, event: E) {
        self.targets.push((entity, event));
    }

    pub fn merge_in(&mut self, event: Event<E>) {
        self.targets.extend(event.targets);
    }

    pub fn iter_tagged(
        &self,
        tag: impl Into<RawTag>,
    ) -> impl Iterator<Item = (Entity, &'_ E)> + '_ {
        let tag = tag.into();
        self.targets
            .iter()
            .map(|(ent, val)| (*ent, val))
            .filter(move |(ent, _)| ent.has_tag(tag))
    }
}

#[derive(Debug, Clone)]
#[derive_where(Default)]
pub struct AnonEvent<E> {
    targets: Vec<E>,
}

impl<E> AnonEvent<E> {
    pub const fn new() -> Self {
        Self {
            targets: Vec::new(),
        }
    }

    pub fn fire(&mut self, event: E) {
        self.targets.push(event);
    }

    pub fn merge_in(&mut self, event: AnonEvent<E>) {
        self.targets.extend(event.targets);
    }

    pub fn iter(&self) -> impl Iterator<Item = &'_ E> + '_ {
        self.targets.iter()
    }
}
