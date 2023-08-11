use saddle::{behavior, cx, namespace, universe, RootBehaviorToken};

universe!(pub MyUniverse);

namespace! {
    pub MyBehavior1 in MyUniverse;
    pub MyBehavior2 in MyUniverse;
    pub MyBehavior3 in MyUniverse;
}

cx! {
    pub trait Foo(MyUniverse) = mut u32;
    pub trait Bar(MyUniverse): Foo = ref i32, mut f32;
}

fn main() {
    let mut bhv = RootBehaviorToken::acquire();

    behavior! {
        as MyBehavior1[bhv] do
        (cx: [Bar; mut f64], bhv2: [MyBehavior2]) {
            behavior! {
                as MyBehavior2[bhv2] do
                (cx: [], bhv: [MyBehavior3]) {
                    behavior! {
                        as MyBehavior3[bhv] do
                        (cx: [;ref f64], bhv: []) {
                            // :)
                        }
                    }
                }
            }

            behavior! {
                as MyBehavior2[bhv2] do
                (cx: [;ref f64], bhv: [MyBehavior3]) {
                    // :)
                }
            }
        }
    }
}
