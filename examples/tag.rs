use bort::{query_all, Tag};

fn main() {
    let a = Tag::<i32>::new();
    let b = Tag::<i32>::new();
    let c = Tag::<u32>::new();

    for (entity, a, mut b, c) in query_all((a.as_ref(), b.as_mut(), c.as_ref())) {
        entity.get::<u32>();
        *b += *a + *c as i32;
    }
}
