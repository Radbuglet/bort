use bort::{debug::dump_database_state, prelude::*};

fn main() {
    let pos_tag = Tag::<i32>::new();
    let vel_tag = Tag::<u32>::new();

    let entity_1 = OwnedEntity::new();
    entity_1.tag(pos_tag);
    entity_1.insert(1i32);
    entity_1.tag(vel_tag);
    entity_1.insert(2u32);

    let entity_2 = OwnedEntity::new();
    entity_2.tag(pos_tag);
    entity_2.insert(1i32);
    entity_2.tag(vel_tag);
    entity_2.insert(2u32);

    println!("{}", dump_database_state());
}
