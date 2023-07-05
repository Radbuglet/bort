use std::{cell::RefCell, time::Duration};

use bort::{
    core::{cell::OptRefCell, heap::Heap, token::MainThreadToken},
    flush,
    query::query_one,
    query_all_anon, storage, OwnedEntity, OwnedObj, Tag,
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

    c.bench_function("query-manual", |c| {
        let token = MainThreadToken::acquire();
        let dummy = OwnedEntity::new();

        let positions = Heap::new(token, 10_000);
        let velocities = Heap::new(token, 10_000);

        for (pos, vel) in positions.slots(token).zip(velocities.slots(token)) {
            pos.set_value_owner_pair(
                token,
                Some((
                    dummy.entity(),
                    Vec3::new(fastrand::f32(), fastrand::f32(), fastrand::f32()),
                )),
            );

            vel.set_value_owner_pair(
                token,
                Some((
                    dummy.entity(),
                    Vec3::new(fastrand::f32(), fastrand::f32(), fastrand::f32()),
                )),
            );
        }

        c.iter(|| {
            for pos in positions.slots(token) {
                *pos.borrow_mut(token) += Vec3::Y;
            }
        });
    });

    c.bench_function("query-auto", |c| {
        struct Pos(Vec3);
        struct Vel(Vec3);

        let pos_tag = Tag::<Pos>::new();
        let vel_tag = Tag::<Vel>::new();

        let _entities = (0..10_000)
            .map(|_| {
                let entity = OwnedEntity::new();
                entity.tag(pos_tag);
                entity.tag(vel_tag);
                entity.insert(Pos(Vec3::new(
                    fastrand::f32(),
                    fastrand::f32(),
                    fastrand::f32(),
                )));
                entity.insert(Vel(Vec3::new(
                    fastrand::f32(),
                    fastrand::f32(),
                    fastrand::f32(),
                )));
                entity
            })
            .collect::<Vec<_>>();

        flush();

        c.iter(|| {
            for mut pos in query_all_anon(pos_tag.as_mut()) {
                pos.0 += Vec3::Y;
            }
        });
    });

    c.bench_function("query-simplified", |c| {
        struct Pos(Vec3);
        struct Vel(Vec3);

        let pos_tag = Tag::<Pos>::new();
        let vel_tag = Tag::<Vel>::new();

        let _entities = (0..10_000)
            .map(|_| {
                let entity = OwnedEntity::new();
                entity.tag(pos_tag);
                entity.tag(vel_tag);
                entity.insert(Pos(Vec3::new(
                    fastrand::f32(),
                    fastrand::f32(),
                    fastrand::f32(),
                )));
                entity.insert(Vel(Vec3::new(
                    fastrand::f32(),
                    fastrand::f32(),
                    fastrand::f32(),
                )));
                entity
            })
            .collect::<Vec<_>>();

        flush();

        c.iter(|| {
            query_one(pos_tag, |pos| {
                pos.0 += Vec3::Y;
            });
        });
    });

    c.bench_function("std-ref-cell", |c| {
        let foo = RefCell::new(3);

        c.iter(|| foo.borrow());
    });
}

criterion_main!(access_tests);
