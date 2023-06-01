use bort::OwnedObj;

fn main() {
    let mut entities = Vec::new();

    for _ in 0..10_000 {
        println!(
            "Heaps: {}; Orcs: {}",
            bort::debug::heap_count(),
            bort::debug::slot_count()
        );

        if fastrand::f32() > 0.99 {
            for _ in 0..fastrand::u8(64..255) {
                entities.pop();
            }
        } else {
            entities.push(OwnedObj::new(3));
        }
    }

    println!("=== Dropping entities ===");
    drop(entities);
    println!(
        "Heaps: {}; Orcs: {}",
        bort::debug::heap_count(),
        bort::debug::slot_count()
    );
}
