use bort::{debug::dump_database_state, flush, OwnedEntity, Tag};

fn main() {
    let hehe = Tag::<i32>::new();
    let haha = OwnedEntity::new().with_tagged(hehe, 3);
    flush();
    drop(haha);
    flush();

    println!("{}", dump_database_state());
}
