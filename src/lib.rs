//! Bort implements a simple object model for Rust that aims to be convenient, intuitive, and fast.
//!
//! ```
//! use bort::{Entity, OwnedEntity};
//! use glam::Vec3;
//!
//! // Define a bunch of components as plain old Rust structs.
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
//!         let mut pos = me.get_mut::<Pos>();
//!         pos.0 += me.get::<Vel>().0;
//!
//!         if pos.0.y < 0.0 {
//!             // Take void damage.
//!             self.hp -= 1;
//!         }
//!     }
//! }
//!
//! // Spawn entities to contain those components.
//! let player = OwnedEntity::new()
//!     .with(Pos(Vec3::ZERO))
//!     .with(Vel(Vec3::ZERO))
//!     .with(PlayerState {
//!         hp: 100,
//!     });
//!
//! let chaser = OwnedEntity::new()
//!     .with(Pos(Vec3::ZERO))
//!     .with(ChaserAi { target: Some(player.entity()), home: Vec3::ZERO });
//!
//! // Fetch the `PlayerState` component from the entity and update its state.
//! player.get_mut::<PlayerState>().update(player.entity());
//!
//! // Process the "chaser" monster's AI.
//! let pos = &mut *chaser.get_mut::<Pos>();
//! let state = chaser.get::<ChaserAi>();
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
//! To create one, just call [`OwnedEntity::new()`].
//!
//! ```
//! use bort::OwnedEntity;
//!
//! let player = OwnedEntity::new();
//! ```
//!
//! From there, you can add components to it using either [`insert`](Entity::insert) or [`with`](Entity::with):
//!
//! ```
//! # use bort::OwnedEntity;
//! # use glam::Vec3;
//! # struct Pos(Vec3);
//! # struct Vel(Vec3);
//! # let player = OwnedEntity::new();
//! #
//! player.insert(Pos(Vec3::ZERO));
//! player.insert(Vel(Vec3::ZERO));
//!
//! // ...which is equivalent to:
//! let player = OwnedEntity::new()
//!     .with(Pos(Vec3::ZERO))
//!     .with(Vel(Vec3::ZERO));
//! ```
//!
//! ...and access them using [`get`](Entity::get) or [`get_mut`](Entity::get_mut):
//!
//! ```
//! # use bort::OwnedEntity;
//! # use glam::Vec3;
//! # struct Pos(Vec3);
//! # struct Vel(Vec3);
//! # let player = OwnedEntity::new().with(Pos(Vec3::ZERO)).with(Vel(Vec3::ZERO));
//! #
//! let mut pos = player.get_mut::<Pos>();  // These are `RefMut`s
//! let vel = player.get::<Vel>();          // ...and `Ref`s.
//!
//! pos.0 += vel.0;
//! ```
//!
//! You might be wonder about the difference between an [`OwnedEntity`] and an [`Entity`]. While an
//! `Entity` is just a wrapper around a [`NonZeroU64`] identifier for an entity and can thus be freely
//! copied around, an `OwnedEntity` augments that "dumb" handle with the notion of ownership.
//!
//! `OwnedEntities` expose the exact same interface as an `Entity` but have an additional `Drop`
//! handler (making them non-`Copy`) that automatically [`Entity::destroy()`]s themselves when they
//! leave the scope.
//!
//! You can extract an `Entity` from them using [`OwnedEntity::entity(&self)`](OwnedEntity::entity).
//!
//! ```
//! use bort::{Entity, OwnedEntity};
//!
//! let player = OwnedEntity::new();
//!
//! let player2 = player;  // (transfers ownership of `OwnedEntity`)
//! // player.insert("hello!");
//! // ^ `player` is invalid here; use `player2` instead!
//!
//! let player_ref = player2.entity();  // (produces a copyable `Entity` reference)
//! let player_ref_2 = player_ref;  // (copies the value)
//!
//! assert_eq!(player_ref, player_ref_2);
//! assert!(player_ref.is_alive());
//!
//! drop(player2);  // (dropped `OwnedEntity`; `player_ref_xx` are all dead now)
//!
//! assert!(!player_ref.is_alive());
//! ```
//!
//! Using these `Entity` handles, you can freely reference you object in multiple places without
//! dealing smart pointers or leaky reference cycles.
//!
//! ```
//! use bort::{Entity, OwnedEntity};
//! use std::collections::HashMap;
//!
//! struct PlayerId(usize);
//!
//! struct Name(String);
//!
//! #[derive(Default)]
//! struct PlayerRegistry {
//!     all: Vec<OwnedEntity>,
//!     by_name: HashMap<String, Entity>,
//! }
//!
//! impl PlayerRegistry {
//!     pub fn add(&mut self, player: OwnedEntity) {
//!         let player_ref = player.entity();
//!
//!         // Add to list of all entities
//!         player.insert(PlayerId(self.all.len()));
//!         self.all.push(player);
//!
//!         // Add to map of entities by name
//!         self.by_name.insert(
//!             player_ref.get::<Name>().0.clone(),
//!             player_ref,
//!         );
//!     }
//! }
//!
//! let (player, player_ref) = OwnedEntity::new()
//!     .with(Name("foo".to_string()))
//!     .split_guard();  // Splits the `OwnedEntity` into a tuple of `(OwnedEntity, Entity)`.
//!
//! let mut registry = PlayerRegistry::default();
//! registry.add(player);
//!
//! println!("Player is at index {}", player_ref.get::<PlayerId>().0);
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
//! ### Borrowing
//!
//! Bort relies quite heavily on runtime borrowing. Although the type system does not prevent you
//! from violating "mutable xor immutable" rules for a given component, doing so will cause a panic
//! anyways:
//!
//! ```should_panic
//! use bort::OwnedEntity;
//!
//! let foo = OwnedEntity::new()
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
//! improve performance when compared to the regular dynamic dispatch way of doing things:
//!
//! ```
//! use bort::{Entity, storage};
//! # struct Pos(glam::Vec3);
//! # struct Vel(glam::Vec3);
//!
//! fn process_players(players: impl IntoIterator<Item = Entity>) {
//!     let positions = storage::<Pos>();
//!     let velocities = storage::<Vel>();
//!
//!     for player in players {
//!         positions.get_mut(player).0 += velocities.get(player).0;
//!     }
//! }
//! ```
//!
//! If runtime borrowing is still troublesome, the pub-in-private trick can help you define a component
//! that can only be borrowed from within trusted modules:
//!
//! ```
//! mod voxel {
//!     use bort::{OwnedEntity, Entity};
//!     use std::{cell::Ref, ops::Deref};
//!
//!     #[derive(Default)]
//!     pub struct World {
//!         chunks: Vec<OwnedEntity>,
//!     }
//!
//!     impl World {
//!         pub fn spawn_chunk(&mut self) -> Entity {
//!              let (chunk, chunk_ref) = OwnedEntity::new()
//!                 .with_debug_label("chunk")
//!                 .with(Chunk::default())
//!                 .split_guard();
//!
//!              self.chunks.push(chunk);
//!              chunk_ref
//!         }
//!
//!         pub fn chunk_state(&self, chunk: Entity) -> ChunkRef {
//!             ChunkRef(chunk.get())
//!         }
//!
//!         pub fn mutate_chunk(&mut self, chunk: Entity) {
//!             chunk.get_mut::<Chunk>().mutated = false;
//!         }
//!     }
//!
//!     mod sealed {
//!         #[derive(Default)]
//!         pub struct Chunk {
//!             pub(super) mutated: bool,
//!         }
//!     }
//!
//!     use sealed::Chunk;
//!
//!     impl Chunk {
//!         pub fn do_something(&self) {
//!             println!("Mutated: {}", self.mutated);
//!         }
//!     }
//!
//!     pub struct ChunkRef<'a>(Ref<'a, Chunk>);
//!
//!     impl Deref for ChunkRef<'_> {
//!         type Target = Chunk;
//!
//!         fn deref(&self) -> &Chunk {
//!             &self.0
//!         }
//!     }
//! }
//!
//! use voxel::World;
//!
//! let mut world = World::default();
//! let chunk = world.spawn_chunk();
//!
//! let chunk_ref = world.chunk_state(chunk);
//! chunk_ref.do_something();
//!
//! // `chunk_ref` is tied to the lifetime of `world`.
//! // Thus, there is no risk of accidentally leaving the guard alive.
//! drop(chunk_ref);
//!
//! world.mutate_chunk(chunk);
//! world.chunk_state(chunk).do_something();
//!
//! // No way to access the component directly.
//! // let chunk_ref = chunk.get::<voxel::Chunk>();
//! //                             ^ this is unnamable!
//! ```
//!
//! ### Threading
//!
//! TODO
//!
use std::{
    any::{type_name, Any, TypeId},
    borrow::{Borrow, Cow},
    cell::{Cell, Ref, RefCell, RefMut},
    fmt, hash, iter,
    marker::PhantomData,
    mem,
    num::NonZeroU64,
    sync::atomic,
};

use crate::{
    debug::{AsDebugLabel, DebugLabel},
    threading::cell::{ensure_main_thread, MainThreadJail, MainThreadToken, NRefCell},
};

// === Helpers === //

type NopHashBuilder = ConstSafeBuildHasherDefault<NoOpHasher>;
type NopHashMap<K, V> = hashbrown::HashMap<K, V, NopHashBuilder>;

type FxHashBuilder = ConstSafeBuildHasherDefault<rustc_hash::FxHasher>;
type FxHashMap<K, V> = hashbrown::HashMap<K, V, FxHashBuilder>;
type FxHashSet<T> = hashbrown::HashSet<T, FxHashBuilder>;

struct ConstSafeBuildHasherDefault<T>(PhantomData<fn(T) -> T>);

impl<T> ConstSafeBuildHasherDefault<T> {
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<T> Default for ConstSafeBuildHasherDefault<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: hash::Hasher + Default> hash::BuildHasher for ConstSafeBuildHasherDefault<T> {
    type Hasher = T;

    fn build_hasher(&self) -> Self::Hasher {
        T::default()
    }
}

#[derive(Default)]
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

fn xorshift64(state: NonZeroU64) -> NonZeroU64 {
    // Adapted from: https://en.wikipedia.org/w/index.php?title=Xorshift&oldid=1123949358
    let state = state.get();
    let state = state ^ (state << 13);
    let state = state ^ (state >> 7);
    let state = state ^ (state << 17);
    NonZeroU64::new(state).unwrap()
}

trait AnyDowncastExt: Any {
    fn as_any(&self) -> &dyn Any;

    fn as_any_mut(&mut self) -> &mut dyn Any;

    fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        self.as_any().downcast_ref()
    }

    fn downcast_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.as_any_mut().downcast_mut()
    }
}

// Rust currently doesn't have inherent downcast impls for `dyn (Any + Sync)`.
impl AnyDowncastExt for dyn Any + Sync {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
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
            drop(storage::<T>().try_remove_untracked(entity)); // (ignores missing components)
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
            // N.B. we don't need `assert_blessed` here because methods on `ComponentList` are only
            // called after liveness has been determined.
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
        self.extensions
            .borrow_mut()
            .entry(with.id)
            .or_insert_with(|| {
                if self.comps.contains(&with) {
                    self
                } else {
                    Self::find_extension_in_db(&self.comps, with)
                }
            })
    }

    pub fn de_extend(&'static self, without: ComponentType) -> &'static Self {
        self.de_extensions
            .borrow_mut()
            .entry(without.id)
            .or_insert_with(|| {
                if !self.comps.contains(&without) {
                    self
                } else {
                    Self::find_de_extension_in_db(&self.comps, without)
                }
            })
    }

    // === Database === //

    thread_local! {
        // N.B. we don't need `assert_blessed` here because methods on `ComponentList` are only
        // called after liveness has been determined.
        static COMP_LISTS: RefCell<FxHashSet<&'static ComponentList>> = {
            RefCell::new(FxHashSet::from_iter([
                ComponentList::empty(),
            ]))
        };
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
                // See if the key component list without the additional component is equal to the
                // base list.
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
                // See if the base component list without the removed component is equal to the key
                // list.
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

// === Random ID Generator === //

fn random_uid() -> NonZeroU64 {
    thread_local! {
        // This doesn't directly leak anything so we're fine with not checking the blessed status
        // of this value.
        static ID_GEN: Cell<NonZeroU64> = const { Cell::new(match NonZeroU64::new(1) {
            Some(v) => v,
            None => unreachable!(),
        }) };
    }

    ID_GEN.with(|v| {
        // N.B. `xorshift`, like all other well-constructed LSFRs, produces a full cycle of non-zero
        // values before repeating itself. Thus, this is an effective way to generate random but
        // unique IDs without using additional storage.
        let state = xorshift64(v.get());
        v.set(state);
        state
    })
}

// === Block allocator === //

// TODO: Consider making this adaptive.
pub const COMP_BLOCK_SIZE: usize = 128;

pub type CompBlock<T> = &'static CompBlockPointee<T>;

pub type CompBlockPointee<T> = [CompSlotPointee<T>; COMP_BLOCK_SIZE];

pub type CompSlotPointee<T> = MainThreadJail<NRefCell<Option<T>>>;

fn use_pool<R>(id: TypeId, f: impl FnOnce(&mut Vec<&'static dyn Any>) -> R) -> R {
    thread_local! {
        static POOLS: RefCell<FxHashMap<TypeId, Vec<&'static dyn Any>>> = {
            ensure_main_thread("Accessed object pools");
            Default::default()
        };
    }

    POOLS.with(|pools| {
        let mut pools = pools.borrow_mut();
        let pool = pools.entry(id).or_insert_with(Vec::new);
        f(pool)
    })
}

pub fn alloc_block<T: 'static>() -> CompBlock<T> {
    use_pool(TypeId::of::<T>(), |pool| {
        if let Some(block) = pool.pop() {
            block.downcast_ref::<CompBlockPointee<T>>().unwrap()
        } else {
            // TODO: Ensure that this doesn't cause stack overflows
            leak(std::array::from_fn(|_| Default::default()))
        }
    })
}

pub fn release_blocks<T: 'static>(blocks: impl IntoIterator<Item = CompBlock<T>>) {
    release_blocks_untyped(TypeId::of::<T>(), blocks.into_iter().map(|b| b as &dyn Any))
}

pub fn release_blocks_untyped(id: TypeId, blocks: impl IntoIterator<Item = &'static dyn Any>) {
    use_pool(id, |pool| {
        pool.extend(blocks);
    });
}

// === Storage === //

pub type CompSlot<T> = &'static CompSlotPointee<T>;

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

struct StorageDb {
    storages: FxHashMap<TypeId, &'static (dyn Any + Sync)>,
}

type StorageData<T> = NRefCell<StorageInner<T>>;

#[derive(Debug)]
struct StorageInner<T: 'static> {
    free_slots: Vec<CompSlot<T>>,
    mappings: NopHashMap<Entity, EntityStorageMapping<T>>,
}

#[derive(Debug)]
struct EntityStorageMapping<T: 'static> {
    slot: &'static CompSlotPointee<T>,
    is_external: bool,
}

static STORAGES: NRefCell<StorageDb> = NRefCell::new(StorageDb {
    storages: FxHashMap::with_hasher(ConstSafeBuildHasherDefault::new()),
});

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
/// # struct Pos(glam::Vec3);
/// # struct Vel(glam::Vec3);
///
/// fn process_players(players: impl IntoIterator<Item = Entity>) {
///     let positions = storage::<Pos>();
///     let velocities = storage::<Vel>();
///
///     for player in players {
///         positions.get_mut(player).0 += velocities.get(player).0;
///     }
/// }
/// ```
///
/// This is a short alias to [`Storage::<T>::acquire()`].
///
pub fn storage<T: 'static>() -> Storage<T> {
    let token = MainThreadToken::acquire();
    let mut db = STORAGES.borrow_mut(token);
    let inner = db
        .storages
        .entry(TypeId::of::<T>())
        .or_insert_with(|| {
            leak::<StorageData<T>>(NRefCell::new(StorageInner {
                free_slots: Vec::new(),
                mappings: NopHashMap::default(),
            }))
        })
        .downcast_ref::<StorageData<T>>()
        .unwrap();

    Storage { inner, token }
}

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
/// # struct Pos(glam::Vec3);
/// # struct Vel(glam::Vec3);
///
/// fn process_players(players: impl IntoIterator<Item = Entity>) {
///     let positions = storage::<Pos>();
///     let velocities = storage::<Vel>();
///
///     for player in players {
///         positions.get_mut(player).0 += velocities.get(player).0;
///     }
/// }
/// ```
///
/// `Storages` are not `Sync` because they employ un-synchronized interior mutability in the form of
/// [`RefCell`]'ed components.
#[derive(Debug)]
pub struct Storage<T: 'static> {
    inner: &'static StorageData<T>,
    token: &'static MainThreadToken,
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
    pub fn acquire() -> Storage<T> {
        storage()
    }

    pub fn try_preallocate_slot(
        &self,
        entity: Entity,
        slot: Option<CompSlot<T>>,
    ) -> Result<CompSlot<T>, CompSlot<T>> {
        let me = &mut *self.inner.borrow_mut(self.token);

        match me.mappings.entry(entity) {
            hashbrown::hash_map::Entry::Occupied(entry) => Err(entry.get().slot),
            hashbrown::hash_map::Entry::Vacant(entry) => {
                let (slot, is_external) = Self::allocate_slot_if_needed(&mut me.free_slots, slot);
                entry.insert(EntityStorageMapping { slot, is_external });
                Ok(slot)
            }
        }
    }

    pub fn insert_in_slot(
        &self,
        entity: Entity,
        value: T,
        slot: Option<CompSlot<T>>,
    ) -> (CompSlot<T>, Option<T>) {
        // Ensure that the entity is alive and extend the component list.
        ALIVE.with(|slots| {
            let mut slots = slots.borrow_mut();
            let slot = slots.get_mut(&entity).unwrap_or_else(|| {
                panic!(
                    "attempted to attach a component of type {} to the dead or cross-thread {:?}.",
                    type_name::<T>(),
                    entity
                )
            });

            // N.B. we always run this, regardless of entry state, to account for preallocated slots,
            // which don't update the entry yet.
            *slot = slot.extend(ComponentType::of::<T>());
        });

        // Update the storage
        let me = &mut *self.inner.borrow_mut(self.token);

        let slot = match me.mappings.entry(entity) {
            hashbrown::hash_map::Entry::Occupied(entry) => entry.get().slot,
            hashbrown::hash_map::Entry::Vacant(entry) => {
                let (slot, is_external) = Self::allocate_slot_if_needed(&mut me.free_slots, slot);
                entry.insert(EntityStorageMapping { slot, is_external });
                slot
            }
        };

        (
            slot,
            slot.get_on_main(self.token)
                .borrow_mut(self.token)
                .replace(value),
        )
    }

    fn allocate_slot_if_needed(
        free_slots: &mut Vec<CompSlot<T>>,
        slot: Option<CompSlot<T>>,
    ) -> (CompSlot<T>, bool) {
        match slot {
            Some(external_slot) => (external_slot, true),
            None => {
                if free_slots.is_empty() {
                    free_slots.extend(alloc_block::<T>().iter());
                }

                let slot = free_slots.pop().unwrap();

                (slot, false)
            }
        }
    }

    pub fn insert_and_return_slot(&self, entity: Entity, value: T) -> (CompSlot<T>, Option<T>) {
        self.insert_in_slot(entity, value, None)
    }

    /// Inserts the provided component `value` onto the specified `entity`.
    ///
    /// If the entity did not have this component before, `None` is returned.
    ///
    /// If the entity did have this component, the component is updated and the old component value
    /// is returned.
    ///
    /// ```
    /// use bort::{storage, OwnedEntity};
    ///
    /// let (my_entity, my_entity_ref) = OwnedEntity::new().split_guard();
    /// let storage = storage::<u32>();
    ///
    /// assert_eq!(storage.insert(my_entity_ref, 0), None);
    /// assert_eq!(storage.insert(my_entity_ref, 1), Some(0));
    /// assert_eq!(storage.insert(my_entity_ref, 2), Some(1));
    /// ```
    ///
    /// This method will panic if `entity` is not alive. Use [`Entity::is_alive`] to check the
    /// liveness state.
    ///
    /// See [`Entity::insert`] for a short version of this method that fetches the appropriate storage
    /// automatically.
    pub fn insert(&self, entity: Entity, value: T) -> Option<T> {
        self.insert_and_return_slot(entity, value).1
    }

    /// Removes the component of type `T` from the specified `entity`, returning the component value
    /// if the entity previously had it.
    ///
    /// ```
    /// use bort::{storage, OwnedEntity};
    ///
    /// let (my_entity, my_entity_ref) = OwnedEntity::new().split_guard();
    /// let storage = storage::<u32>();
    ///
    /// assert_eq!(storage.remove(my_entity_ref), None);
    /// storage.insert(my_entity_ref, 0);
    /// assert_eq!(storage.remove(my_entity_ref), Some(0));
    /// assert_eq!(storage.remove(my_entity_ref), None);
    /// ```
    ///
    /// This method will panic if the `entity` is dead and the component is missing. If the entity
    /// is actively being destroyed but the component is still attached to the entity (a scenario
    /// which could occur in the `Drop` handler of an entity component), this method will remove the
    /// component normally.
    ///
    /// See [`Entity::remove`] for a short version of this method that fetches the appropriate storage
    /// automatically.
    pub fn remove(&self, entity: Entity) -> Option<T> {
        if let Some(removed) = self.try_remove_untracked(entity) {
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

    fn try_remove_untracked(&self, entity: Entity) -> Option<T> {
        let mut me = self.inner.borrow_mut(self.token);

        if let Some(mapping) = me.mappings.remove(&entity) {
            let taken = mapping
                .slot
                .get_on_main(self.token)
                .borrow_mut(self.token)
                .take();

            if !mapping.is_external {
                me.free_slots.push(mapping.slot);
            }
            taken
        } else {
            None
        }
    }

    pub fn try_get_slot(&self, entity: Entity) -> Option<CompSlot<T>> {
        self.inner
            .borrow(self.token)
            .mappings
            .get(&entity)
            .map(|mapping| mapping.slot)
    }

    pub fn get_slot(&self, entity: Entity) -> CompSlot<T> {
        self.try_get_slot(entity).unwrap_or_else(|| {
            panic!(
                "failed to find component of type {} for {:?}",
                type_name::<T>(),
                entity,
            )
        })
    }

    /// Attempts to fetch and immutably borrow the component belonging to the specified `entity`,
    /// returning a [`CompRef`] on success.
    ///
    /// Returns `None` if the entity is either missing the component or dead.
    ///
    /// Panics if the component is already borrowed mutably.
    ///
    /// ```
    /// use bort::{storage, OwnedEntity};
    ///
    /// let (my_entity, my_entity_ref) = OwnedEntity::new().split_guard();
    /// let storage = storage::<u32>();
    ///
    /// assert_eq!(storage.try_get(my_entity_ref).as_deref(), None);
    /// storage.insert(my_entity_ref, 0);
    /// assert_eq!(storage.try_get(my_entity_ref).as_deref(), Some(&0));
    ///
    /// // Destroy the entity
    /// drop(my_entity);
    ///
    /// // The dangling reference resolves to `None`
    /// assert_eq!(storage.try_get(my_entity_ref).as_deref(), None);
    /// ```
    ///
    /// If the entity is actively being destroyed but the component is still attached to the entity
    /// (a scenario which could occur in the `Drop` handler of an entity component), this method will
    /// fetch and borrow the component normally.
    ///
    /// If the returned [`CompRef`] remains alive while the entity is being destroyed, the entity
    /// destructor will panic.
    ///
    /// See [`Entity::try_get`] for a short version of this method that fetches the appropriate
    /// storage automatically.
    pub fn try_get(&self, entity: Entity) -> Option<CompRef<T>> {
        self.try_get_slot(entity).and_then(|slot| {
            Ref::filter_map(slot.get_on_main(self.token).borrow(self.token), |v| {
                v.as_ref()
            })
            .ok()
        })
    }

    /// Attempts to fetch and mutably borrow the component belonging to the specified `entity`,
    /// returning a [`CompMut`] on success.
    ///
    /// Returns `None` if the entity is either missing the component or dead.
    ///
    /// Panics if the component is already borrowed either mutably or immutably.
    ///
    /// ```
    /// use bort::{storage, OwnedEntity};
    ///
    /// let (my_entity, my_entity_ref) = OwnedEntity::new().split_guard();
    /// let storage = storage::<u32>();
    ///
    /// assert_eq!(storage.try_get_mut(my_entity_ref).as_deref(), None);
    /// storage.insert(my_entity_ref, 0);
    /// assert_eq!(storage.try_get_mut(my_entity_ref).as_deref(), Some(&0));
    ///
    /// // Destroy the entity
    /// drop(my_entity);
    ///
    /// // The dangling reference resolves to `None`
    /// assert_eq!(storage.try_get_mut(my_entity_ref).as_deref(), None);
    /// ```
    ///
    /// If the entity is actively being destroyed but the component is still attached to the entity
    /// (a scenario which could occur in the `Drop` handler of an entity component), this method will
    /// fetch and borrow the component normally.
    ///
    /// If the returned [`CompMut`] remains alive while the entity is being destroyed, the entity
    /// destructor will panic.
    ///
    /// See [`Entity::try_get_mut`] for a short version of this method that fetches the appropriate
    /// storage automatically.
    pub fn try_get_mut(&self, entity: Entity) -> Option<CompMut<T>> {
        self.try_get_slot(entity).map(|slot| {
            RefMut::map(slot.get_on_main(self.token).borrow_mut(self.token), |v| {
                v.as_mut().unwrap()
            })
        })
    }

    /// Fetches and immutably borrow the component belonging to the specified `entity`, returning a
    /// [`CompRef`].
    ///
    /// Panics if the entity is either missing the component or dead. This method will also panic if
    /// the component is already borrowed mutably.
    ///
    /// ```
    /// use bort::{storage, OwnedEntity};
    ///
    /// let my_entity = OwnedEntity::new();
    /// let storage = storage::<u32>();
    ///
    /// storage.insert(my_entity.entity(), 0);
    /// assert_eq!(&*storage.get(my_entity.entity()), &0);
    /// ```
    ///
    /// If the entity is actively being destroyed but the component is still attached to the entity
    /// (a scenario which could occur in the `Drop` handler of an entity component), this method will
    /// fetch and borrow the component normally.
    ///
    /// If the returned [`CompRef`] remains alive while the entity is being destroyed, the entity
    /// destructor will panic.
    ///
    /// See [`Entity::get`] for a short version of this method that fetches the appropriate
    /// storage automatically.
    pub fn get(&self, entity: Entity) -> CompRef<T> {
        Ref::map(
            self.get_slot(entity)
                .get_on_main(self.token)
                .borrow(self.token),
            |v| v.as_ref().unwrap(),
        )
    }

    /// Fetches and mutably borrow the component belonging to the specified `entity`, returning a
    /// [`CompMut`].
    ///
    /// Panics if the entity is either missing the component or dead. This method will also panic if
    /// the component is already borrowed either mutably or immutably.
    ///
    /// ```
    /// use bort::{storage, OwnedEntity};
    ///
    /// let (my_entity, my_entity_ref) = OwnedEntity::new().split_guard();
    /// let storage = storage::<u32>();
    ///
    /// storage.insert(my_entity_ref, 0);
    ///
    /// let mut value = storage.get_mut(my_entity_ref);
    /// assert_eq!(&*value, &0);
    /// *value = 1;
    /// assert_eq!(&*value, &1);
    /// ```
    ///
    /// If the entity is actively being destroyed but the component is still attached to the entity
    /// (a scenario which could occur in the `Drop` handler of an entity component), this method will
    /// fetch and borrow the component normally.
    ///
    /// If the returned [`CompMut`] remains alive while the entity is being destroyed, the entity
    /// destructor will panic.
    ///
    /// See [`Entity::get_mut`] for a short version of this method that fetches the appropriate
    /// storage automatically.
    pub fn get_mut(&self, entity: Entity) -> CompMut<T> {
        RefMut::map(
            self.get_slot(entity)
                .get_on_main(self.token)
                .borrow_mut(self.token),
            |v| v.as_mut().unwrap(),
        )
    }

    /// Returns whether the specified `entity` has a component of this type.
    ///
    /// Returns `false` if the entity is already dead.
    ///
    /// ```
    /// use bort::{storage, OwnedEntity};
    ///
    /// let (my_entity, my_entity_ref) = OwnedEntity::new().split_guard();
    /// let storage = storage::<u32>();
    ///
    /// assert!(!storage.has(my_entity_ref));
    /// storage.insert(my_entity_ref, 0);
    /// assert!(storage.has(my_entity_ref));
    ///
    /// // Destroy the entity
    /// drop(my_entity);
    ///
    /// // The dangling check resolves `false`.
    /// assert!(!storage.has(my_entity_ref));
    /// ```
    ///
    pub fn has(&self, entity: Entity) -> bool {
        self.try_get_slot(entity).is_some()
    }
}

// === Entity === //

static DEBUG_ENTITY_COUNTER: atomic::AtomicU64 = atomic::AtomicU64::new(0);

thread_local! {
    static ALIVE: RefCell<NopHashMap<Entity, &'static ComponentList>> = {
        ensure_main_thread("Spawned, despawned, or checked the liveness of an entity");
        Default::default()
    };
}

/// An entity represents a [`Copy`]able reference single logical object (e.g. a player, a zombie, a
/// UI widget, etc) containing an arbitrary number of typed components.
///
/// Internally, the definition of an `Entity` is just:
///
/// ```
/// # use std::num::NonZeroU64;
/// #
/// #[derive(Copy, Clone, Hash, Eq, PartialEq)]
/// struct Entity(NonZeroU64);
/// ```
///
/// ...so these are as cheap as pointers on 64-bit machines to copy around.
///
/// ## Spawning
///
/// Entities are usually spawned as their managed [`OwnedEntity`] counterpart, which is a simple
/// wrapper around a real `Entity` instance which destroys the entity on `Drop`. An actual `Entity`
/// handle can be extracted from it using the [`OwnedEntity::entity()`] method.
///
/// ```
/// use bort::OwnedEntity;
///
/// let player = OwnedEntity::new();
/// let player_ref = player.entity();  // <-- This is an `Entity`
/// ```
///
/// Every method available on `Entity` is also available on `OwnedEntity`.
///
/// You can also create an unmanaged `Entity` instance directly using [`Entity::new_unmanaged()`] or
/// by calling [`OwnedEntity::unmanage`] on an `OwnedEntity`. These, however, are quite dangerous
/// because, even if you remain vigilant about [`Entity::destroy()`]ing entities at the end of their
/// lifetime, a panic could still force the stack to unwind before then, potentially leaking the
/// entity.
///
/// ```
/// use std::panic::catch_unwind;
/// use bort::{Entity, debug::alive_entity_count};
///
/// assert_eq!(alive_entity_count(), 0);
///
/// catch_unwind(|| {
///     let player = Entity::new_unmanaged();
///
///     fn uses_entity(entity: Entity) {
///         panic!("Oh no, I panicked!");
///     }
///
///     uses_entity(player);
///
///     // This method call is never reached.
///     // So much for vigilance!
///     player.destroy();
/// });
///
/// // Oh no, we leaked the player entity!
/// assert_eq!(alive_entity_count(), 1);
/// ```
///
/// Oftentimes, it is much more appropriate to spawn an `OwnedEntity` and extract a working `Entity`
/// instance from it like so:
///
/// ```
/// use std::panic::catch_unwind;
/// use bort::{OwnedEntity, Entity, debug::alive_entity_count};
///
/// assert_eq!(alive_entity_count(), 0);
///
/// catch_unwind(|| {
///     let (player, player_ref) = OwnedEntity::new().split_guard();
///
///     fn uses_entity(entity: Entity) {
///         panic!("Oh no, I panicked!");
///     }
///
///     fn spawn_entity_in_world(entity: OwnedEntity) {
///         // Additionally, entity ownership is now much clearer!
///     }
///
///     uses_entity(player_ref);
///     spawn_entity_in_world(player);
/// });
///
/// // Good news, nothing was leaked!
/// assert_eq!(alive_entity_count(), 0);
/// ```
///
/// ## Component Management
///
/// Components can be added to and removed from entities using the [`Entity::insert()`] and
/// [`Entity::remove()`] methods. Like [`HashMap::insert()`](std::collections::HashMap::insert) and
/// [`HashMap::remove()`](std::collections::HashMap::remove), `Entity::insert()` will overwrite
/// instances and return the old value and `Entity::remove()` will silently ignore operations on an
/// entity without the specified component.
///
/// ```
/// use bort::OwnedEntity;
/// use glam::Vec3;
///
/// #[derive(Debug, PartialEq)]
/// struct Pos(Vec3);
///
/// #[derive(Debug, PartialEq)]
/// struct Vel(Vec3);
///
/// let (player, player_ref) = OwnedEntity::new().split_guard();
///
/// player_ref.insert(Pos(Vec3::new(-1.3, -0.45, 4.76)));
/// player_ref.insert(Pos(Vec3::ZERO));  // ^ Overwrites the previous assignment.
/// player_ref.insert(Vel(-Vec3::Y));
///
/// assert_eq!(player_ref.remove::<Vel>(), Some(Vel(-Vec3::Y)));
/// assert_eq!(player_ref.remove::<Vel>(), None);  // ^ The component was already removed.
/// ```
///
/// References to components can be acquired using [`Entity::get()`] and [`Entity::get_mut()`]. These
/// methods return [`CompRef`]s and [`CompMut`]s, which are just fancy type aliases around
/// [`std::cell::Ref`] and [`std::cell::RefMut`].
///
/// ```
/// # use glam::Vec3;
/// # struct Pos(Vec3);
/// # let (player, player_ref) = bort::OwnedEntity::new().with(Pos(Vec3::ZERO)).split_guard();
///
/// // Fetch an immutable reference to the position.
/// let pos = player.get::<Pos>();
///
/// assert_eq!(pos.0, Vec3::ZERO);
///
/// // Fetch a mutable reference to the position.
///
/// // Because borrows are checked using `Ref` guards, we need to drop the reference before we can
/// // mutably borrow the same component mutably, lest we panic!
/// drop(pos);
///
/// // To access the `DerefMut` method of a `RefMut`—as is done implicitly when we access attempt to
/// // mutate the interior vector—we need to provide a `&mut` reference, hence the `mut` qualifier on
/// // the variable.
/// let mut pos = player.get_mut::<Pos>();
///
/// pos.0 += Vec3::Y;
///
/// assert_eq!(pos.0, Vec3::new(0.0, 1.0, 0.0));
/// ```
///
/// We can also fallibly acquire a component using [`Entity::try_get()`] and [`Entity::try_get_mut()`].
/// Instead of panicking if the entity is dead or the component is missing, these variants will simply
/// return `None`. These methods will still panic on runtime borrow violations, however.
///
/// ```
/// # use glam::Vec3;
/// # #[derive(Debug, PartialEq)]
/// # struct Pos(Vec3);
/// # struct Vel(Vec3);
/// # let (player, player_ref) = bort::OwnedEntity::new().with(Pos(Vec3::Y)).split_guard();
///
/// // This component exists...
/// assert_eq!(player_ref.try_get::<Pos>().as_deref(), Some(&Pos(Vec3::new(0.0, 1.0, 0.0))));
///
/// // This component does not...
/// assert!(player_ref.try_get_mut::<Vel>().is_none());
///
/// // And now that we destroy the player...
/// drop(player);  // Recall: `player` is the `OwnedEntity` guard managing the instance's lifetime.
///
/// // It's gone!
/// assert!(player_ref.try_get::<Pos>().is_none());
/// ```
///
/// Finally, we can also check whether an entity has a component without borrowing it using [`Entity::has`].
/// The same liveness semantics as [`Entity::try_get()`] apply here as well.
///
/// ```
/// # use bort::OwnedEntity;
/// # use glam::Vec3;
/// # struct Pos(Vec3);
/// # struct Vel(Vec3);
/// let (player, player_ref) = OwnedEntity::new().split_guard();
///
/// player.insert(Pos(Vec3::ZERO));
/// player.insert(Vel(Vec3::ZERO));
///
/// assert!(player.has::<Pos>());
/// assert!(player.has::<Vel>());
///
/// player.remove::<Vel>();
///
/// assert!(!player.has::<Vel>());
///
/// drop(player);
///
/// assert!(!player_ref.has::<Pos>());
/// ```
///
/// ## Construction
///
/// In general, `Entity::with_xx` methods are *builder* methods. That is, they are designed to be
/// chained in an expression to build up an entity.
///
/// The [`Entity::with_debug_label`] method allows you to attach a [`DebugLabel`] to the entity in
/// builds with `debug_assertions` enabled.
///
/// ```
/// use bort::{OwnedEntity, debug::DebugLabel};
///
/// let player = OwnedEntity::new()
///     .with_debug_label("my player :)");
///
/// if cfg!(debug_assertions) {
///     assert_eq!(&player.get::<DebugLabel>().0, "my player :)");
/// } else {
///     assert!(!player.has::<DebugLabel>());
/// }
/// ```
///
/// The [`Entity::with`] method allows you to attach any component to the entity. Like [`Entity::insert`],
/// this will overwrite existing component instances.
///
/// ```
/// # use bort::OwnedEntity;
/// # use glam::Vec3;
/// # struct Pos(Vec3);
/// # struct Vel(Vec3);
///
/// let player = OwnedEntity::new()
///     .with(Pos(Vec3::new(-1.3, -0.45, 4.76)))
///     .with(Vel(Vec3::ZERO))
///     .with(Pos(Vec3::ZERO));
/// ```
///
/// Finally, you can succinctly construct self-referential structures using [`Entity::with_self_referential`].
///
/// ```
/// # use bort::{OwnedEntity, Entity};
///
/// struct MyStruct {
///     me: Entity,
/// }
///
/// let player = OwnedEntity::new()
///     .with_self_referential(|me| MyStruct { me });
/// ```
///
/// ## Lifetime
///
/// TODO
///
/// ## Performance
///
/// TODO
///
#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct Entity(NonZeroU64);

impl Entity {
    pub fn new_unmanaged() -> Self {
        // Allocate a slot
        let me = Self(random_uid());

        // Register our slot in the alive set
        // N.B. we call `ComponentList::empty()` within the `ALIVE.with` section to ensure that blessed
        // validation occurs before we allocate an empty component list.
        ALIVE.with(|slots| slots.borrow_mut().insert(me, ComponentList::empty()));

        // Increment the total entity counter
        // N.B. we do this once everything else has succeeded so that calls to `new_unmanaged` on
        // un-blessed threads don't result in a phantom entity being recorded in the counter.
        DEBUG_ENTITY_COUNTER.fetch_add(1, atomic::Ordering::Relaxed);

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

    pub fn try_preallocate_slot<T: 'static>(
        self,
        slot: Option<CompSlot<T>>,
    ) -> Result<CompSlot<T>, CompSlot<T>> {
        storage::<T>().try_preallocate_slot(self, slot)
    }

    pub fn insert_in_slot<T: 'static>(
        self,
        comp: T,
        slot: Option<CompSlot<T>>,
    ) -> (CompSlot<T>, Option<T>) {
        storage::<T>().insert_in_slot(self, comp, slot)
    }

    pub fn insert_and_return_slot<T: 'static>(self, comp: T) -> (CompSlot<T>, Option<T>) {
        storage::<T>().insert_and_return_slot(self, comp)
    }

    pub fn insert<T: 'static>(self, comp: T) -> Option<T> {
        storage::<T>().insert(self, comp)
    }

    pub fn remove<T: 'static>(self) -> Option<T> {
        storage::<T>().remove(self)
    }

    pub fn try_get_slot<T: 'static>(self) -> Option<CompSlot<T>> {
        storage::<T>().try_get_slot(self)
    }

    pub fn try_get<T: 'static>(self) -> Option<CompRef<T>> {
        storage::<T>().try_get(self)
    }

    pub fn try_get_mut<T: 'static>(self) -> Option<CompMut<T>> {
        storage::<T>().try_get_mut(self)
    }

    pub fn get_slot<T: 'static>(self) -> CompSlot<T> {
        storage::<T>().get_slot(self)
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
            // Remove the slot
            let slot = slots.borrow_mut().remove(&self).unwrap_or_else(|| {
                panic!(
                    "attempted to destroy the already-dead or cross-threaded {:?}.",
                    self
                )
            });

            // Run the component destructors
            slot.run_dtors(self);
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

            if let Some(components) = alive.borrow().get(self) {
                let mut builder = f.debug_tuple("Entity");

                if let Some(label) = self.try_get::<DebugLabel>() {
                    builder.field(&label);
                }

                builder.field(&Id(self.0));

                for v in components.comps.iter() {
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

#[derive(Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct OwnedEntity {
    entity: Entity,
}

impl OwnedEntity {
    // === Lifecycle === //

    pub fn new() -> Self {
        Self::from_raw_entity(Entity::new_unmanaged())
    }

    pub fn from_raw_entity(entity: Entity) -> Self {
        Self { entity }
    }

    pub fn entity(&self) -> Entity {
        self.entity
    }

    pub fn unmanage(self) -> Entity {
        let entity = self.entity;
        mem::forget(self);

        entity
    }

    pub fn split_guard(self) -> (Self, Entity) {
        let entity = self.entity();
        (self, entity)
    }

    // === Forwards === //

    pub fn with<T: 'static>(self, comp: T) -> Self {
        self.entity.insert(comp);
        self
    }

    pub fn with_self_referential<T: 'static>(self, func: impl FnOnce(Entity) -> T) -> Self {
        self.entity.insert(func(self.entity()));
        self
    }

    pub fn with_debug_label<L: AsDebugLabel>(self, label: L) -> Self {
        self.entity.with_debug_label(label);
        self
    }

    pub fn try_preallocate_slot<T: 'static>(
        &self,
        slot: Option<CompSlot<T>>,
    ) -> Result<CompSlot<T>, CompSlot<T>> {
        self.entity.try_preallocate_slot(slot)
    }

    pub fn insert_in_slot<T: 'static>(
        &self,
        comp: T,
        slot: Option<CompSlot<T>>,
    ) -> (CompSlot<T>, Option<T>) {
        self.entity.insert_in_slot(comp, slot)
    }

    pub fn insert_and_return_slot<T: 'static>(&self, comp: T) -> (CompSlot<T>, Option<T>) {
        self.entity.insert_and_return_slot(comp)
    }

    pub fn insert<T: 'static>(&self, comp: T) -> Option<T> {
        self.entity.insert(comp)
    }

    pub fn remove<T: 'static>(&self) -> Option<T> {
        self.entity.remove()
    }

    pub fn try_get_slot<T: 'static>(&self) -> Option<CompSlot<T>> {
        self.entity.try_get_slot()
    }

    pub fn try_get<T: 'static>(&self) -> Option<CompRef<T>> {
        self.entity.try_get()
    }

    pub fn try_get_mut<T: 'static>(&self) -> Option<CompMut<T>> {
        self.entity.try_get_mut()
    }

    pub fn get_slot<T: 'static>(&self) -> CompSlot<T> {
        self.entity.get_slot()
    }

    pub fn get<T: 'static>(&self) -> CompRef<T> {
        self.entity.get()
    }

    pub fn get_mut<T: 'static>(&self) -> CompMut<T> {
        self.entity.get_mut()
    }

    pub fn has<T: 'static>(&self) -> bool {
        self.entity.has::<T>()
    }

    pub fn is_alive(&self) -> bool {
        self.entity.is_alive()
    }

    pub fn destroy(self) {
        drop(self);
    }
}

impl Borrow<Entity> for OwnedEntity {
    fn borrow(&self) -> &Entity {
        &self.entity
    }
}

impl Drop for OwnedEntity {
    fn drop(&mut self) {
        self.entity.destroy();
    }
}

// === Obj === //

pub struct Obj<T: 'static> {
    entity: Entity,
    slot: ObjPointeeSlot<T>,
}

pub type ObjPointeeSlot<T> = CompSlot<ObjPointee<T>>;

#[derive(Debug, Copy, Clone)]
pub struct ObjPointee<T> {
    pub entity: Entity,
    pub value: T,
}

impl<T: 'static> Obj<T> {
    pub fn new_unmanaged(value: T) -> Self {
        Self::wrap(Entity::new_unmanaged(), value)
    }

    pub fn wrap(entity: Entity, value: T) -> Self {
        let (slot, _) = entity.insert_and_return_slot(ObjPointee { entity, value });
        Self { entity, slot }
    }

    pub fn entity(self) -> Entity {
        self.entity
    }

    pub fn with_debug_label<L: AsDebugLabel>(self, label: L) -> Self {
        self.entity.with_debug_label(label);
        self
    }

    pub fn slot(self) -> ObjPointeeSlot<T> {
        self.entity.get_slot()
    }

    pub fn try_get(self) -> Option<CompRef<T>> {
        let token = MainThreadToken::acquire();

        CompRef::filter_map(
            self.slot.get_on_main(token).borrow(token),
            |slot| match slot {
                Some(slot) if slot.entity == self.entity => Some(&slot.value),
                _ => None,
            },
        )
        .ok()
    }

    pub fn try_get_mut(self) -> Option<CompMut<T>> {
        let token = MainThreadToken::acquire();

        CompMut::filter_map(
            self.slot.get_on_main(token).borrow_mut(token),
            |slot| match slot {
                Some(slot) if slot.entity == self.entity => Some(&mut slot.value),
                _ => None,
            },
        )
        .ok()
    }

    pub fn get(self) -> CompRef<T> {
        self.try_get()
            .expect("attempted to get the value of a dead Obj")
    }

    pub fn get_mut(self) -> CompMut<T> {
        self.try_get_mut()
            .expect("attempted to get the value of a dead Obj")
    }

    pub fn is_alive(self) -> bool {
        self.entity.is_alive()
    }

    pub fn destroy(self) {
        self.entity.destroy()
    }
}

impl<T: 'static + fmt::Debug> fmt::Debug for Obj<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut builder = f.debug_struct("Obj");
        builder.field("entity", &self.entity);

        // If we're not the main thread...
        let Some(token) = MainThreadToken::try_acquire() else {
            // Ensure that it is borrowed by us and not someone else.
            return if self.is_alive() {
                #[derive(Debug)]
                struct NonMainThread;

                builder.field("value", &NonMainThread).finish()
            } else {
                builder.finish_non_exhaustive()
            };
        };

        // If it's already borrowed...
        let Ok(pointee) = self.slot.get_on_main(token).try_borrow(token) else {
            // Ensure that it is borrowed by us and not someone else.
            return if self.is_alive() {
                #[derive(Debug)]
                struct AlreadyBorrowed;

                builder.field("value", &AlreadyBorrowed).finish()
            } else {
                builder.finish_non_exhaustive()
            };
        };

        // If the slot is inappropriate for our value, we are dead.
        let Some(pointee) = &*pointee else {
            return builder.finish_non_exhaustive();
        };

        if pointee.entity != self.entity {
            return builder.finish_non_exhaustive();
        }

        // Otherwise, display the value
        builder.field("value", &pointee.value);
        builder.finish()
    }
}

impl<T: 'static> Copy for Obj<T> {}

impl<T: 'static> Clone for Obj<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static> hash::Hash for Obj<T> {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.entity.hash(state);
    }
}

impl<T: 'static> Eq for Obj<T> {}

impl<T: 'static> PartialEq for Obj<T> {
    fn eq(&self, other: &Self) -> bool {
        self.entity == other.entity
    }
}

impl<T: 'static> Ord for Obj<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.entity.cmp(&other.entity)
    }
}

impl<T: 'static> PartialOrd for Obj<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T: 'static> Borrow<Entity> for Obj<T> {
    fn borrow(&self) -> &Entity {
        &self.entity
    }
}

// === OwnedObj === //

pub struct OwnedObj<T: 'static> {
    obj: Obj<T>,
}

impl<T: 'static> OwnedObj<T> {
    // === Lifecycle === //

    pub fn new(value: T) -> Self {
        Self::from_raw_obj(Obj::new_unmanaged(value))
    }

    pub fn wrap(entity: OwnedEntity, value: T) -> Self {
        let obj = Self::from_raw_obj(Obj::wrap(entity.entity(), value));
        // N.B. we unmanage the entity here to ensure that it gets dropped if the above call panics.
        entity.unmanage();
        obj
    }

    pub fn from_raw_obj(obj: Obj<T>) -> Self {
        Self { obj }
    }

    pub fn obj(&self) -> Obj<T> {
        self.obj
    }

    pub fn entity(&self) -> Entity {
        self.obj.entity()
    }

    pub fn owned_entity(self) -> OwnedEntity {
        OwnedEntity::from_raw_entity(self.unmanage().entity())
    }

    pub fn unmanage(self) -> Obj<T> {
        let obj = self.obj;
        mem::forget(self);
        obj
    }

    pub fn split_guard(self) -> (Self, Obj<T>) {
        let obj = self.obj();
        (self, obj)
    }

    // === Forwards === //

    pub fn with_debug_label<L: AsDebugLabel>(self, label: L) -> Self {
        self.obj.with_debug_label(label);
        self
    }

    pub fn slot(&self) -> ObjPointeeSlot<T> {
        self.obj.slot()
    }

    pub fn try_get(self) -> Option<CompRef<T>> {
        self.obj.try_get()
    }

    pub fn try_get_mut(self) -> Option<CompMut<T>> {
        self.obj.try_get_mut()
    }

    pub fn get(self) -> CompRef<T> {
        self.obj.get()
    }

    pub fn get_mut(self) -> CompMut<T> {
        self.obj.get_mut()
    }

    pub fn is_alive(&self) -> bool {
        self.obj.is_alive()
    }

    pub fn destroy(self) {
        drop(self);
    }
}

impl<T: 'static> Drop for OwnedObj<T> {
    fn drop(&mut self) {
        self.obj.destroy();
    }
}

impl<T: 'static + fmt::Debug> fmt::Debug for OwnedObj<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OwnedObj").field("obj", &self.obj).finish()
    }
}

impl<T: 'static> hash::Hash for OwnedObj<T> {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.obj.hash(state);
    }
}

impl<T: 'static> Eq for OwnedObj<T> {}

impl<T: 'static> PartialEq for OwnedObj<T> {
    fn eq(&self, other: &Self) -> bool {
        self.obj == other.obj
    }
}

impl<T: 'static> Ord for OwnedObj<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.obj.cmp(&other.obj)
    }
}

impl<T: 'static> PartialOrd for OwnedObj<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.obj.partial_cmp(&other.obj)
    }
}

impl<T: 'static> Borrow<Obj<T>> for OwnedObj<T> {
    fn borrow(&self) -> &Obj<T> {
        &self.obj
    }
}

impl<T: 'static> Borrow<Entity> for OwnedObj<T> {
    fn borrow(&self) -> &Entity {
        &self.obj.entity
    }
}

// === Debug utilities === //

pub mod debug {
    //! Debug helpers exposing various runtime statistics and mechanisms for assigning and querying
    //! entity debug labels.

    use super::*;

    /// Returns the number of entities currently alive on this thread.
    ///
    /// ```
    /// use bort::debug::alive_entity_count;
    ///
    /// let count = alive_entity_count();
    ///
    /// if count > 0 {
    ///     println!(
    ///         "Leaked {} {} before program exit.",
    ///         count,
    ///         if count == 1 { "entity" } else { "entities" },
    ///     );
    /// } else {
    ///     println!("No entities remaining!");
    /// }
    /// ```
    ///
    pub fn alive_entity_count() -> usize {
        ALIVE.with(|slots| slots.borrow().len())
    }

    /// Returns the list of all currently alive entities on this thread. This can be somewhat expensive
    /// if you have a lot of entities in your application and should, as its classification implies,
    /// only be used for debug purposes.
    ///
    /// ```
    /// use bort::debug::alive_entities;
    ///
    /// println!("The following entities are currently alive:");
    ///
    /// for entity in alive_entities() {
    ///     println!("- {entity:?}");
    /// }
    /// ```
    ///
    /// If you just need the total number of entities alive at a given time, you can use [`alive_entity_count`],
    /// which is much cheaper.
    ///
    pub fn alive_entities() -> Vec<Entity> {
        ALIVE.with(|slots| slots.borrow().keys().copied().collect())
    }

    /// Returns the total number of entities to have ever been spawned anywhere in this application.
    pub fn spawned_entity_count() -> u64 {
        DEBUG_ENTITY_COUNTER.load(atomic::Ordering::Relaxed)
    }

    /// The component [`Entity::with_debug_label`] uses to record the provided debug label.
    ///
    /// These can be constructed from any object implementing [`AsDebugLabel`] (e.g. `&'static str`,
    /// `fmt::Arguments`) using its [`From`] conversion.
    ///
    /// Manually accessing a `DebugLabel` can allow you to reflect the entity's debug label at runtime:
    ///
    /// ```
    /// use bort::{OwnedEntity, debug::DebugLabel};
    ///
    /// let my_entity = OwnedEntity::new()
    ///     .with_debug_label("my entity 1");
    ///
    /// if cfg!(debug_assertions) {
    ///     assert_eq!(&my_entity.get::<DebugLabel>().0, "my entity 1");
    /// } else {
    ///     // Recall that `with_debug_label` only attaches the debug label in
    ///     // debug builds.
    ///     assert!(!my_entity.has::<DebugLabel>());
    /// }
    /// ```
    ///
    /// Just remember that [`with_debug_label()`](Entity::with_debug_label) only attaches debug labels
    /// to entities in debug builds with `debug_assertions` turned on.
    ///
    /// If you wish to attach a debug label unconditionally, you can add it to the entity as if it
    /// were any other component:
    ///
    /// ```
    /// use bort::{OwnedEntity, debug::DebugLabel};
    ///
    /// let my_entity = OwnedEntity::new()
    ///     .with(DebugLabel::from("my entity 1"));
    ///
    /// // This always works, even in release mode!
    /// assert_eq!(&my_entity.get::<DebugLabel>().0, "my entity 1");
    /// ```
    ///
    #[derive(Debug, Clone)]
    pub struct DebugLabel(pub Cow<'static, str>);

    impl<L: AsDebugLabel> From<L> for DebugLabel {
        fn from(value: L) -> Self {
            Self(AsDebugLabel::reify(value))
        }
    }

    /// A trait implemented for anything that can be used as a debug label.
    ///
    /// More specifically, this trait is implemented for any string-like object that can be lazily
    /// converted into a `Cow<'static, str>`—that is, anything that can produce either a `'static`
    /// string slice or a dynamically created `String` instance.
    ///
    /// Objects implementing this trait should avoid performing any allocations unless [`AsDebugLabel::reify`]
    /// is called.
    ///
    /// To obtain a "reified" version of the label you can store, use the [`AsDebugLabel::reify`]
    /// associated function:
    ///
    /// ```
    /// use bort::debug::AsDebugLabel;
    /// # fn do_something_with_name<T>(_: T) {}
    ///
    /// fn set_the_name(label: impl AsDebugLabel) {
    ///     if cfg!(debug_assertions) {
    ///         // `AsDebugLabel::reify` turns it into a `Cow<'static, str>`.
    ///         // Also note that this is an associated function—not a method—hence this calling
    ///         // convention.
    ///         do_something_with_name(AsDebugLabel::reify(label));
    ///     } else {
    ///         // (do nothing)
    ///         // Ideally, nothing should have been allocated in the creation of `label` if this
    ///         // branch is taken.
    ///     }
    /// }
    /// ```
    ///
    /// There are three main implementations of this trait provided by this crate:
    ///
    /// - `&'static str`, which corresponds to string literals and other strings embedded into the
    ///   binary.
    /// - [`fmt::Arguments`], which corresponds to lazily allocated format strings created by the
    ///   standard library's [`format_args!`] macro. Unlike `Strings`, these only allocate on the
    ///   heap if [`AsDebugLabel::reify(me: Self)`](AsDebugLabel::reify) is called.
    /// - `String`, which correspond to strings allocated at runtime on the heap. These should be
    ///   avoided in practice because, even if [`AsDebugLabel::reify(me: Self)`](AsDebugLabel::reify) is never called,
    ///   the allocation required to create them will still take place.
    ///
    /// There is also an identity conversion from `Cow<'static, str>`.
    ///
    /// Usually, only the first two are used in practice. The rest are niche and there usually isn't
    /// a good reason to use them:
    ///
    /// ```
    /// use bort::debug::AsDebugLabel;
    /// # fn set_the_name(label: impl AsDebugLabel) { }
    ///
    /// set_the_name("just a string literal");
    /// set_the_name(format_args!("just a string created at {:?}", std::time::Instant::now()));
    /// ```
    ///
    pub trait AsDebugLabel {
        /// Lazily produces a `Cow<'static, str>` that can be stored as an object's debug label.
        ///
        /// This is defined as an *associated function* instead of a method and must therefore be
        /// called as `AsDebugLabel::reify(target)`.
        ///
        /// ```
        /// use bort::debug::AsDebugLabel;
        ///
        /// let reified = AsDebugLabel::reify("foo");
        /// ```
        ///
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
}

// === Threading === /

pub mod threading {
    use std::{
        any::{type_name, TypeId},
        cell::{Ref, RefMut},
    };

    use crate::{AnyDowncastExt, CompSlot, Entity, StorageData, StorageDb, StorageInner, STORAGES};

    use self::cell::{MainThreadToken, ParallelTokenSource, TypeExclusiveToken, TypeReadToken};

    // Re-export `is_main_thread`
    pub use cell::is_main_thread;

    pub fn become_main_thread() {
        cell::ensure_main_thread("Tried to become the main thread");
    }

    pub fn parallelize<F, R>(f: F) -> R
    where
        F: Send + FnOnce(ParallismSession<'_>) -> R,
        R: Send,
    {
        MainThreadToken::acquire().parallelize(|cx| f(ParallismSession::new(cx)))
    }

    // ParallismSession
    #[derive(Debug, Clone)]
    pub struct ParallismSession<'a> {
        cx: &'a ParallelTokenSource,
        db_token: TypeReadToken<'a, StorageDb>,
    }

    impl<'a> ParallismSession<'a> {
        pub fn new(cx: &'a ParallelTokenSource) -> Self {
            Self {
                cx,
                db_token: cx.read_token(),
            }
        }

        pub fn token_source(&self) -> &'a ParallelTokenSource {
            self.cx
        }

        pub fn storage<T: 'static + Send + Sync>(&self) -> ReadStorageView<T> {
            let storage = STORAGES
                .get(&self.db_token)
                .storages
                .get(&TypeId::of::<T>())
                .map(|inner| inner.downcast_ref::<StorageData<T>>().unwrap());

            ReadStorageView {
                inner: storage.map(|storage| ReadStorageViewInner {
                    storage,
                    storage_token: self.cx.read_token(),
                    value_token: self.cx.read_token(),
                }),
            }
        }

        pub fn storage_mut<T: 'static + Send + Sync>(&self) -> MutStorageView<T> {
            let storage = STORAGES
                .get(&self.db_token)
                .storages
                .get(&TypeId::of::<T>())
                .map(|inner| inner.downcast_ref::<StorageData<T>>().unwrap());

            MutStorageView {
                inner: storage.map(|storage| MutStorageViewInner {
                    storage,
                    storage_token: self.cx.read_token(),
                    value_token: self.cx.exclusive_token(),
                }),
            }
        }
    }

    // MutStorageView
    // TODO: De-duplicate code in all three storage variants
    #[derive(Debug, Clone)]
    pub struct MutStorageView<'a, T: 'static> {
        inner: Option<MutStorageViewInner<'a, T>>,
    }

    #[derive(Debug, Clone)]
    struct MutStorageViewInner<'a, T: 'static> {
        storage: &'a StorageData<T>,
        storage_token: TypeReadToken<'a, StorageInner<T>>,
        value_token: TypeExclusiveToken<'a, Option<T>>,
    }

    impl<T: 'static + Send + Sync> MutStorageView<'_, T> {
        fn inner(&self) -> &MutStorageViewInner<'_, T> {
            self.inner.as_ref().unwrap()
        }

        pub fn try_get_slot(&self, entity: Entity) -> Option<CompSlot<T>> {
            let inner = self.inner.as_ref()?;

            inner
                .storage
                .get(&inner.storage_token)
                .mappings
                .get(&entity)
                .map(|mapping| mapping.slot)
        }

        pub fn get_slot(&self, entity: Entity) -> CompSlot<T> {
            self.try_get_slot(entity).unwrap_or_else(|| {
                panic!(
                    "failed to find component of type {} for {:?}",
                    type_name::<T>(),
                    entity,
                )
            })
        }

        pub fn try_get(&self, entity: Entity) -> Option<Ref<T>> {
            self.try_get_slot(entity).and_then(|slot| {
                Ref::filter_map(slot.get().borrow(&self.inner().value_token), |v| v.as_ref()).ok()
            })
        }

        pub fn try_get_mut(&self, entity: Entity) -> Option<RefMut<T>> {
            self.try_get_slot(entity).map(|slot| {
                RefMut::map(slot.get().borrow_mut(&self.inner().value_token), |v| {
                    v.as_mut().unwrap()
                })
            })
        }

        pub fn get(&self, entity: Entity) -> Ref<T> {
            Ref::map(
                self.get_slot(entity)
                    .get()
                    .borrow(&self.inner().value_token),
                |v| v.as_ref().unwrap(),
            )
        }

        pub fn get_mut(&self, entity: Entity) -> RefMut<T> {
            RefMut::map(
                self.get_slot(entity)
                    .get()
                    .borrow_mut(&self.inner().value_token),
                |v| v.as_mut().unwrap(),
            )
        }

        pub fn has(&self, entity: Entity) -> bool {
            self.try_get_slot(entity).is_some()
        }
    }

    // ReadStorageView
    #[derive(Debug, Clone)]
    pub struct ReadStorageView<'a, T: 'static> {
        inner: Option<ReadStorageViewInner<'a, T>>,
    }

    #[derive(Debug, Clone)]
    struct ReadStorageViewInner<'a, T: 'static> {
        storage: &'a StorageData<T>,
        storage_token: TypeReadToken<'a, StorageInner<T>>,
        value_token: TypeReadToken<'a, Option<T>>,
    }

    impl<T: 'static + Send + Sync> ReadStorageView<'_, T> {
        fn inner(&self) -> &ReadStorageViewInner<'_, T> {
            self.inner.as_ref().unwrap()
        }

        pub fn try_get_slot(&self, entity: Entity) -> Option<CompSlot<T>> {
            let inner = self.inner.as_ref()?;

            inner
                .storage
                .get(&inner.storage_token)
                .mappings
                .get(&entity)
                .map(|mapping| mapping.slot)
        }

        pub fn get_slot(&self, entity: Entity) -> CompSlot<T> {
            self.try_get_slot(entity).unwrap_or_else(|| {
                panic!(
                    "failed to find component of type {} for {:?}",
                    type_name::<T>(),
                    entity,
                )
            })
        }

        pub fn try_get(&self, entity: Entity) -> Option<&T> {
            Some(
                self.try_get_slot(entity)?
                    .get()
                    .get(&self.inner().value_token)
                    .as_ref()
                    .unwrap(),
            )
        }

        pub fn get(&self, entity: Entity) -> &T {
            self.get_slot(entity)
                .get()
                .get(&self.inner().value_token)
                .as_ref()
                .unwrap()
        }

        pub fn has(&self, entity: Entity) -> bool {
            self.try_get_slot(entity).is_some()
        }
    }

    pub mod cell {
        use hashbrown::hash_map::Entry as HashMapEntry;
        use std::{
            any::{type_name, TypeId},
            cell::{BorrowError, BorrowMutError, Cell, Ref, RefCell, RefMut},
            error::Error,
            fmt,
            marker::PhantomData,
            num::NonZeroU64,
            sync::{
                atomic::{AtomicBool, AtomicU64, Ordering},
                Mutex, MutexGuard,
            },
            thread::{self, current, Thread},
        };

        use crate::FxHashMap;

        // === Helpers === //

        struct NotOnMainThread;

        impl fmt::Debug for NotOnMainThread {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("<not on main thread>")
            }
        }

        // === NRefCell === //

        #[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
        pub struct Namespace(NonZeroU64);

        impl Namespace {
            pub fn new() -> Self {
                static ALLOC: AtomicU64 = AtomicU64::new(1);

                Self(NonZeroU64::new(ALLOC.fetch_add(1, Ordering::Relaxed)).unwrap())
            }
        }

        pub unsafe trait Token<T: ?Sized>: fmt::Debug {
            fn can_access(&self, namespace: Option<Namespace>) -> bool;
            fn is_exclusive(&self) -> bool;
        }

        pub unsafe trait ReadToken<T: ?Sized>: Token<T> {}

        pub unsafe trait ExclusiveToken<T: ?Sized>: Token<T> {}

        fn unwrap_error<T, E: Error>(result: Result<T, E>) -> T {
            result.unwrap_or_else(|e| panic!("{e}"))
        }

        pub struct NRefCell<T: ?Sized> {
            namespace: AtomicU64,
            value: RefCell<T>,
        }

        unsafe impl<T: ?Sized + Send + Sync> Sync for NRefCell<T> {}

        impl<T> NRefCell<T> {
            pub const fn new(value: T) -> Self {
                Self::new_namespaced(value, None)
            }

            pub const fn new_namespaced(value: T, namespace: Option<Namespace>) -> Self {
                Self {
                    value: RefCell::new(value),
                    namespace: AtomicU64::new(match namespace {
                        Some(Namespace(id)) => id.get(),
                        None => 0,
                    }),
                }
            }

            pub fn into_inner(self) -> T {
                // Safety: this is a method that takes exclusive ownership of the object. Hence, it is
                // not impacted by our potentially dangerous `Sync` impl.
                self.value.into_inner()
            }
        }

        impl<T: ?Sized> NRefCell<T> {
            pub fn get_mut(&mut self) -> &mut T {
                // Safety: this is a method that takes exclusive ownership of the object. Hence, it is
                // not impacted by our potentially dangerous `Sync` impl.
                self.value.get_mut()
            }

            // === Namespace management === //

            pub fn namespace(&self) -> Option<Namespace> {
                NonZeroU64::new(self.namespace.load(Ordering::Relaxed)).map(Namespace)
            }

            fn assert_accessible_by(&self, token: &impl Token<T>) {
                let owner = self.namespace();
                assert!(
                    token.can_access(owner),
                    "{token:?} cannot access NRefCell under namespace {owner:?}.",
                );
            }

            pub fn set_namespace(&mut self, namespace: Option<Namespace>) {
                *self.namespace.get_mut() = namespace.map_or(0, |Namespace(id)| id.get());
            }

            pub fn set_namespace_ref(
                &self,
                guard: &impl ExclusiveToken<T>,
                namespace: Option<Namespace>,
            ) {
                self.assert_accessible_by(guard);

                // Safety: we can read and mutate the `value`'s borrow count safely because we are the
                // only thread with "write" namespace access and we know that fact will not change during
                // this operation because *this is the operation we're using to change that fact!*
                match self.value.try_borrow_mut() {
                    Ok(_) => {}
                    Err(err) => {
                        panic!(
                        "Failed to release NRefCell from namespace {:?}; dynamic borrows are still \
                         ongoing: {err}",
                        self.namespace(),
                    );
                    }
                }

                // Safety: It is forget our namespace because all write tokens on this thread acting on
                // this object have relinquished their dynamic borrows.
                self.namespace.store(
                    match namespace {
                        Some(Namespace(id)) => id.get(),
                        None => 0,
                    },
                    Ordering::Relaxed,
                );
            }

            // === Borrow methods === //

            pub fn try_get<'a>(
                &'a self,
                guard: &'a impl ReadToken<T>,
            ) -> Result<&'a T, BorrowError> {
                self.assert_accessible_by(guard);

                // Safety: we know we can read from the `value`'s borrow count safely because this method
                // can only be run so long as we have `NamespaceReadGuard`s alive and we can't cause reads
                // until we get a `NamespaceWriteGuard`s, which is only possible once `guard` is dead.
                unsafe {
                    // Safety: additionally, we know nobody can borrow this cell mutably until all
                    // `NamespaceReadGuard`s die out so this is safe as well.
                    self.value.try_borrow_unguarded()
                }
            }

            pub fn get<'a>(&'a self, guard: &'a impl ReadToken<T>) -> &'a T {
                unwrap_error(self.try_get(guard))
            }

            pub fn try_borrow<'a>(
                &'a self,
                guard: &'a impl ExclusiveToken<T>,
            ) -> Result<Ref<'a, T>, BorrowError> {
                self.assert_accessible_by(guard);

                // Safety: we know we can read and write from the `value`'s borrow count safely because
                // this method can only be run so long as we have `NamespaceWriteGuard`s alive and we
                // can't...
                //
                // a) construct `NamespaceReadGuard`s to use on other threads until `guard` dies out
                // b) move this `guard` to another thread because it's neither `Send` nor `Sync`
                // c) change the owner to admit new namespaces on other threads until all the borrows
                //    expire.
                //
                self.value.try_borrow()
            }

            pub fn borrow<'a>(&'a self, guard: &'a impl ExclusiveToken<T>) -> Ref<'a, T> {
                unwrap_error(self.try_borrow(guard))
            }

            pub fn try_borrow_mut<'a>(
                &'a self,
                guard: &'a impl ExclusiveToken<T>,
            ) -> Result<RefMut<'a, T>, BorrowMutError> {
                self.assert_accessible_by(guard);

                // Safety: see `try_borrow`.
                self.value.try_borrow_mut()
            }

            pub fn borrow_mut<'a>(&'a self, guard: &'a impl ExclusiveToken<T>) -> RefMut<'a, T> {
                unwrap_error(self.try_borrow_mut(guard))
            }
        }

        impl<T: fmt::Debug> fmt::Debug for NRefCell<T> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                if is_main_thread() {
                    f.debug_struct("NRefCell")
                        .field("namespace", &self.namespace())
                        .field("value", &self.value)
                        .finish()
                } else {
                    f.debug_struct("NRefCell")
                        .field("namespace", &self.namespace())
                        .field("value", &NotOnMainThread)
                        .finish()
                }
            }
        }

        impl<T: Default> Default for NRefCell<T> {
            fn default() -> Self {
                Self::new(T::default())
            }
        }

        // === Tokens === //

        // Blessing
        static HAS_MAIN_THREAD: AtomicBool = AtomicBool::new(false);

        thread_local! {
            static IS_MAIN_THREAD: Cell<bool> = const { Cell::new(false) };
        }

        pub fn is_main_thread() -> bool {
            IS_MAIN_THREAD.with(|v| v.get())
        }

        #[must_use]
        pub fn try_become_main_thread() -> bool {
            if is_main_thread() {
                return true;
            }

            if HAS_MAIN_THREAD
                .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                IS_MAIN_THREAD.with(|v| v.set(true));
                true
            } else {
                false
            }
        }

        pub(crate) fn ensure_main_thread(action: impl fmt::Display) {
            assert!(
                try_become_main_thread(),
                "{action} on non-main thread. See the \"multi-threading\" section of \
                 the module documenation for details.",
            );
        }

        // MainThreadToken
        #[derive(Debug, Copy, Clone)]
        pub struct MainThreadToken {
            _no_send_or_sync: PhantomData<*const ()>,
        }

        impl Default for MainThreadToken {
            fn default() -> Self {
                *Self::acquire()
            }
        }

        impl MainThreadToken {
            pub fn try_acquire() -> Option<&'static Self> {
                if try_become_main_thread() {
                    Some(&Self {
                        _no_send_or_sync: PhantomData,
                    })
                } else {
                    None
                }
            }

            pub fn acquire() -> &'static Self {
                ensure_main_thread("Attempted to acquire MainThreadToken");
                &Self {
                    _no_send_or_sync: PhantomData,
                }
            }

            pub fn exclusive_token<T: ?Sized + 'static>(&self) -> TypeExclusiveToken<'static, T> {
                TypeExclusiveToken {
                    _no_send_or_sync: PhantomData,
                    _ty: PhantomData,
                    session: None,
                }
            }

            pub fn parallelize<F, R>(&self, f: F) -> R
            where
                F: Send + FnOnce(&mut ParallelTokenSource) -> R,
                R: Send,
            {
                let mut result = None;

                thread::scope(|s| {
                    // Spawn a new thread and join it immediately.
                    //
                    // This ensures that, while borrows originating from our `MainThreadToken`
                    // could still be ongoing, they will never be acted upon until the
                    // "reborrowing" tokens expire, which live for at most the lifetime of
                    // `ParallelTokenSource`.
                    s.spawn(|| {
                        result = Some(f(&mut ParallelTokenSource {
                            borrows: Default::default(),
                        }));
                    });
                });

                result.unwrap()
            }
        }

        unsafe impl<T: ?Sized> Token<T> for MainThreadToken {
            fn can_access(&self, _: Option<Namespace>) -> bool {
                true
            }

            fn is_exclusive(&self) -> bool {
                true
            }
        }

        unsafe impl<T: ?Sized> ExclusiveToken<T> for MainThreadToken {}

        // ParallelTokenSource
        const TOO_MANY_EXCLUSIVE_ERR: &str = "too many TypeExclusiveTokens!";
        const TOO_MANY_READ_ERR: &str = "too many TypeReadTokens!";

        type BorrowMap = FxHashMap<(TypeId, Option<Namespace>), (isize, Option<Thread>)>;

        #[derive(Debug)]
        pub struct ParallelTokenSource {
            borrows: Mutex<BorrowMap>,
        }

        impl ParallelTokenSource {
            fn borrows(&self) -> MutexGuard<BorrowMap> {
                match self.borrows.lock() {
                    Ok(guard) => guard,
                    Err(err) => err.into_inner(),
                }
            }

            pub fn exclusive_token<T: ?Sized + 'static>(&self) -> TypeExclusiveToken<'_, T> {
                // Increment reference count
                match self.borrows().entry((TypeId::of::<T>(), None)) {
                    HashMapEntry::Occupied(mut entry) => {
                        let (rc, Some(owner)) = entry.get_mut() else {
                            unreachable!();
                        };

                        // Validate thread ID
                        let ty_name = type_name::<T>();
                        let current = current();

                        assert_eq!(
                            owner.id(),
                            current.id(),
                            "Cannot acquire a `TypeExclusiveToken<{ty_name}>` to a type already acquired by
                             the thread {owner:?} (current thread: {current:?})",
                        );

                        // Increment rc
                        *rc = rc.checked_add(1).expect(TOO_MANY_EXCLUSIVE_ERR);
                    }
                    HashMapEntry::Vacant(entry) => {
                        entry.insert((1, Some(current())));
                    }
                }

                // Construct token
                TypeExclusiveToken {
                    _no_send_or_sync: PhantomData,
                    _ty: PhantomData,
                    session: Some(self),
                }
            }

            pub fn read_token<T: ?Sized + 'static>(&self) -> TypeReadToken<'_, T> {
                // Increment reference count
                match self.borrows().entry((TypeId::of::<T>(), None)) {
                    HashMapEntry::Occupied(mut entry) => {
                        let (rc, owner) = entry.get_mut();
                        debug_assert!(owner.is_none());

                        // Decrement rc
                        *rc = rc.checked_sub(1).expect(TOO_MANY_READ_ERR);
                    }
                    HashMapEntry::Vacant(entry) => {
                        entry.insert((-1, None));
                    }
                }

                // Construct token
                TypeReadToken {
                    _ty: PhantomData,
                    session: self,
                }
            }

            // TODO: Remaining token types (read + namespaced variants)
        }

        // TypeExclusiveToken
        pub struct TypeExclusiveToken<'a, T: ?Sized + 'static> {
            _no_send_or_sync: PhantomData<*const ()>,
            _ty: PhantomData<fn() -> T>,
            session: Option<&'a ParallelTokenSource>,
        }

        impl<T: ?Sized + 'static> fmt::Debug for TypeExclusiveToken<'_, T> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct("TypeExclusiveToken")
                    .field("session", &self.session)
                    .finish_non_exhaustive()
            }
        }

        unsafe impl<T: ?Sized + 'static> Token<T> for TypeExclusiveToken<'_, T> {
            fn can_access(&self, _: Option<Namespace>) -> bool {
                true
            }

            fn is_exclusive(&self) -> bool {
                true
            }
        }

        unsafe impl<T: ?Sized + 'static> ExclusiveToken<T> for TypeExclusiveToken<'_, T> {}

        impl<T: ?Sized + 'static> Clone for TypeExclusiveToken<'_, T> {
            fn clone(&self) -> Self {
                if let Some(session) = self.session {
                    let mut borrows = session.borrows();
                    let (rc, _) = borrows.get_mut(&(TypeId::of::<T>(), None)).unwrap();
                    *rc = rc.checked_add(1).expect(TOO_MANY_EXCLUSIVE_ERR);
                }

                Self {
                    _no_send_or_sync: PhantomData,
                    _ty: PhantomData,
                    session: self.session,
                }
            }
        }

        impl<T: ?Sized + 'static> Drop for TypeExclusiveToken<'_, T> {
            fn drop(&mut self) {
                if let Some(session) = self.session {
                    let mut borrows = session.borrows();
                    let HashMapEntry::Occupied(mut entry) = borrows.entry((TypeId::of::<T>(), None)) else {
                        unreachable!()
                    };

                    let (rc, _) = entry.get_mut();
                    *rc -= 1;
                    if *rc == 0 {
                        entry.remove();
                    }
                }
            }
        }

        // TypeReadToken
        pub struct TypeReadToken<'a, T: ?Sized + 'static> {
            _ty: PhantomData<fn() -> T>,
            session: &'a ParallelTokenSource,
        }

        impl<T: ?Sized + 'static> fmt::Debug for TypeReadToken<'_, T> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct("TypeReadToken")
                    .field("session", &self.session)
                    .finish_non_exhaustive()
            }
        }

        unsafe impl<T: ?Sized + 'static> Token<T> for TypeReadToken<'_, T> {
            fn can_access(&self, _: Option<Namespace>) -> bool {
                true
            }

            fn is_exclusive(&self) -> bool {
                false
            }
        }

        unsafe impl<T: ?Sized + 'static> ReadToken<T> for TypeReadToken<'_, T> {}

        impl<T: ?Sized + 'static> Clone for TypeReadToken<'_, T> {
            fn clone(&self) -> Self {
                let mut borrows = self.session.borrows();
                let (rc, _) = borrows.get_mut(&(TypeId::of::<T>(), None)).unwrap();
                *rc = rc.checked_sub(1).expect(TOO_MANY_READ_ERR);

                Self {
                    _ty: PhantomData,
                    session: self.session,
                }
            }
        }

        impl<T: ?Sized + 'static> Drop for TypeReadToken<'_, T> {
            fn drop(&mut self) {
                let mut borrows = self.session.borrows();
                let HashMapEntry::Occupied(mut entry) = borrows.entry((TypeId::of::<T>(), None)) else {
                    unreachable!()
                };

                let (rc, _) = entry.get_mut();
                *rc += 1;
                if *rc == 0 {
                    entry.remove();
                }
            }
        }

        // MainThreadJail
        #[derive(Default)]
        pub struct MainThreadJail<T: ?Sized>(T);

        impl<T: ?Sized + fmt::Debug> fmt::Debug for MainThreadJail<T> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                if is_main_thread() {
                    f.debug_tuple("MainThreadJail").field(&&self.0).finish()
                } else {
                    f.debug_tuple("MainThreadJail")
                        .field(&NotOnMainThread)
                        .finish()
                }
            }
        }

        // Safety: you can only get a reference to `T` on the main thread which
        // created the object.
        unsafe impl<T> Sync for MainThreadJail<T> {}

        impl<T> MainThreadJail<T> {
            pub fn new(_: &MainThreadToken, value: T) -> Self {
                MainThreadJail(value)
            }

            pub fn into_inner(self) -> T {
                self.0
            }

            pub fn get(&self) -> &T
            where
                T: Sync,
            {
                &self.0
            }

            pub fn get_on_main(&self, _: &MainThreadToken) -> &T {
                &self.0
            }

            pub fn get_mut(&mut self) -> &mut T {
                &mut self.0
            }
        }
    }
}
