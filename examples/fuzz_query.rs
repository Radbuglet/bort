use bort::{core::token::MainThreadToken, flush, query, Entity, Tag};
use rustc_hash::FxHashSet;

fn main() {
    struct LogOnCrash;

    impl Drop for LogOnCrash {
        fn drop(&mut self) {
            if std::thread::panicking() {
                println!("=== Database crash dump ===");
                println!("{}", bort::debug::dump_database_state());
            }
        }
    }

    fastrand::seed(18032657114263383921);
    println!("Seed: {}", fastrand::get_seed());

    let token = MainThreadToken::acquire();
    let my_tag = Tag::<i32>::new();
    let my_tag_2 = Tag::<u32>::new();

    let mut alive_set = FxHashSet::default();
    let mut alive_and_tagged_2_set = FxHashSet::default();
    let mut alive_list = Vec::<Entity>::new();

    let _crash_guard = LogOnCrash;

    for op in 0.. {
        println!(
            "Successful after {op} op(s). Alive count: {}",
            alive_list.len()
        );

        for _ in 0..fastrand::u32(0..129) {
            let choice = fastrand::u32(0..3);

            if choice == 0 && !alive_list.is_empty() {
                let entity = alive_list.swap_remove(fastrand::usize(0..alive_list.len()));
                // println!("Destroyed {entity:?}");
                assert!(alive_set.remove(&entity));
                assert_eq!(
                    entity.is_tagged(my_tag_2),
                    alive_and_tagged_2_set.remove(&entity)
                );

                entity.destroy();
            } else if choice == 1 && !alive_list.is_empty() {
                let target = alive_list[fastrand::usize(0..alive_list.len())];
                if fastrand::bool() {
                    target.tag(my_tag_2);
                    alive_and_tagged_2_set.insert(target);
                } else {
                    target.untag(my_tag_2);
                    alive_and_tagged_2_set.remove(&target);
                }
            } else {
                let entity = Entity::new_unmanaged().with_tagged(my_tag, 3);
                assert!(alive_set.insert(entity));
                // println!("Spawned {entity:?}");
                alive_list.push(entity);
            }
        }

        flush();

        // Validate liveness states
        for entity in &alive_list {
            assert!(entity.is_alive());
        }

        // Validate full list
        {
            let mut queried = FxHashSet::default();
            query! {
                for (@me, slot value in my_tag) {
                    let owner = value.owner(token);
                    assert_eq!(owner, Some(me), "index: {}", queried.len());
                    assert_eq!(*me.get::<i32>(), 3);
                    assert_eq!(*value.borrow(token), 3);
                    assert!(queried.insert(me));
                }
            }

            assert_eq!(&queried, &alive_set);
        }

        // Validate entity-less query
        query! {
            for (slot _value in my_tag) {}
        }

        // Validate partial list
        {
            let mut queried = FxHashSet::default();
            query! {
                for (@me) + [my_tag_2] {
                    assert!(queried.insert(me));
                }
            }

            assert_eq!(&queried, &alive_and_tagged_2_set);
        }
    }
}
