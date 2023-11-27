use std::hint::black_box;

use autoken::{PotentialImmutableBorrow, PotentialMutableBorrow};
use bort::{
    core::{
        cell::{MultiOptRefCell, MultiRefCellIndex, OptRefCell},
        heap::Heap,
        token::{is_main_thread, MainThreadToken},
    },
    debug::{alive_entity_count, force_reset_database},
    flush, query, storage, Entity, OwnedEntity, OwnedObj, Tag,
};
use criterion::{criterion_main, Criterion};

#[derive(Clone)]
struct Position(f32);

#[derive(Clone)]
struct Velocity(f32);

fn access_tests() {
    let mut c = Criterion::default().configure_from_args();

    MainThreadToken::acquire();

    c.bench_function("thread.is_mt", |c| {
        c.iter(is_main_thread);
    });

    c.bench_function("thread.acquire", |c| {
        c.iter(MainThreadToken::acquire);
    });

    c.bench_function("thread.acquire4", |c| {
        c.iter(|| {
            MainThreadToken::acquire();
            MainThreadToken::acquire();
            MainThreadToken::acquire();
            MainThreadToken::acquire();
        });
    });

    c.bench_function("spawn.empty", |c| {
        c.iter(Entity::new_unmanaged);
        force_reset_database();
        assert_eq!(alive_entity_count(), 0);
    });

    c.bench_function("spawn.with", |c| {
        c.iter_with_large_drop(|| OwnedEntity::new().with(Position(0.0)).with(Velocity(0.0)));
    });

    c.bench_function("spawn.storages", |c| {
        let pos = storage::<Position>();
        let vel = storage::<Velocity>();

        c.iter_with_large_drop(|| {
            let entity = OwnedEntity::new();
            pos.insert(entity.entity(), Position(0.0));
            vel.insert(entity.entity(), Velocity(0.0));
            entity
        });
    });

    c.bench_function("get.entity.normal.ref", |c| {
        let _pop = spawn_anon_pos_pop();
        let obj = OwnedEntity::new().with(Position(1.0));

        c.iter(|| obj.get::<Position>());
    });

    c.bench_function("get.entity.normal.mut", |c| {
        let _pop = spawn_anon_pos_pop();
        let obj = OwnedEntity::new().with(Position(1.0));

        c.iter(|| obj.get_mut::<Position>());
    });

    c.bench_function("get.entity.storage.ref", |c| {
        let _pop = spawn_anon_pos_pop();
        let obj = OwnedEntity::new().with(Position(1.0));
        let storage = storage::<Position>();

        c.iter(|| storage.get(obj.entity()));
    });

    c.bench_function("get.entity.storage.mut", |c| {
        let _pop = spawn_anon_pos_pop();
        let obj = OwnedEntity::new().with(Position(1.0));
        let storage = storage::<Position>();

        c.iter(|| storage.get_mut(obj.entity()));
    });

    c.bench_function("get.obj.normal.ref", |c| {
        let _pop = spawn_anon_pos_pop();
        let obj = OwnedObj::new(Position(1.0));

        c.iter(|| obj.get());
    });

    c.bench_function("get.obj.normal.mut", |c| {
        let _pop = spawn_anon_pos_pop();
        let obj = OwnedObj::new(Position(1.0));

        c.iter(|| obj.get_mut());
    });

    c.bench_function("get.obj.may_aba.ref", |c| {
        let _pop = spawn_anon_pos_pop();
        let obj = OwnedObj::new(Position(1.0));

        c.iter(|| obj.get_maybe_aba());
    });

    c.bench_function("get.obj.may_aba.mut", |c| {
        let _pop = spawn_anon_pos_pop();
        let obj = OwnedObj::new(Position(1.0));

        c.iter(|| obj.get_mut_maybe_aba());
    });

    c.bench_function("get.slot.re_token.ref", |c| {
        let _pop = spawn_anon_pos_pop();
        let obj = OwnedObj::new(Position(1.0));
        let slot = obj.value();

        c.iter(|| slot.borrow(MainThreadToken::acquire()));
    });

    c.bench_function("get.slot.re_token.mut", |c| {
        let _pop = spawn_anon_pos_pop();
        let obj = OwnedObj::new(Position(1.0));
        let slot = obj.value();

        c.iter(|| slot.borrow_mut(MainThreadToken::acquire()));
    });

    c.bench_function("get.slot.store_token.ref", |c| {
        let _pop = spawn_anon_pos_pop();
        let obj = OwnedObj::new(Position(1.0));
        let slot = obj.value();
        let token = MainThreadToken::acquire();

        c.iter(|| slot.borrow(token));
    });

    c.bench_function("get.slot.store_token.mut", |c| {
        let _pop = spawn_anon_pos_pop();
        let obj = OwnedObj::new(Position(1.0));
        let slot = obj.value();
        let token = MainThreadToken::acquire();

        c.iter(|| slot.borrow_mut(token));
    });

    c.bench_function("get.slot.direct.ref", |c| {
        let _pop = spawn_anon_pos_pop();
        let obj = OwnedObj::new(Position(1.0));

        let token = MainThreadToken::acquire();
        let slot = unsafe { obj.value().direct_slot(token) };

        c.iter(|| slot.borrow(token));
    });

    c.bench_function("get.slot.direct.mut", |c| {
        let _pop = spawn_anon_pos_pop();
        let obj = OwnedObj::new(Position(1.0));

        let token = MainThreadToken::acquire();
        let slot = unsafe { obj.value().direct_slot(token) };

        c.iter(|| slot.borrow_mut(token));
    });

    c.bench_function("query.slots.only_slots", |c| {
        let pos_tag = Tag::new();
        let vel_tag = Tag::new();
        let entities = spawn_tagged_pos_vel_pop(pos_tag, vel_tag);
        flush();

        c.iter(|| {
            query! {
                for (slot pos in pos_tag, slot vel in vel_tag) {
                    black_box((pos, vel));
                }
            }
        });

        drop(entities);
        flush();
    });

    c.bench_function("query.slots.manual_borrow", |c| {
        let pos_tag = Tag::new();
        let vel_tag = Tag::new();
        let entities = spawn_tagged_pos_vel_pop(pos_tag, vel_tag);
        let token = MainThreadToken::acquire();
        flush();

        c.iter(|| {
            query! {
                for (slot pos in pos_tag, slot vel in vel_tag) {
                    pos.borrow_mut(token).0 += vel.borrow(token).0;
                }
            }
        });

        drop(entities);
        flush();
    });

    c.bench_function("query.normal.no_entity", |c| {
        let pos_tag = Tag::new();
        let vel_tag = Tag::new();
        let entities = spawn_tagged_pos_vel_pop(pos_tag, vel_tag);
        flush();

        c.iter(|| {
            query! {
                for (mut pos in pos_tag, ref vel in vel_tag) {
                    pos.0 += vel.0;
                }
            }
        });

        drop(entities);
        flush();
    });

    c.bench_function("query.manual.no_entity", |c| {
        let pos_tag = Tag::new();
        let vel_tag = Tag::new();
        let entities = spawn_tagged_pos_vel_pop(pos_tag, vel_tag);
        flush();

        c.iter(|| '__query: {
            use ::bort::query::query_internals;

            // Evaluate our tag expressions
            let pos = query_internals::get_tag(pos_tag);
            let vel = query_internals::get_tag(vel_tag);

            // Determine tag list
            let virtual_tags_dyn = Vec::<query_internals::InertTag>::new();
            let virtual_tags_static = [Some(pos.1), Some(vel.1)];

            // Acquire the main thread token used for our query
            let token = query_internals::MainThreadToken::acquire_fmt("query entities");

            // Acquire the database
            let mut db = query_internals::DbRoot::get(token);

            // Collect the necessary storages and tags
            let pos = query_internals::get_storage(&mut db, token, pos);
            let vel = query_internals::get_storage(&mut db, token, vel);

            // Acquire a chunk iterator
            let chunks = db.prepare_anonymous_entity_query(query_internals::ReifiedTagList {
                static_tags: &virtual_tags_static,
                dynamic_tags: &virtual_tags_dyn,
            });

            // Acquire a query guard to prevent flushing
            let _guard = db.borrow_query_guard(token);

            // Drop the database to allow safe userland code involving Bort to run
            drop(db);

            // For each chunk...
            for chunk in chunks {
                // Collect the heaps for each storage
                let pos = chunk.heaps(&pos.borrow(token)).into_iter();
                let vel = chunk.heaps(&vel.borrow(token)).into_iter();

                // Handle all our heaps
                let mut i = chunk.heap_count();

                // N.B. the following while loop's pattern may be empty if no components
                // are being borrowed or entities are being read, which could cause an
                // underflow when `i` is subtracted. We guard against that scenario here.
                if i == 0 {
                    continue;
                }

                for (pos, vel) in pos.zip(vel) {
                    // Determine whether we're the last heap of the chunk
                    i -= 1;
                    let is_last = i == 0;

                    // Determine the actual length of our iterator
                    #[allow(unused_variables)]
                    let len = pos.len();
                    let len = vel.len();

                    // Construct iterators
                    let pos = pos.values_and_slots(token).take(if is_last {
                        MultiRefCellIndex::blocks_needed(chunk.last_heap_len())
                    } else {
                        pos.len()
                    });

                    let vel = vel.values_and_slots(token).take(if is_last {
                        MultiRefCellIndex::blocks_needed(chunk.last_heap_len())
                    } else {
                        vel.len()
                    });

                    // Iterate through every group in this heap
                    '__query_ent: for (chunk_index, (pos, vel)) in pos.zip(vel).enumerate() {
                        // Convert the residuals to their target form
                        let mut loaner = PotentialMutableBorrow::new();
                        let pos = (pos.values().try_borrow_all_mut(token, &mut loaner), pos);

                        let loaner = PotentialImmutableBorrow::new();
                        let vel = (vel.values().try_borrow_all(token, &loaner), vel);

                        if let (Some(mut pos), Some(vel)) =
                            (pos.0.map(|v| (v, pos.1)), vel.0.map(|v| (v, vel.1)))
                        {
                            for (pos, vel) in (pos.0.iter_mut().zip(pos.1.slots()))
                                .zip(vel.0.iter().zip(vel.1.slots()))
                            {
                                (pos.0).0 += (vel.0).0;
                            }
                        } else {
                            for i in MultiRefCellIndex::iter() {
                                if i as usize + chunk_index * MultiRefCellIndex::COUNT >= len {
                                    break;
                                }

                                let pos = &mut *pos.1.values().borrow_mut(token, i);
                                let vel = &*vel.1.values().borrow(token, i);

                                pos.0 += vel.0;
                            }
                        };
                    }
                }
            }
        });

        drop(entities);
        flush();
    });

    c.bench_function("query.normal.with_entity", |c| {
        let pos_tag = Tag::new();
        let vel_tag = Tag::new();
        let entities = spawn_tagged_pos_vel_pop(pos_tag, vel_tag);
        flush();

        c.iter(|| {
            query! {
                for (@_me, mut pos in pos_tag, ref vel in vel_tag) {
                    pos.0 += vel.0;
                }
            }
        });

        drop(entities);
        flush();
    });

    c.bench_function("query.heap.full", |c| {
        let token = MainThreadToken::acquire();

        let pos_heap = Heap::new(token, 100_000);
        for slot in pos_heap.slots(token) {
            slot.set_value(token, Some(Position(1.)));
        }

        let vel_heap = Heap::new(token, 100_000);
        for slot in vel_heap.slots(token) {
            slot.set_value(token, Some(Velocity(1.)));
        }

        c.iter(|| {
            let mut iter_pos = pos_heap.values().iter();
            let mut iter_vel = vel_heap.values().iter();

            while let (Some(pos_group), Some(vel_group)) = (iter_pos.next(), iter_vel.next()) {
                let mut loaner = PotentialMutableBorrow::new();
                let pos_group = pos_group.try_borrow_all_mut(token, &mut loaner);

                let loaner = PotentialImmutableBorrow::new();
                let vel_group = vel_group.try_borrow_all(token, &loaner);

                if let (Some(mut pos_group), Some(vel_group)) = (pos_group, vel_group) {
                    for (pos, vel) in pos_group.iter_mut().zip(vel_group.iter()) {
                        pos.0 += vel.0;
                    }
                };
            }
        })
    });

    c.bench_function("query.heap.single.group", |c| {
        let token = MainThreadToken::acquire();

        let pos_heap = Heap::new(token, 100_000);
        for slot in pos_heap.slots(token) {
            slot.set_value(token, Some(Position(1.)));
        }

        c.iter(|| {
            for group in pos_heap.values() {
                let mut loaner = PotentialMutableBorrow::new();
                let Some(mut group) = group.try_borrow_all_mut(token, &mut loaner) else {
                    continue;
                };
                for slot in &mut *group {
                    slot.0 += 1.;
                }
            }
        })
    });

    c.bench_function("query.heap.single.individual", |c| {
        let token = MainThreadToken::acquire();

        let pos_heap = Heap::new(token, 100_000);
        for slot in pos_heap.slots(token) {
            slot.set_value(token, Some(Position(1.)));
        }

        c.iter(|| {
            for group in pos_heap.values() {
                for slot in MultiRefCellIndex::iter() {
                    group.borrow_mut(token, slot).0 += 1.;
                }
            }
        })
    });

    c.bench_function("refcell.single.ref", |c| {
        let cell = OptRefCell::new_full(3);

        c.iter(|| cell.borrow());
    });

    c.bench_function("refcell.single.mut", |c| {
        let cell = OptRefCell::new_full(3);

        c.iter(|| cell.borrow_mut());
    });

    c.bench_function("refcell.multi.single.ref.static", |c| {
        let mut cell = MultiOptRefCell::new();
        cell.set(MultiRefCellIndex::Slot3, Some(3));

        c.iter(|| cell.borrow(MultiRefCellIndex::Slot3));
    });

    c.bench_function("refcell.multi.single.ref.bb", |c| {
        let mut cell = MultiOptRefCell::new();
        cell.set(MultiRefCellIndex::Slot3, Some(3));

        c.iter(|| cell.borrow(black_box(MultiRefCellIndex::Slot3)));
    });

    c.bench_function("refcell.multi.single.mut.static", |c| {
        let mut cell = MultiOptRefCell::new();
        cell.set(MultiRefCellIndex::Slot3, Some(3));

        c.iter(|| cell.borrow_mut(MultiRefCellIndex::Slot3));
    });

    c.bench_function("refcell.multi.single.mut.bb", |c| {
        let mut cell = MultiOptRefCell::new();
        cell.set(MultiRefCellIndex::Slot3, Some(3));

        c.iter(|| cell.borrow_mut(black_box(MultiRefCellIndex::Slot3)));
    });

    c.bench_function("refcell.multi.multi.ref", |c| {
        let mut cell = MultiOptRefCell::new();
        for slot in MultiRefCellIndex::iter() {
            cell.set(slot, Some(slot as u32));
        }

        c.iter(|| {
            let loaner = PotentialImmutableBorrow::new();
            cell.try_borrow_all(&loaner);
        });
    });

    c.bench_function("refcell.multi.multi.mut", |c| {
        let mut cell = MultiOptRefCell::new();
        for slot in MultiRefCellIndex::iter() {
            cell.set(slot, Some(slot as u32));
        }

        c.iter(|| {
            let mut loaner = PotentialMutableBorrow::new();
            cell.try_borrow_all_mut(&mut loaner);
        });
    });
}

criterion_main!(access_tests);

fn spawn_anon_pos_pop() -> Vec<OwnedEntity> {
    (0..10_000)
        .map(|i| OwnedEntity::new().with(Position(i as f32)))
        .collect()
}

fn spawn_tagged_pos_vel_pop(pos_tag: Tag<Position>, vel_tag: Tag<Velocity>) -> Vec<OwnedEntity> {
    (0..100_000)
        .map(|i| {
            OwnedEntity::new()
                .with_tagged(pos_tag, Position(i as f32))
                .with_tagged(vel_tag, Velocity(i as f32))
        })
        .collect()
}
