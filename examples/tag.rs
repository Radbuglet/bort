use bort::{OwnedEntity, Tag};

fn main() {
    let foo = Tag::new();

    let bar = OwnedEntity::new();
    assert!(!bar.is_tagged(foo));
    bar.tag(foo);
    assert!(bar.is_tagged(foo));
    bar.tag(foo);
    assert!(bar.is_tagged(foo));
    bar.untag(foo);
    assert!(!bar.is_tagged(foo));
    bar.untag(foo);
    assert!(!bar.is_tagged(foo));
}
