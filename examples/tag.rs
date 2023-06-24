use bort::{flush, query_tagged, OwnedEntity, Tag};

fn main() {
    let foo = Tag::<i32>::new();

    let bar = OwnedEntity::new();

    assert!(!bar.is_tagged(foo));

    println!("tagged 1");
    bar.tag(foo);
    assert!(bar.is_tagged(foo));

    println!("{:?}", query_tagged(foo).collect::<Vec<_>>());

    println!("tagged 2");
    bar.tag(foo);
    assert!(bar.is_tagged(foo));

    println!("Flushed");
    flush();

    println!("{:?}", query_tagged(foo).collect::<Vec<_>>());

    println!("untagged 1");
    bar.untag(foo);
    assert!(!bar.is_tagged(foo));

    println!("untagged 2");
    bar.untag(foo);
    assert!(!bar.is_tagged(foo));

    println!("{:?}", query_tagged(foo).collect::<Vec<_>>());

    println!("Flushed");
    flush();

    println!("{:?}", query_tagged(foo).collect::<Vec<_>>());

    println!("tagged 3");
    bar.tag(foo);
    assert!(bar.is_tagged(foo));

    println!("tagged 4");
    bar.tag(foo);
    assert!(bar.is_tagged(foo));

    println!("{:?}", query_tagged(foo).collect::<Vec<_>>());

    println!("Flushed again");
    flush();

    println!("{:?}", query_tagged(foo).collect::<Vec<_>>());

    println!("Inserting");
    bar.insert(1i32);

    println!("{:?}", query_tagged(foo).collect::<Vec<_>>());

    println!("Destroying.");
    bar.destroy();

    println!("{:?}", query_tagged(foo).collect::<Vec<_>>());

    println!("Flushed");
    flush();

    println!("{:?}", query_tagged(foo).collect::<Vec<_>>());
}
