# Bort

<!-- cargo-rdme start -->

Bort implements a simple object model for Rust that aims to be convenient, intuitive, and fast.

```rust
use bort::{Entity, OwnedEntity};
use glam::Vec3;

// Define a bunch of components as plain old Rust structs.
#[derive(Debug, Copy, Clone)]
struct Pos(Vec3);

#[derive(Debug, Copy, Clone)]
struct Vel(Vec3);

#[derive(Debug)]
struct ChaserAi {
    target: Option<Entity>,
    home: Vec3,
}

#[derive(Debug)]
struct PlayerState {
    hp: u32,
}

impl PlayerState {
    fn update(&mut self, me: Entity) {
        let mut pos = me.get_mut::<Pos>();
        pos.0 += me.get::<Vel>().0;

        if pos.0.y < 0.0 {
            // Take void damage.
            self.hp -= 1;
        }
    }
}

// Spawn entities to contain those components.
let player = OwnedEntity::new()
    .with(Pos(Vec3::ZERO))
    .with(Vel(Vec3::ZERO))
    .with(PlayerState {
        hp: 100,
    });

let chaser = OwnedEntity::new()
    .with(Pos(Vec3::ZERO))
    .with(ChaserAi { target: Some(player.entity()), home: Vec3::ZERO });

// Fetch the `PlayerState` component from the entity and update its state.
player.get_mut::<PlayerState>().update(player.entity());

// Process the "chaser" monster's AI.
let pos = &mut *chaser.get_mut::<Pos>();
let state = chaser.get::<ChaserAi>();

pos.0 = pos.0.lerp(match state.target {
    Some(target) => target.get::<Pos>().0,
    None => state.home,
}, 0.2);
```

### Getting Started

Applications in Bort are made of [`Entity`] instances. These are `Copy`able references to
logical objects in your application. They can represent anything from a player character in a
game to a UI widget.

To create one, just call [`OwnedEntity::new()`].

```rust
use bort::OwnedEntity;

let player = OwnedEntity::new();
```

From there, you can add components to it using either [`insert`](Entity::insert) or [`with`](Entity::with):

```rust
player.insert(Pos(Vec3::ZERO));
player.insert(Vel(Vec3::ZERO));

// ...which is equivalent to:
let player = OwnedEntity::new()
    .with(Pos(Vec3::ZERO))
    .with(Vel(Vec3::ZERO));
```

...and access them using [`get`](Entity::get) or [`get_mut`](Entity::get_mut):

```rust
let mut pos = player.get_mut::<Pos>();  // These are `RefMut`s
let vel = player.get::<Vel>();          // ...and `Ref`s.

pos.0 += vel.0;
```

You might be wonder about the difference between an [`OwnedEntity`] and an [`Entity`]. While an
`Entity` is just a wrapper around a [`NonZeroU64`] identifier for an entity and can thus be freely
copied around, an `OwnedEntity` augments that "dumb" handle with the notion of ownership.

`OwnedEntities` expose the exact same interface as an `Entity` but have an additional `Drop`
handler (making them non-`Copy`) that automatically [`Entity::destroy()`]s themselves when they
leave the scope.

You can extract an `Entity` from them using [`OwnedEntity::entity(&self)`](OwnedEntity::entity).

```rust
use bort::{Entity, OwnedEntity};

let player = OwnedEntity::new();

let player2 = player;  // (transfers ownership of `OwnedEntity`)
// player.insert("hello!");
// ^ `player` is invalid here; use `player2` instead!

let player_ref = player2.entity();  // (produces a copyable `Entity` reference)
let player_ref_2 = player_ref;  // (copies the value)

assert_eq!(player_ref, player_ref_2);
assert!(player_ref.is_alive());

drop(player2);  // (dropped `OwnedEntity`; `player_ref_xx` are all dead now)

assert!(!player_ref.is_alive());
```

Using these `Entity` handles, you can freely reference you object in multiple places without
dealing smart pointers or leaky reference cycles.

```rust
use bort::{Entity, OwnedEntity};
use std::collections::HashMap;

struct PlayerId(usize);

struct Name(String);

#[derive(Default)]
struct PlayerRegistry {
    all: Vec<OwnedEntity>,
    by_name: HashMap<String, Entity>,
}

impl PlayerRegistry {
    pub fn add(&mut self, player: OwnedEntity) {
        let player_ref = player.entity();

        // Add to list of all entities
        player.insert(PlayerId(self.all.len()));
        self.all.push(player);

        // Add to map of entities by name
        self.by_name.insert(
            player_ref.get::<Name>().0.clone(),
            player_ref,
        );
    }
}

let (player, player_ref) = OwnedEntity::new()
    .with(Name("foo".to_string()))
    .split_guard();  // Splits the `OwnedEntity` into a tuple of `(OwnedEntity, Entity)`.

let mut registry = PlayerRegistry::default();
registry.add(player);

println!("Player is at index {}", player_ref.get::<PlayerId>().0);
```

See the reference documentation for [`Entity`] and [`OwnedEntity`] for a complete list of methods.

Features not discussed in this introductory section include:

- Liveness checking through [`Entity::is_alive()`]
- Storage handle caching through [`storage()`] for hot code sections
- Additional entity builder methods such as [`Entity::with_debug_label()`] and [`Entity::with_self_referential`]
- Debug utilities to [query entity statistics](debug)

#### Borrowing

Bort relies quite heavily on runtime borrowing. Although the type system does not prevent you
from violating "mutable xor immutable" rules for a given component, doing so will cause a panic
anyways:

```rust
use bort::OwnedEntity;

let foo = OwnedEntity::new()
    .with(vec![3i32]);

let vec1 = foo.get::<Vec<i32>>();  // Ok
let a = &vec1[0];

let mut vec2 = foo.get_mut::<Vec<i32>>();  // Panics at runtime!
vec2.push(4);
```

Components should strive to only borrow from their logical children. For example, it's somewhat
expected for a player to access the components of the items in its inventory but it's somewhat
unexpected to have that same player access the world state without that state being explicitly
passed to it. Maintaining this informal rule should make borrowing dependencies easier to reason
about.

Dispatching object behavior through "system" functions that iterate over and update entities of
a given type can be a wonderful way to encourage intuitive borrowing practices and can greatly
improve performance when compared to the regular dynamic dispatch way of doing things:

```rust
use bort::{Entity, storage};

fn process_players(players: impl IntoIterator<Item = Entity>) {
    let positions = storage::<Pos>();
    let velocities = storage::<Vel>();

    for player in players {
        positions.get_mut(player).0 += velocities.get(player).0;
    }
}
```

If runtime borrowing is still troublesome, the pub-in-private trick can help you define a component
that can only be borrowed from within trusted modules:

```rust
mod voxel {
    use bort::{OwnedEntity, Entity};
    use std::{cell::Ref, ops::Deref};

    #[derive(Default)]
    pub struct World {
        chunks: Vec<OwnedEntity>,
    }

    impl World {
        pub fn spawn_chunk(&mut self) -> Entity {
             let (chunk, chunk_ref) = OwnedEntity::new()
                .with_debug_label("chunk")
                .with(Chunk::default())
                .split_guard();

             self.chunks.push(chunk);
             chunk_ref
        }

        pub fn chunk_state(&self, chunk: Entity) -> ChunkRef {
            ChunkRef(chunk.get())
        }

        pub fn mutate_chunk(&mut self, chunk: Entity) {
            chunk.get_mut::<Chunk>().mutated = false;
        }
    }

    mod sealed {
        #[derive(Default)]
        pub struct Chunk {
            pub(super) mutated: bool,
        }
    }

    use sealed::Chunk;

    impl Chunk {
        pub fn do_something(&self) {
            println!("Mutated: {}", self.mutated);
        }
    }

    pub struct ChunkRef<'a>(Ref<'a, Chunk>);

    impl Deref for ChunkRef<'_> {
        type Target = Chunk;

        fn deref(&self) -> &Chunk {
            &self.0
        }
    }
}

use voxel::World;

let mut world = World::default();
let chunk = world.spawn_chunk();

let chunk_ref = world.chunk_state(chunk);
chunk_ref.do_something();

// `chunk_ref` is tied to the lifetime of `world`.
// Thus, there is no risk of accidentally leaving the guard alive.
drop(chunk_ref);

world.mutate_chunk(chunk);
world.chunk_state(chunk).do_something();

// No way to access the component directly.
// let chunk_ref = chunk.get::<voxel::Chunk>();
//                             ^ this is unnamable!
```

#### Threading

Most globals in Bort are thread local and therefore, most entity operations will not work on other
threads. Additionally, because Bort relies so much on allocating global memory pools for the
duration of the program to handle component allocations, using Bort on a non-main thread and then
destroying that thread would leak memory.

Because of these two hazards, only the first thread to call into Bort will be allowed to use its
thread-specific functionality. You can circumvent this restriction by explicitly calling
[`theading::bless()`](threading::bless) on the additional threads on which you wish to use Bort. Just know
that the caveats still apply.

<!-- cargo-rdme end -->
