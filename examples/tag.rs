use bort::{flush, query::query_i32, query_tagged, OwnedEntity, Tag};

fn main() {
    let foo = Tag::<i32>::new();
    let bar = Tag::<u32>::new();

    let ent = OwnedEntity::new();

    assert!(!ent.is_tagged(foo));

    println!("tagged 1");
    ent.tag(foo);
    assert!(ent.is_tagged(foo));

    println!("{:?}", query_tagged([foo]).collect::<Vec<_>>());

    println!("tagged 2");
    ent.tag(foo);
    assert!(ent.is_tagged(foo));

    println!("Flushed");
    flush();

    println!("{:?}", query_tagged([foo]).collect::<Vec<_>>());
    println!(
        "{:?}",
        query_tagged([bar.raw(), foo.raw()]).collect::<Vec<_>>()
    );

    ent.tag(bar);
    println!(
        "{:?}",
        query_tagged([bar.raw(), foo.raw()]).collect::<Vec<_>>()
    );
    flush();
    println!(
        "{:?}",
        query_tagged([bar.raw(), foo.raw()]).collect::<Vec<_>>()
    );

    println!("untagged 1");
    ent.untag(foo);
    assert!(!ent.is_tagged(foo));

    println!("untagged 2");
    ent.untag(foo);
    assert!(!ent.is_tagged(foo));

    println!("{:?}", query_tagged([foo]).collect::<Vec<_>>());
    println!("{:?}", query_tagged([bar]).collect::<Vec<_>>());

    println!("Flushed");
    flush();

    println!("{:?}", query_tagged([foo]).collect::<Vec<_>>());
    println!("{:?}", query_tagged([bar]).collect::<Vec<_>>());

    println!("tagged 3");
    ent.tag(foo);
    assert!(ent.is_tagged(foo));

    println!("tagged 4");
    ent.tag(foo);
    assert!(ent.is_tagged(foo));

    println!("{:?}", query_tagged([foo]).collect::<Vec<_>>());

    println!("Flushed again");
    flush();

    println!("{:?}", query_tagged([foo]).collect::<Vec<_>>());

    println!("Inserting");
    ent.insert(1i32);

    println!("{:?}", query_tagged([foo]).collect::<Vec<_>>());

    for (ent, mut val) in query_i32(foo) {
        println!("{ent:?}: {val:?}");
        *val += 1;
    }

    println!("Destroying.");
    ent.destroy();

    println!("{:?}", query_tagged([foo]).collect::<Vec<_>>());

    println!("Flushed");
    flush();

    println!("{:?}", query_tagged([foo]).collect::<Vec<_>>());
}
