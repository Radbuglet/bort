use std::{cell::RefCell, time::Duration};

use bort::{
    core::{cell::OptRefCell, heap::Heap, token::MainThreadToken},
    storage, OwnedEntity, OwnedObj,
};
use criterion::{criterion_main, Criterion};
use glam::Vec3;

fn access_tests() {
    let mut c = Criterion::default()
        .warm_up_time(Duration::from_millis(100))
        .measurement_time(Duration::from_millis(900))
        .configure_from_args();

    c.bench_function("storage", |c| {
        c.iter(|| storage::<i32>());
    });

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

    c.bench_function("obj.get", |c| {
        let foo = OwnedObj::new(());

        c.iter(|| foo.get());
    });

    c.bench_function("opt-ref-cell", |c| {
        let foo = OptRefCell::new_full(3);

        c.iter(|| foo.borrow());
    });

    c.bench_function("heap-access", |c| {
        let token = MainThreadToken::acquire();
        let positions = Heap::new(10_000);
        let velocities = Heap::new(10_000);

        let dummy = OwnedEntity::new();

        for i in 0..positions.len() {
            positions.slot(i).set_value_owner_pair(
                token,
                Some((
                    dummy.entity(),
                    Vec3::new(fastrand::f32(), fastrand::f32(), fastrand::f32()),
                )),
            );

            velocities.slot(i).set_value_owner_pair(
                token,
                Some((
                    dummy.entity(),
                    Vec3::new(fastrand::f32(), fastrand::f32(), fastrand::f32()),
                )),
            );
        }

        c.iter(|| {
            for (pos, vel) in positions.slots().zip(velocities.slots()) {
                *pos.borrow_mut(token) += *vel.borrow(token);
            }
        });

        positions.clear_slots(token);
        velocities.clear_slots(token);
    });

    c.bench_function("std-ref-cell", |c| {
        let foo = RefCell::new(3);

        c.iter(|| foo.borrow());
    });
}

criterion_main!(access_tests);
