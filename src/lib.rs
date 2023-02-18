//! # Bort
//!
//! A Simple Object Model for Rust
//!
//! ```
//! use cgmath::Vec3;
//! use bort::Entity;
//!
//! #[derive(Debug, Copy, Clone)]
//! struct Pos(Vec3);
//!
//! #[derive(Debug, Copy, Clone)]
//! struct Vel(Vec3);
//!
//! #[derive(Debug)]
//! struct ChaserAi {
//!     target: Option<Entity>,
//!     home: Vec3,
//! }
//!
//! #[derive(Debug)]
//! struct PlayerState {
//!     hp: u32,
//! }
//!
//! impl PlayerState {
//!     fn update(&mut self, me: Entity) {
//!         me.get_mut::<Pos>().0 += me.get::<Vel>().0;
//!     }
//! }
//!
//! let player = Entity::new()
//!     .with(Pos(Vec3::ZERO))
//!     .with(Vel(Vec3::ZERO))
//!     .with(PlayerState {
//!         hp: 100,
//!     });
//!
//! let chaser = Entity::new()
//!     .with(Pos(Vec3::ZERO))
//!     .with(ChaserAi { target: Some(player), home: Vec3::ZERO });
//!
//! player.get_mut::<PlayerState>().update(player.entity());
//!
//! let pos = &mut *chaser.get_mut::<Pos>();
//! let state = chaser.get::<ChaserState>();
//!
//! pos.0 = pos.0.lerp(match state.target {
//!     Some(target) => target.get::<Pos>().0,
//!     None => state.home,
//! }, 0.2);
//! ```
//!
//! ## Getting Started
//!
//! Applications in Bort are made of [`Entity`] instances. These are `Copy`able references to
//! logical objects in your application. They can represent anything from a player character in a
//! game to a UI widget.
//!
//! To create one, just call [`Entity::new()`].
//!
//! ```
//! use bort::Entity;
//!
//! let player = Entity::new();
//! ```
//!
//! From there, you can add components to it using either [`insert`](Entity::insert) or [`with`](Entity::with):
//!
//! ```
//! player.insert(Pos(Vec3::ZERO));
//! player.insert(Vel(Vec3::ZERO));
//!
//! // ...which is equivalent to:
//! let player = Entity::new()
//!     .with(Pos(Vec3::ZERO))
//!     .with(Vel(Vec3::ZERO));
//! ```
//!
//! ...and access them using [`get`](Entity::get) or [`get_mut`](Entity::get_mut):
//!
//! ```
//! let mut pos = player.get_mut::<Pos>();  // These are `RefMut`s
//! let vel = player.get::<Vel>();          // ...and `Ref`s.
//!
//! pos.0 += vel.0;
//! ```
//!
//! Note that `Entity::new()` doesn't actually return an `Entity`, but rather an [`OwnedEntity`].
//! These expose the exact same interface as an `Entity` but have an additional `Drop` handler
//! (making them non-`Copy`) that automatically [`Entity::destroy`](Entity::destroy)s their managed
//! entity when they leave the scope.
//!
//! You can extract an `Entity` from them using [`OwnedEntity::entity(&self)`](OwnedEntity::entity).
//!
//! ```
//! let player2 = player;  // (transfers ownership of `OwnedEntity`)
//! // player.insert("hello!");
//! // ^ `player` is invalid here; use `player2` instead!
//!
//! let player_ref = player.entity();  // (produces a copyable `Entity` reference)
//! let player_ref_2 = player_ref;  // (copies the value)
//!
//! assert_eq(player_ref, player_ref_2);
//! assert(player_ref.is_alive());
//!
//! drop(player2);  // (dropped `OwnedEntity`; `player_xx` are all dead now)
//!
//! assert!(!player_ref.is_alive());
//! ```
//!
//! Using these `Entity` handles, you can freely reference you object in multiple places without
//! dealing with cloning smart pointers.
//!
//! ```
//! use bort::Entity;
//!
//! struct PlayerId(usize);
//!
//! struct Name(String);
//!
//! #[derive(Default)]
//! struct PlayerRegistry {
//!     all: Vec<Entity>,
//!     by_name: HashMap<String, Entity>,
//! }
//!
//! impl PlayerRegistry {
//!     pub fn add(&mut self, player: Entity) {
//!         player.insert(PlayerId(self.all.len()));
//!         self.all.push(player);
//!         self.by_name.insert(
//!             player.get::<Name>().0.clone(),
//!             player,
//!         );
//!     }
//! }
//!
//! let player = Entity::new()
//!     .with(Name("foo".to_string()));
//!
//! let mut registry = PlayerRegistry::default();
//! registry.add(player);
//!
//! println!("Player is at index {}", player.get::<PlayerId>().0);
//! ```
//!
//! See the reference documentation for [`Entity`] and [`OwnedEntity`] for a complete list of methods.
//!
//! Features not discussed in this introductory section include:
//!
//! - Liveness checking through [`Entity::is_alive()`]
//! - Storage handle caching through [`storage()`] for hot code sections
//! - Additional entity builder methods such as [`Entity::with_debug_label()`] and [`Entity::with_self_referential`]
//! - Debug utilities to [query entity statistics](debug)
//!
//! ### A Note on Borrowing
//!
//! Bort relies quite heavily on runtime borrowing. Although the type system does not prevent you
//! from mutably borrowing the same component more than once, doing so will cause a panic anyways:
//!
//! ```
//! use bort::Entity;
//!
//! let foo = Entity::new()
//!     .with(vec![3i32]);
//!
//! let vec1 = foo.get::<Vec<i32>>();  // Ok
//! let a = &vec1[0];
//!
//! let mut vec2 = foo.get_mut::<Vec<i32>>();  // Panics at runtime!
//! vec2.push(4);
//! ```
//!
//! Components should strive to only borrow from their logical children. For example, it's somewhat
//! expected for a player to access the components of the items in its inventory but it's somewhat
//! unexpected to have that same player access the world state without that state being explicitly
//! passed to it. Maintaining this informal rule should make borrowing dependencies easier to reason
//! about.
//!
//! Dispatching object behavior through "system" functions that iterate over and update entities of
//! a given type can be a wonderful way to encourage intuitive borrowing practices and can greatly
//! improve performance when compared to regular dynamic dispatch:
//!
//! ```
//! use bort::{Entity, storage};
//!
//! fn process_players(players: impl Iterator<Item = Entity>) {
//!     let positions = storage::<Pos>();
//!     let velocities = storage::<Vel>();
//!
//!     for player in players {
//!         positions.get_mut(player).0 += velocities.get(player).0;
//!     }
//! }
//! ```
//!
//! ### A Note on Threading
//!
//! Currently, Bort is entirely single-threaded. Because everything is stored in thread-local storage,
//! entities spawned on one thread will likely appear dead on another and all component sets will be
//! thread-specific.

// TODO: Stop leaking thread globals in case people want to use Bort on a non-main thread.

use std::{
    any::{type_name, Any, TypeId},
    borrow::{Borrow, Cow},
    cell::{Cell, Ref, RefCell, RefMut},
    fmt, hash, iter, mem,
    num::NonZeroU64,
    sync::atomic::{AtomicU64, Ordering},
};

use debug::{AsDebugLabel, DebugLabel};

// === Helpers === //

fn xorshift64(state: NonZeroU64) -> NonZeroU64 {
    // Adapted from: https://en.wikipedia.org/w/index.php?title=Xorshift&oldid=1123949358
    let state = state.get();
    let state = state ^ (state << 13);
    let state = state ^ (state >> 7);
    let state = state ^ (state << 17);
    NonZeroU64::new(state).unwrap()
}

type NopHashBuilder = hash::BuildHasherDefault<NoOpHasher>;
type NopHashMap<K, V> = hashbrown::HashMap<K, V, NopHashBuilder>;

type FxHashBuilder = hash::BuildHasherDefault<rustc_hash::FxHasher>;
type FxHashMap<K, V> = hashbrown::HashMap<K, V, FxHashBuilder>;
type FxHashSet<T> = hashbrown::HashSet<T, FxHashBuilder>;

fn hash_iter<H, E, I>(state: &mut H, iter: I)
where
    H: hash::Hasher,
    E: hash::Hash,
    I: IntoIterator<Item = E>,
{
    for item in iter {
        item.hash(state);
    }
}

fn merge_iters<I, A, B>(a: A, b: B) -> impl Iterator<Item = I>
where
    I: Ord,
    A: IntoIterator<Item = I>,
    B: IntoIterator<Item = I>,
{
    let mut a_iter = a.into_iter().peekable();
    let mut b_iter = b.into_iter().peekable();

    iter::from_fn(move || {
        // Unfortunately, `Option`'s default Ord impl isn't suitable for this.
        match (a_iter.peek(), b_iter.peek()) {
            (Some(a), Some(b)) => {
                if a < b {
                    a_iter.next()
                } else {
                    b_iter.next()
                }
            }
            (Some(_), None) => a_iter.next(),
            (None, Some(_)) => b_iter.next(),
            (None, None) => None,
        }
    })
}

fn leak<T>(value: T) -> &'static T {
    Box::leak(Box::new(value))
}

#[derive(Debug, Default)]
struct NoOpHasher(u64);

impl hash::Hasher for NoOpHasher {
    fn write_u64(&mut self, i: u64) {
        self.0 = i;
    }

    fn write(&mut self, _bytes: &[u8]) {
        unimplemented!("This is only supported for `u64`s.")
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

// === ComponentList === //

#[derive(Copy, Clone)]
struct ComponentType {
    id: TypeId,
    name: &'static str,
    dtor: fn(Entity),
}

impl ComponentType {
    fn of<T: 'static>() -> Self {
        fn dtor<T: 'static>(entity: Entity) {
            drop(storage::<T>().remove_untracked(entity)); // (ignores missing components)
        }

        Self {
            id: TypeId::of::<T>(),
            name: type_name::<T>(),
            dtor: dtor::<T>,
        }
    }
}

impl Ord for ComponentType {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.id.cmp(&other.id)
    }
}

impl PartialOrd for ComponentType {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl hash::Hash for ComponentType {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl Eq for ComponentType {}

impl PartialEq for ComponentType {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

struct ComponentList {
    comps: Box<[ComponentType]>,
    extensions: RefCell<FxHashMap<TypeId, &'static Self>>,
    de_extensions: RefCell<FxHashMap<TypeId, &'static Self>>,
}

impl hash::Hash for ComponentList {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        hash_iter(state, self.comps.iter());
    }
}

impl Eq for ComponentList {}

impl PartialEq for ComponentList {
    fn eq(&self, other: &Self) -> bool {
        self.comps == other.comps
    }
}

impl ComponentList {
    pub fn empty() -> &'static Self {
        thread_local! {
            static EMPTY: &'static ComponentList = leak(ComponentList {
                comps: Box::new([]),
                extensions: Default::default(),
                de_extensions: Default::default(),
            });
        }

        EMPTY.with(|v| *v)
    }

    pub fn run_dtors(&self, target: Entity) {
        for comp in &*self.comps {
            (comp.dtor)(target);
        }
    }

    pub fn extend(&'static self, with: ComponentType) -> &'static Self {
        if self.comps.contains(&with) {
            return self;
        }

        self.extensions
            .borrow_mut()
            .entry(with.id)
            .or_insert_with(|| Self::find_extension_in_db(&self.comps, with))
    }

    pub fn de_extend(&'static self, without: ComponentType) -> &'static Self {
        if !self.comps.contains(&without) {
            return self;
        }

        self.de_extensions
            .borrow_mut()
            .entry(without.id)
            .or_insert_with(|| Self::find_de_extension_in_db(&self.comps, without))
    }

    // === Database === //

    thread_local! {
        static COMP_LISTS: RefCell<FxHashSet<&'static ComponentList>> = RefCell::new(FxHashSet::from_iter([
            ComponentList::empty(),
        ]));
    }

    fn find_extension_in_db(base_set: &[ComponentType], with: ComponentType) -> &'static Self {
        struct ComponentListSearch<'a>(&'a [ComponentType], ComponentType);

        impl hash::Hash for ComponentListSearch<'_> {
            fn hash<H: hash::Hasher>(&self, state: &mut H) {
                hash_iter(state, merge_iters(self.0, &[self.1]));
            }
        }

        impl hashbrown::Equivalent<&'static ComponentList> for ComponentListSearch<'_> {
            fn equivalent(&self, key: &&'static ComponentList) -> bool {
                // See if the key component list without the additional component
                // is equal to the base list.
                key.comps.iter().filter(|v| **v == self.1).eq(self.0.iter())
            }
        }

        ComponentList::COMP_LISTS.with(|set| {
            *set.borrow_mut()
                .get_or_insert_with(&ComponentListSearch(base_set, with), |_| {
                    leak(Self {
                        comps: merge_iters(base_set.iter().copied(), [with])
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                        extensions: Default::default(),
                        de_extensions: Default::default(),
                    })
                })
        })
    }

    fn find_de_extension_in_db(
        base_set: &[ComponentType],
        without: ComponentType,
    ) -> &'static Self {
        struct ComponentListSearch<'a>(&'a [ComponentType], ComponentType);

        impl hash::Hash for ComponentListSearch<'_> {
            fn hash<H: hash::Hasher>(&self, state: &mut H) {
                hash_iter(state, self.0.iter().filter(|v| **v != self.1));
            }
        }

        impl hashbrown::Equivalent<&'static ComponentList> for ComponentListSearch<'_> {
            fn equivalent(&self, key: &&'static ComponentList) -> bool {
                // See if the base component list without the removed component
                // is equal to the key list.
                self.0.iter().filter(|v| **v == self.1).eq(key.comps.iter())
            }
        }

        ComponentList::COMP_LISTS.with(|set| {
            *set.borrow_mut()
                .get_or_insert_with(&ComponentListSearch(base_set, without), |_| {
                    leak(Self {
                        comps: base_set
                            .iter()
                            .copied()
                            .filter(|v| *v != without)
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                        extensions: Default::default(),
                        de_extensions: Default::default(),
                    })
                })
        })
    }
}

// === Storage === //

/// The type of an immutable reference to a component. These are essentially [`Ref`]s from the
/// standard library. See [`CompMut`] for its mutable counterpart.
///
/// The `'static` lifetime means that the memory location holding the component data will be valid
/// throughout the lifetime of the program—not that the component will be leaked. Indeed, these slots
/// will be reclaimed once the owning entity dies.
///
/// Because outstanding `CompRefs` and `CompMuts` prevent that component from being removed from a
/// dying entity, *it is highly discouraged to store these in permanent structures unless you know
/// all referents will expire before the owning entity will*.
pub type CompRef<T> = Ref<'static, T>;

/// The type of an mutable reference to a component. These are essentially [`RefMut`]s from the
/// standard library. See [`CompRef`] for its immutable counterpart.
///
/// The `'static` lifetime means that the memory location holding the component data will be valid
/// throughout the lifetime of the program—not that the component will be leaked. Indeed, these slots
/// will be reclaimed once the owning entity dies.
///
/// Because outstanding `CompRefs` and `CompMuts` prevent that component from being removed from a
/// dying entity, *it is highly discouraged to store these in permanent structures unless you know
/// all referents will expire before the owning entity will*.
pub type CompMut<T> = RefMut<'static, T>;

thread_local! {
    static STORAGES: RefCell<FxHashMap<TypeId, &'static dyn Any>> = Default::default();
}

/// Fetches a [`Storage`] instance for the given component type `T`, which can be used to optimize
/// tight loops fetching many entity components.
///
/// Components are normally fetched from these using the [`Entity::get()`] and [`Entity::get_mut()`]
/// shorthand methods. However, fetching a storage manually and using it for multiple component
/// fetches in a function can be useful for optimizing tight loops which fetch a predictable set of
/// components, such as when you write a "system" function processing entities of a given logical
/// type.
///
/// ```
/// use bort::{Entity, storage};
///
/// fn process_players(players: impl Iterator<Item = Entity>) {
///     let positions = storage::<Pos>();
///     let velocities = storage::<Vel>();
///     for player in players {
///         positions.get_mut(player).0 += velocities.get(player).0;
///     }
/// }
/// ```
///
/// This is a short alias to [`Storage::<T>::acquire()`].
///
pub fn storage<T: 'static>() -> &'static Storage<T> {
    STORAGES.with(|db| {
        db.borrow_mut()
            .entry(TypeId::of::<T>())
            .or_insert_with(|| {
                leak(Storage::<T>(RefCell::new(StorageInner {
                    free_slots: Vec::new(),
                    mappings: NopHashMap::default(),
                })))
            })
            .downcast_ref::<Storage<T>>()
            .unwrap()
    })
}

const fn block_elem_size<T>() -> usize {
    // TODO: make this adaptive
    128
}

type StorageSlot<T> = RefCell<Option<T>>;

/// Storages are glorified maps from entity IDs to components of type `T` with some extra bookkeeping
/// that provide the backbone for [`Entity::get()`] and [`Entity::get_mut()`] queries. Fetching
/// components from a storage explicitly can optimize component accesses in a tight loop.
///
/// There is exactly one storage for every given component type, and it can be acquired using either
/// [`Storage::<T>::acquire()`] or its shorter [`storage::<T>()`] function alias.
///
/// Components are normally fetched from these using the [`Entity::get()`] and [`Entity::get_mut()`]
/// shorthand methods. However, fetching a storage manually and using it for multiple component
/// fetches in a function can be useful for optimizing tight loops which fetch a predictable set of
/// components, such as when you write a "system" function processing entities of a given logical
/// type.
///
/// ```
/// use bort::{Entity, storage};
///
/// fn process_players(players: impl Iterator<Item = Entity>) {
///     let positions = storage::<Pos>();
///     let velocities = storage::<Vel>();
///     for player in players {
///         positions.get_mut(player).0 += velocities.get(player).0;
///     }
/// }
/// ```
///
/// `Storages` are not `Sync` because they employ un-synchronized interior mutability in the form of
/// [`RefCell`]'ed components.
#[derive(Debug)]
pub struct Storage<T: 'static>(RefCell<StorageInner<T>>);

#[derive(Debug)]
struct StorageInner<T: 'static> {
    free_slots: Vec<&'static StorageSlot<T>>,
    mappings: NopHashMap<Entity, &'static StorageSlot<T>>,
}

impl<T: 'static> Storage<T> {
    /// Acquires the `Storage<T>` singleton for the current thread.
    ///
    /// This is a longer form of the shorter [`storage<T>()`] function alias.
    ///
    /// ```
    /// use bort::{storage, Storage};
    ///
    /// let foo = Storage::<u32>::acquire();
    /// let bar = storage::<u32>();  // This is an equivalent shorter form.
    /// ```
    ///
    pub fn acquire() -> &'static Storage<T> {
        storage()
    }

    pub fn insert(&self, entity: Entity, value: T) -> Option<T> {
        ALIVE.with(|slots| {
            let mut slots = slots.borrow_mut();
            let slot = slots.get_mut(&entity).unwrap_or_else(|| {
                panic!(
                    "attempted to attach a component of type {} to the dead or cross-thread {:?}.",
                    type_name::<T>(),
                    entity
                )
            });

            *slot = slot.extend(ComponentType::of::<T>());
        });

        self.insert_untracked(entity, value)
    }

    fn insert_untracked(&self, entity: Entity, value: T) -> Option<T> {
        let me = &mut *self.0.borrow_mut();

        let slot = match me.mappings.entry(entity) {
            hashbrown::hash_map::Entry::Occupied(entry) => entry.get(),
            hashbrown::hash_map::Entry::Vacant(entry) => {
                if me.free_slots.is_empty() {
                    let block = iter::repeat_with(StorageSlot::default)
                        .take(block_elem_size::<T>())
                        .collect::<Vec<_>>()
                        .leak();

                    me.free_slots.extend(block.iter());
                }

                let slot = me.free_slots.pop().unwrap();
                entry.insert(slot);
                slot
            }
        };

        slot.borrow_mut().replace(value)
    }

    pub fn remove(&self, entity: Entity) -> Option<T> {
        if let Some(removed) = self.remove_untracked(entity) {
            // Modify the component list or fail silently if the entity lacks the component.
            // This behavior allows users to `remove` components explicitly from entities that are
            // in the of being destroyed. This is the opposite behavior of `insert`, which requires
            // the entity to be valid before modifying it. This pairing ensures that, by the time
            // `Entity::destroy()` resolves, all of the entity's components will have been removed.
            ALIVE.with(|slots| {
                let mut slots = slots.borrow_mut();
                let Some(slot) = slots.get_mut(&entity) else { return };

                *slot = slot.de_extend(ComponentType::of::<T>());
            });

            Some(removed)
        } else {
            // Only if the component is missing will we issue the standard error.
            assert!(
                entity.is_alive(),
                "attempted to remove a component of type {} from the already fully-dead or cross-thread {:?}",
                type_name::<T>(),
                entity,
            );
            None
        }
    }

    fn remove_untracked(&self, entity: Entity) -> Option<T> {
        let mut me = self.0.borrow_mut();

        if let Some(slot) = me.mappings.remove(&entity) {
            let taken = slot.borrow_mut().take();
            me.free_slots.push(slot);
            taken
        } else {
            None
        }
    }

    #[inline(always)]
    fn try_get_slot(&self, entity: Entity) -> Option<&'static StorageSlot<T>> {
        self.0.borrow().mappings.get(&entity).copied()
    }

    #[inline(always)]
    fn get_slot(&self, entity: Entity) -> &'static StorageSlot<T> {
        #[cold]
        #[inline(never)]
        fn get_slot_failed<T: 'static>(entity: Entity) -> ! {
            panic!(
                "failed to find component of type {} for {:?}",
                type_name::<T>(),
                entity,
            );
        }

        self.try_get_slot(entity)
            .unwrap_or_else(|| get_slot_failed::<T>(entity))
    }

    #[inline(always)]
    pub fn try_get(&self, entity: Entity) -> Option<CompRef<T>> {
        self.try_get_slot(entity)
            .map(|slot| Ref::map(slot.borrow(), |v| v.as_ref().unwrap()))
    }

    #[inline(always)]
    pub fn try_get_mut(&self, entity: Entity) -> Option<CompMut<T>> {
        self.try_get_slot(entity)
            .map(|slot| RefMut::map(slot.borrow_mut(), |v| v.as_mut().unwrap()))
    }

    #[inline(always)]
    pub fn get(&self, entity: Entity) -> CompRef<T> {
        Ref::map(self.get_slot(entity).borrow(), |v| v.as_ref().unwrap())
    }

    #[inline(always)]
    pub fn get_mut(&self, entity: Entity) -> CompMut<T> {
        RefMut::map(self.get_slot(entity).borrow_mut(), |v| v.as_mut().unwrap())
    }

    #[inline(always)]
    pub fn has(&self, entity: Entity) -> bool {
        self.try_get_slot(entity).is_some()
    }
}

// === Entity === //

thread_local! {
    static ALIVE: RefCell<NopHashMap<Entity, &'static ComponentList>> = Default::default();
}

static DEBUG_ENTITY_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Copy, Clone, Hash, Eq, PartialEq)]
pub struct Entity(NonZeroU64);

impl Entity {
    pub fn new() -> OwnedEntity {
        OwnedEntity::new()
    }

    pub fn new_unmanaged() -> Self {
        // Increment the total entity counter
        DEBUG_ENTITY_COUNTER.fetch_add(1, Ordering::Relaxed);

        // Allocate a slot
        thread_local! {
            static ID_GEN: Cell<NonZeroU64> = const { Cell::new(match NonZeroU64::new(1) {
                Some(v) => v,
                None => unreachable!(),
            }) };
        }

        let me = Self(ID_GEN.with(|v| {
            let state = xorshift64(v.get());
            v.set(state);
            state
        }));

        // Register our slot in the alive set
        ALIVE.with(|slots| slots.borrow_mut().insert(me, ComponentList::empty()));

        me
    }

    pub fn with<T: 'static>(self, comp: T) -> Self {
        self.insert(comp);
        self
    }

    pub fn with_self_referential<T: 'static>(self, func: impl FnOnce(Entity) -> T) -> Self {
        self.insert(func(self));
        self
    }

    pub fn with_debug_label<L: AsDebugLabel>(self, label: L) -> Self {
        #[cfg(debug_assertions)]
        self.with(DebugLabel::from(label));
        #[cfg(not(debug_assertions))]
        let _ = label;
        self
    }

    pub fn insert<T: 'static>(self, comp: T) -> Option<T> {
        storage::<T>().insert(self, comp)
    }

    pub fn remove<T: 'static>(self) -> Option<T> {
        storage::<T>().remove(self)
    }

    pub fn try_get<T: 'static>(self) -> Option<CompRef<T>> {
        storage::<T>().try_get(self)
    }

    pub fn try_get_mut<T: 'static>(self) -> Option<CompMut<T>> {
        storage::<T>().try_get_mut(self)
    }

    pub fn get<T: 'static>(self) -> CompRef<T> {
        storage::<T>().get(self)
    }

    pub fn get_mut<T: 'static>(self) -> CompMut<T> {
        storage::<T>().get_mut(self)
    }

    pub fn has<T: 'static>(self) -> bool {
        storage::<T>().has(self)
    }

    pub fn is_alive(self) -> bool {
        ALIVE.with(|slots| slots.borrow().contains_key(&self))
    }

    pub fn destroy(self) {
        ALIVE.with(|slots| {
            let comp_list = slots.borrow_mut().remove(&self).unwrap_or_else(|| {
                panic!(
                    "attempted to destroy the already-dead or cross-threaded {:?}.",
                    self
                )
            });

            comp_list.run_dtors(self);
        });
    }
}

impl fmt::Debug for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct StrLit<'a>(&'a str);

        impl fmt::Debug for StrLit<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.0)
            }
        }

        ALIVE.with(|alive| {
            #[derive(Debug)]
            struct Id(NonZeroU64);

            if let Some(comp_list) = alive.borrow().get(self).copied() {
                let mut builder = f.debug_tuple("Entity");

                if let Some(label) = self.try_get::<DebugLabel>() {
                    builder.field(&label);
                }

                builder.field(&Id(self.0));

                for v in comp_list.comps.iter() {
                    if v.id != TypeId::of::<DebugLabel>() {
                        builder.field(&StrLit(v.name));
                    }
                }

                builder.finish()
            } else {
                f.debug_tuple("Entity")
                    .field(&"<DEAD OR CROSS-THREAD>")
                    .field(&Id(self.0))
                    .finish()
            }
        })
    }
}

// === OwnedEntity === //

#[derive(Debug, Hash, Eq, PartialEq)]
pub struct OwnedEntity(Entity);

impl OwnedEntity {
    // === Lifecycle === //

    pub fn new() -> Self {
        Self(Entity::new_unmanaged())
    }

    pub fn entity(&self) -> Entity {
        self.0
    }

    pub fn unmanage(self) -> Entity {
        let entity = self.0;
        mem::forget(self);

        entity
    }

    pub fn split_guard(self) -> (Self, Entity) {
        let entity = self.entity();
        (self, entity)
    }

    // === Forwards === //

    pub fn with<T: 'static>(self, comp: T) -> Self {
        self.0.insert(comp);
        self
    }

    pub fn with_self_referential<T: 'static>(self, func: impl FnOnce(Entity) -> T) -> Self {
        self.0.insert(func(self.entity()));
        self
    }

    pub fn with_debug_label<L: AsDebugLabel>(self, label: L) -> Self {
        self.0.with_debug_label(label);
        self
    }

    pub fn insert<T: 'static>(&self, comp: T) -> Option<T> {
        self.0.insert(comp)
    }

    pub fn remove<T: 'static>(&self) -> Option<T> {
        self.0.remove()
    }

    pub fn try_get<T: 'static>(&self) -> Option<CompRef<T>> {
        self.0.try_get()
    }

    pub fn try_get_mut<T: 'static>(&self) -> Option<CompMut<T>> {
        self.0.try_get_mut()
    }

    pub fn get<T: 'static>(&self) -> CompRef<T> {
        self.0.get()
    }

    pub fn get_mut<T: 'static>(&self) -> CompMut<T> {
        self.0.get_mut()
    }

    pub fn has<T: 'static>(&self) -> bool {
        self.0.has::<T>()
    }

    pub fn is_alive(&self) -> bool {
        self.0.is_alive()
    }
}

impl Borrow<Entity> for OwnedEntity {
    fn borrow(&self) -> &Entity {
        &self.0
    }
}

impl Drop for OwnedEntity {
    fn drop(&mut self) {
        self.0.destroy();
    }
}

// === Debug utilities === //

pub mod debug {
    use super::*;

    pub fn alive_entity_count() -> usize {
        ALIVE.with(|slots| slots.borrow().len())
    }

    pub fn alive_entities() -> Vec<Entity> {
        ALIVE.with(|slots| slots.borrow().keys().copied().collect())
    }

    pub fn spawned_entity_count() -> u64 {
        DEBUG_ENTITY_COUNTER.load(Ordering::Relaxed)
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
}
