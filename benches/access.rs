use std::hint::black_box;

use autoken::{PotentialImmutableBorrow, PotentialMutableBorrow};
use bort::{
    core::{
        cell::{MultiOptRefCell, MultiRefCellIndex, OptRefCell},
        heap::{Heap, Slot},
        token::{is_main_thread, MainThreadToken},
    },
    debug::{alive_entity_count, force_reset_database},
    flush, query, storage, Entity, Obj, OwnedEntity, OwnedObj, Tag, VecEventList,
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
        c.iter(|| {
            Entity::new_unmanaged()
                .with(Position(0.0))
                .with(Velocity(0.0))
        });
        force_reset_database();
        assert_eq!(alive_entity_count(), 0);
    });

    c.bench_function("spawn.storages", |c| {
        let pos = storage::<Position>();
        let vel = storage::<Velocity>();

        c.iter(|| {
            let entity = Entity::new_unmanaged();
            pos.insert(entity, Position(0.0));
            vel.insert(entity, Velocity(0.0));
            entity
        });
        force_reset_database();
        assert_eq!(alive_entity_count(), 0);
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

    c.bench_function("query.normal.overhead.global", |c| {
        let tag_1 = Tag::<i32>::new();
        let tag_2 = Tag::<u32>::new();
        let tag_3 = Tag::<f32>::new();

        c.iter(|| {
            query!(for (ref _foo in tag_1, mut _bar in tag_2, obj _baz in tag_3) {});
        });
    });

    c.bench_function("query.normal.overhead.events", |c| {
        let tag_1 = Tag::<i32>::new();
        let tag_2 = Tag::<u32>::new();
        let tag_3 = Tag::<f32>::new();

		let events = VecEventList::<u32>::default();

        c.iter(|| {
            query!(for (event _ in events, ref _foo in tag_1, mut _bar in tag_2, obj _baz in tag_3) {});
        });
    });

    c.bench_function("query.normal.only_slots.no_bb", |c| {
        let pos_tag = Tag::new();
        let vel_tag = Tag::new();
        let entities = spawn_tagged_pos_vel_pop(pos_tag, vel_tag);
        flush();

        c.iter(|| {
            query! {
                for (slot _pos in pos_tag, slot _vel in vel_tag) {}
            }
        });

        drop(entities);
        flush();
    });

    c.bench_function("query.normal.only_slots.bb", |c| {
        let pos_tag = Tag::new();
        let vel_tag = Tag::new();
        let entities = spawn_tagged_pos_vel_pop(pos_tag, vel_tag);
        flush();

        c.iter(|| {
            query! {
                for (slot _pos in pos_tag, slot _vel in vel_tag) {
                    black_box(());
                }
            }
        });

        drop(entities);
        flush();
    });

    c.bench_function("query.normal.one_component", |c| {
        let pos_tag = Tag::new();
        let vel_tag = Tag::new();
        let entities = spawn_tagged_pos_vel_pop(pos_tag, vel_tag);
        flush();

        c.iter(|| {
            query! {
                for (mut pos in pos_tag) {
                    pos.0 += 1.0;
                }
            }
        });

        drop(entities);
        flush();
    });

    c.bench_function("query.normal.no_entity.normal", |c| {
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

    c.bench_function("query.normal.no_entity.with_slot", |c| {
        let pos_tag = Tag::new();
        let vel_tag = Tag::new();
        let entities = spawn_tagged_pos_vel_pop(pos_tag, vel_tag);
        flush();

        c.iter(|| {
            query! {
                for (mut pos in pos_tag, ref vel in vel_tag, slot _vel_slot in vel_tag) {
                    pos.0 += vel.0;
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
                for (entity _me, mut pos in pos_tag, ref vel in vel_tag) {
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

    c.bench_function("linked_list.regular", |c| {
        let mut items = Vec::new();

        struct Item {
            next: usize,
            value: u64,
        }

        for i in 0..100_000 {
            if i != 100_000 - 2 {
                items.push(Item {
                    next: i + 1,
                    value: 100 + i as u64,
                });
            } else {
                items.push(Item {
                    next: usize::MAX,
                    value: 2,
                });
            }
        }

        c.iter(|| {
            let mut cursor = 0;
            let mut accum = 0;

            while cursor < items.len() {
                accum += items[cursor].value;
                cursor = items[cursor].next;
            }

            accum
        });
    });

    c.bench_function("linked_list.entity", |c| {
        struct Item {
            next: Option<Entity>,
            value: u64,
        }

        let mut items = vec![OwnedObj::new(Item {
            next: None,
            value: 2,
        })];

        let start = {
            let mut curr = items[0].obj();

            for i in 1..100_000 {
                let item = OwnedObj::new(Item {
                    next: Some(curr.entity()),
                    value: i + 100,
                });
                curr = item.obj();
                items.push(item);
            }
            curr.entity()
        };

        let storage = storage::<Item>();

        c.iter(|| {
            let mut cursor = Some(start);
            let mut accum = 0;

            while let Some(curr) = cursor {
                let item = storage.get_mut(curr);
                accum += item.value;
                cursor = item.next;
            }

            accum
        });
    });

    c.bench_function("linked_list.obj", |c| {
        struct Item {
            next: Option<Obj<Self>>,
            value: u64,
        }

        let mut items = vec![OwnedObj::new(Item {
            next: None,
            value: 2,
        })];
        let start = {
            let mut curr = items[0].obj();

            for i in 1..100_000 {
                let item = OwnedObj::new(Item {
                    next: Some(curr),
                    value: i + 100,
                });
                curr = item.obj();
                items.push(item);
            }
            curr
        };

        c.iter(|| {
            let mut cursor = Some(start);
            let mut accum = 0;

            while let Some(curr) = cursor {
                let item = curr.get_mut_maybe_aba();
                accum += item.value;
                cursor = item.next;
            }

            accum
        });
    });

    c.bench_function("linked_list.slot", |c| {
        struct Item {
            next: Option<Slot<Self>>,
            value: u64,
        }

        let mut items = vec![OwnedObj::new(Item {
            next: None,
            value: 2,
        })];
        let start = {
            let mut curr = items[0].obj();

            for i in 1..100_000 {
                let item = OwnedObj::new(Item {
                    next: Some(curr.value()),
                    value: i + 100,
                });
                curr = item.obj();
                items.push(item);
            }
            curr.value()
        };

        let token = MainThreadToken::acquire();

        c.iter(|| {
            let mut cursor = Some(start);
            let mut accum = 0;

            while let Some(curr) = cursor {
                let item = curr.borrow_mut(token);
                accum += item.value;
                cursor = item.next;
            }

            accum
        });
    });

    c.bench_function("linked_list_rng.regular", |c| {
        struct Item {
            next: usize,
            value: u64,
        }

        let mut items = (0..100_000)
            .map(|i| Item {
                next: usize::MAX,
                value: i + 100,
            })
            .collect::<Vec<_>>();

        let (start, chain) = generate_permuted_chain(100_000);

        for (src, target) in chain.into_iter().enumerate() {
            items[src].next = target;
        }

        c.iter(|| {
            let mut cursor = start;
            let mut accum = 0;

            while cursor < items.len() {
                accum += items[cursor].value;
                cursor = items[cursor].next;
            }

            accum
        });
    });

    c.bench_function("linked_list_rng.slot", |c| {
        struct Item {
            next: Option<Slot<Self>>,
            value: u64,
        }

        let items = (0..100_000)
            .map(|i| {
                OwnedObj::new(Item {
                    next: None,
                    value: i + 100,
                })
            })
            .collect::<Vec<_>>();

        let (start, chain) = generate_permuted_chain(100_000);

        for (src, target) in chain.into_iter().enumerate() {
            items[src].get_mut().next = items.get(target).map(|v| v.value());
        }

        let start = items[start].value();
        let token = MainThreadToken::acquire();

        c.iter(|| {
            let mut cursor = Some(start);
            let mut accum = 0;

            while let Some(curr) = cursor {
                let item = curr.borrow_mut(token);
                accum += item.value;
                cursor = item.next;
            }

            accum
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
                .with(Position(i as f32))
                .with_tag(pos_tag)
                .with(Velocity(i as f32))
                .with_tag(vel_tag)
        })
        .collect()
}

fn generate_permuted_chain(n: usize) -> (usize, Vec<usize>) {
    fastrand::seed(4);

    let mut remaining = (0..n).collect::<Vec<_>>();
    let mut chain = (0..n).map(|_| usize::MAX).collect::<Vec<_>>();

    let start = remaining.swap_remove(fastrand::usize(0..remaining.len()));
    let mut cursor = start;

    while !remaining.is_empty() {
        let target = remaining.swap_remove(fastrand::usize(0..remaining.len()));

        chain[cursor] = target;
        cursor = target;
    }

    (start, chain)
}
