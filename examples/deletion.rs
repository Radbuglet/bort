use bort::{debug::archetype_count, flush, Entity, Tag};

fn main() {
    let tag_1 = Tag::<i32>::new();
    let tag_2 = Tag::<u32>::new();
    let haha = Entity::new_unmanaged()
        .with_tagged(tag_1, 3)
        .with_tagged(tag_2, 4);

    flush();
    haha.untag(tag_1);
    haha.destroy();
    flush();

    // Test
    assert_eq!(archetype_count(), 1);
}
