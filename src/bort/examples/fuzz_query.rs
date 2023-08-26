use bort::{query, Entity, VirtualTag};
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
    let force_arch_tag = VirtualTag::new();

    let mut alive_set = FxHashSet::default();
    let mut alive_list = Vec::<Entity>::new();

    let _crash_guard = LogOnCrash;

    for op in 0.. {
        println!(
            "Successful after {op} op(s). Alive count: {}",
            alive_list.len()
        );

        for _ in 0..fastrand::u32(0..129) {
            if fastrand::bool() && !alive_list.is_empty() {
                let entity = alive_list.swap_remove(fastrand::usize(0..alive_list.len()));
                // println!("Destroyed {entity:?}");
                entity.destroy();
                assert!(alive_set.remove(&entity));
            } else {
                let entity = Entity::new_unmanaged().with_tag(force_arch_tag);
                assert!(alive_set.insert(entity));
                // println!("Spawned {entity:?}");
                alive_list.push(entity);
            }
        }

        for entity in &alive_list {
            assert!(entity.is_alive());
        }

        let mut queried = FxHashSet::default();
        query! {
            for (@me) + [force_arch_tag] {
                assert!(queried.insert(me));
            }
        }

        assert_eq!(&queried, &alive_set);
    }
}
