use bort::{entity::VirtualTag, OwnedEntity};

fn main() {
    let foo = VirtualTag::new();

    let bar = OwnedEntity::new();

    assert!(!bar.is_tagged(foo));

    println!("tagged 1");
    bar.tag(foo);
    assert!(bar.is_tagged(foo));

    println!("tagged 2");
    bar.tag(foo);
    assert!(bar.is_tagged(foo));

    println!("untagged 1");
    bar.untag(foo);
    assert!(!bar.is_tagged(foo));

    println!("untagged 2");
    bar.untag(foo);
    assert!(!bar.is_tagged(foo));
}
