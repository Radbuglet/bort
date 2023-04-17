use bort::OwnedObj;

fn main() {
    let mut entities = Vec::new();

    for _ in 0..10_000 {
        if fastrand::bool() && !entities.is_empty() {
            entities.swap_remove(fastrand::usize(0..entities.len()));
        } else {
            println!("Allocated!");
            entities.push(OwnedObj::new(3));
        }
    }
}
