use bort::prelude::*;

fn main() {
    // Bort's primary building block is the entity. An entity represents any logical object in
    // your application, such as a player, an enemy, or something more abstract like a UI widget.
    // Entities, by themselves, are nothing but a fancy identifier. To make them useful, we have
    // to attach data in the form of components to them.

    struct Name(String);
    struct Age(u32);

    let player = OwnedEntity::new()
        .with(Name("Player the Played".to_string()))
        .with(Age(601));

    // Components borrows on entities are borrow-checked at runtime.
    player.get_mut::<Age>().0 += 1;
    println!(
        "Happy birthday! {} is now {} year(s) old!",
        player.get::<Name>().0,
        player.get::<Age>().0
    );

    // Entities in Bort have the full gamut of entity-component-system querying functionality. You
    // can tag entities with tags which are either virtual (i.e. have no associated component) or
    // managed (i.e. have an associated component) and iterate through all entities with a given tag.
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

    // In code covering just a single subsystem, these semantics are relatively easy to reason about.
    // However, as you start adding more systems, you may run into three big problems:
    //
    // 1. You may forget to run the logic of a given interested subsystem for a given event.
    // 2. You may borrow a component you have already borrowed somewhere else.
    // 3. The hierarchical structure of entities may be difficult to understand at a glance.
    //
    // We begin by addressing the first issue using two new features: events and the behavior registry.

    // Events provide a mechanism for deferring execution of a given handler until a more convenient
    // time.
    struct EnterHomeEvent;

    let mut on_enter_home = VecEventList::new();

    on_enter_home.fire(player.entity(), EnterHomeEvent);

    fn greeter_system(on_enter_home: &mut VecEventList<EnterHomeEvent>) {
        query! {
            for (_event in on_enter_home; ref name in GlobalTag::<Name>) {
                println!("A person with the name {} just entered my home!", name.0);
            }
        }
    }

    fn counter_system(
        on_enter_home: &mut VecEventList<EnterHomeEvent>,
        in_world: VirtualTag,
        tracker: &mut u32,
    ) {
        query! {
            for (_event in on_enter_home) + [in_world] {
                *tracker += 1;
            }
        }
    }

    greeter_system(&mut on_enter_home);

    let mut people_from_this_world = 0;
    counter_system(&mut on_enter_home, in_world, &mut people_from_this_world);

    // Note that we can process the same event list in the same system multiple times and only have
    // a given event be counted by that system once.
    counter_system(&mut on_enter_home, in_world, &mut people_from_this_world);

    println!("People who have entered my home who are from this world: {people_from_this_world}");

    // We just need to make sure to clear the event list once we're sure that everyone interested in
    // the event has received it. Otherwise, we get a warning in the console in debug builds telling
    // us that we may have leaked an event inadvertently.
    on_enter_home.clear();

    // The second system is the behavior registry. The behavior registry allows us to register
    // closures in a global registry and call all closures registered for a given task without having
    // to know the complete list ahead of time.

    // We begin by declaring a delegate to encapsulate our closure type.
    delegate! {
        fn HomeEnterBehavior(
            bhv: &BehaviorRegistry,
            on_enter_home: &mut VecEventList<EnterHomeEvent>,
            in_world: VirtualTag,
            people_from_this_world: &mut u32,
        )
        as deriving behavior
    }

    // Now, we can start to register some behaviors.
    let bhv = BehaviorRegistry::new()
        .with(HomeEnterBehavior::new(
            |_bhv, on_enter_home, _in_world, _people_from_this_world| {
                greeter_system(on_enter_home);
            },
        ))
        .with(HomeEnterBehavior::new(
            |_bhv, on_enter_home, in_world, people_from_this_world| {
                counter_system(on_enter_home, in_world, people_from_this_world);
            },
        ));

    // ...and dispatch them in the same way we had done before.
    on_enter_home.fire(player.entity(), EnterHomeEvent);
    bhv.get::<HomeEnterBehavior>()(
        &bhv,
        &mut on_enter_home,
        in_world,
        &mut people_from_this_world,
    );

    println!(
        "People who have now entered my home who are from this world: {}",
        people_from_this_world
    );

    // What's great about the behavior registry is that we can define the behavior handlers for a
    // given subsystem in one place, rather than scattering the calls to their handlers in several
    // files.

    // This is great but we still run the risk of accidentally borrowing something several times.
    // To help solve this problem, Bort offers an optional integration with another crate called
    // "saddle."

    // Saddle helps check the validity of dynamic dispatches in the behavior registry while still
    // leaving enough flexibility for subsystem components to borrow in the complex ways that they
    // require. Saddle accomplishes this goal as follows:

    // TODO
}
