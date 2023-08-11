saddle::universe!(pub Whee);

saddle::cx! {
    pub trait Foo(Whee) = mut u32;
}

saddle::cx! {
    pub trait Bar(Whee): Foo = ref i32, mut f32;
}

fn main() {}
