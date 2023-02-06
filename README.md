# Geode

A Simple Object Model for Rust

```rust
use geode::Entity;

let player = Entity::new()
    .with(Pos(Vec3::ZERO))
    .with(Vel(Vec3::ZERO))
    .with(PlayerState { ... });

let chaser = Entity::new()
    .with(Pos(Vec3::ZERO))
    .with(ChaserAi { target: Some(player), home: Vec3::ZERO });

player.get_mut::<PlayerState>().update(player.handle());

let pos = &mut *chaser.get_mut::<Pos>();
let state = chaser.get::<ChaserState>();

pos.0 = pos.0.lerp(match state.target {
    Some(target) => target.get::<Pos>().0,
    None => state.home,
}, 0.2);
```

## Getting Started

Applications in Geode are made of `Entity` instances. These are `Copy`able references to logical objects in your application. They can represent anything from a player character in a game to a UI widget.

To create one, just call `Entity::new()`.

```rust
use geode::Entity;

let player = Entity::new();
```

From there, you can add components to it using either `insert` or `with`:

```rust
player.insert(Pos(Vec3::ZERO));
player.insert(Vel(Vec3::ZERO));

// ...which is equivalent to:
let player = Entity::new()
    .with(Pos(Vec3::ZERO))
    .with(Vel(Vec3::ZERO));
```

...and access them using `get` or `get_mut`:

```rust
let mut pos = player.get_mut::<Pos>();  // These are `RefMut`s
let vel = player.get::<Vel>();          // ...and `Ref`s.

pos.0 += vel.0;
```

Note that `Entity::new()` doesn't actually return an `Entity`, but rather an `OwnedEntity`. These expose the exact same interface as an `Entity` but have an additional `Drop` handler (making them non-`Copy`) that automatically `Entity::destroy`s their managed entity when they leave the scope.

You can extract an `Entity` from them using `OwnedEntity::entity(&self)`.

```rust
let player2 = player;  // (transfers ownership of `OwnedEntity`)
// player.insert("hello!");
// ^ `player` is invalid here; use `player2` instead!

let player_ref = player.entity();  // (produces a copyable `Entity` reference)
let player_ref_2 = player_ref;  // (copies the value)

assert_eq(player_ref, player_ref_2);
assert(player_ref.is_alive());

drop(player2);  // (dropped `OwnedEntity`; `player_xx` are all dead now)

assert!(!player_ref.is_alive());
```

Using these `Entity` handles, you can freely reference you object in multiple places without dealing with cloning smart pointers.

```rust
use geode::Entity;

struct PlayerId(usize);

struct Name(String);

#[derive(Default)]
struct PlayerRegistry {
    all: Vec<Entity>,
    by_name: HashMap<String, Entity>,
}

impl PlayerRegistry {
    pub fn add(&mut self, player: Entity) {
        player.insert(PlayerId(self.all.len()));
        self.all.push(player);
        self.by_name.insert(
            player.get::<Name>().0.clone(),
            player,
        );
    }
}

let player = Entity::new()
    .with(Name("foo".to_string()));

let mut registry = PlayerRegistry::default();
registry.add(player);

println!("Player is at index {}", player.get::<PlayerId>().0);
```

See the reference documentation for `Entity` and `OwnedEntity` for a complete list of methods.

### A Note on Borrowing

Geode relies quite heavily on runtime borrowing. Although the type system does not prevent you from mutably borrowing the same component more than once, doing so will cause a panic anyways:

```rust
use geode::Entity;

let foo = Entity::new()
    .with(vec![3i32]);

let vec1 = foo.get::<Vec<i32>>();  // Ok
let a = &vec1[0];

let mut vec2 = foo.get_mut::<Vec<i32>>();  // Panics at runtime!
vec2.push(4);
```

Components should strive to only borrow from their logical children. For example, it's somewhat expected for a player to access the components of the items in its inventory but it's somewhat unexpected to have that same player access the world state without that state being explicitly passed to it. Maintaining this informal rule should make borrowing dependencies easier to reason about.

### A Note on Threading

Currently, Geode is entirely single-threaded. Because everything is stored in thread-local storage, entities spawned on one thread will appear dead on another.
