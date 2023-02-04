use std::{cell::RefCell, time::Duration};

use atomic_refcell::AtomicRefCell;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use mini_geode::cell::{Namespace, SyncRefCell};

fn bench(c: &mut Criterion) {
    c.bench_function("ref cell", |c| {
        let cell = RefCell::new(3);

        c.iter(|| drop(black_box(&cell).borrow_mut()));
    });

    c.bench_function("atomic ref cell", |c| {
        let cell = AtomicRefCell::new(3);

        c.iter(|| black_box(&cell).borrow_mut());
    });

    c.bench_function("sync ref cell", |c| {
        let namespace = Namespace::new();
        let _guard = namespace.acquire();

        let cell = SyncRefCell::new_in(namespace, 3);

        c.iter(|| drop(black_box(&cell).borrow_mut()));
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_millis(100))
        .measurement_time(Duration::from_millis(1000));
    targets = bench,
}

criterion_main!(benches);
