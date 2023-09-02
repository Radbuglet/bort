# Bort

A Simple Object-Model for the Rust Programming Language

## Basic Usage

Bort's primary building block is the entity. An entity represents any logical object in your application, such as a player, an enemy, or something more abstract like a UI widget.

We can spawn one like so:

```rust
use bort::OwnedEntity;

let player = OwnedEntity::new();
```

> **Note:** We create an `OwnedEntity` instead of a regular `Entity` (which is also a thing in Bort) because owned entities are unique and destroy themselves upon being dropped while entities are freely copyable but do not automatically destroy themselves.
>
> You can get a copyable `Entity` reference to an `OwnedEntity` by calling the `.entity()` method.

Entities, by themselves, are nothing but a fancy identifier number. To make them useful, we have to attach data in the form of components to them.

```rust
struct Name(String);
struct Age(u32);

player.insert(Name("Player the Played".to_string()));
player.insert(Age(601));
```

It is also possible to use builder notation to construct such entities more conveniently:

```rust
let player = OwnedEntity::new()
    .with(Name("Player the Played".to_string()))
    .with(Age(601));
```

Users can then borrow data from entities using the `get` and `get_mut` methods. These metods use runtime borrow checks to ensure that you aren't breaking Rust's [borrowing rules]().

```rust
player.get_mut::<Age>().0 += 1;
println!(
    "Happy birthday! {} is now {} year(s) old!",
    player.get::<Name>().0,
    player.get::<Age>().0
);
```

Entities in Bort have the full gamut of entity-component-system querying functionality.

You can tag entities with tags which are either virtual (i.e. have no associated component) or managed (i.e. have an associated component) and iterate through all entities with a given tag.

```rust
use bort::{HasGlobalManagedTag, GlobalTag, VirtualTag, query};

let in_world = VirtualTag::new();
player.tag(in_world);

impl HasGlobalManagedTag for Name {
    type Component = Self;
}

player.tag(GlobalTag::<Name>);

query! {
    for (@me, mut name in GlobalTag::<Name>) + [in_world] {
        println!("{me:?} is in the world and has the name {}.", name.0);
    }
}
```

##### Further Reading

- The `Entity` or `OwnedEntity` API references, which enumerate the available methods on entities, including:
  - additional builder syntax for constructing entities
  - additional methods for manipulating and querying entities
  - additional conversions between various representations of an entity
- The section on entity semantics, which describes:
  - component borrowing semantics
  - details on how entities are deleted
  - how entities are queried
- The `Obj` and `OwnedObj` API reference, which describes objects entity references, which give you...
  - mechanisms for statically typing entities by the component they contain
  - mechanisms for fetching that component incredibly efficiently
  - the special semantics regarding `Obj`s and component removal
- The `CompRef` and `CompMut` API references, which enumerate the available methods on temporary references to components, including:
  -  mechanisms for determining the owning entity of a given component reference
- The `query!` macro documentation, which describes the specific syntax for querying objects, including:
  - syntax for getting `CompRef`s and `CompMut`s directly instead of references and mutable references
  - detailed semantics for nested queries and concurrent modification
- The `tag` module documentation, which describes the various ways to define tags, including:
  - the difference between managed tags, virtual tags, raw tags, and not tagging something at all
  - creating managed and virtual tags dynamically
  - creating global managed and virtual tags statically
  - the various aliases for all these actions

## Behaviors and Delegates

==TODO==

## Saddle Validation

==TODO==

## Multithreading

==TODO==

## Suggested Organization

==TODO==
