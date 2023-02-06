use std::time::Duration;

use criterion::{criterion_main, Criterion};
use geode::{storage, Entity};

fn access_tests() {
    let mut c = Criterion::default()
        .warm_up_time(Duration::from_millis(100))
        .measurement_time(Duration::from_millis(900))
        .configure_from_args();

    c.bench_function("get method", |c| {
        let entity = Entity::new().with(3i32);

        c.iter(|| {
            drop(entity.get::<i32>());
        })
    });

    c.bench_function("get storage", |c| {
        let storage = storage::<i32>();
        let entity = Entity::new().with(3i32);

        c.iter(|| {
            drop(storage.get(entity.entity()));
        })
    });
}

criterion_main!(access_tests);
