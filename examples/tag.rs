use bort::{debug::dump_database_state, OwnedEntity, Tag};

fn main() {
    let foo = Tag::<i32>::new();

    let bar = OwnedEntity::new();

    assert!(!bar.is_tagged(foo));

    println!("tagged 1");
    bar.tag(foo);
    assert!(bar.is_tagged(foo));

    println!("tagged 2");
    bar.tag(foo);
    assert!(bar.is_tagged(foo));

    bar.insert(1i32);

    println!("untagged 1");
    bar.untag(foo);
    assert!(!bar.is_tagged(foo));

    println!("untagged 2");
    bar.untag(foo);
    assert!(!bar.is_tagged(foo));

    println!("{}", dump_database_state());
}
