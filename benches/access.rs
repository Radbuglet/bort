use std::time::Duration;

use bort::{storage, OwnedEntity};
use criterion::{criterion_main, Criterion};

fn access_tests() {
    let mut c = Criterion::default()
        .warm_up_time(Duration::from_millis(100))
        .measurement_time(Duration::from_millis(900))
        .configure_from_args();

    c.bench_function("entity.get", |c| {
        let entity = OwnedEntity::new().with(3i32);

        c.iter(|| {
            drop(entity.get::<i32>());
        })
    });

    c.bench_function("storage.get", |c| {
        let storage = storage::<i32>();

        let entity = OwnedEntity::new().with(3i32);

        c.iter(|| {
            drop(storage.get(entity.entity()));
        });
    });

    c.bench_function("storage.try_get.unwrap", |c| {
        let storage = storage::<i32>();

        let entity = OwnedEntity::new().with(3i32);

        c.iter(|| {
            drop(storage.try_get(entity.entity()).unwrap());
        });
    });

    c.bench_function("storage.has", |c| {
        let storage = storage::<i32>();
        let entity = OwnedEntity::new().with(3i32);

        c.iter(|| {
            storage.has(entity.entity());
        });
    });
}

criterion_main!(access_tests);
