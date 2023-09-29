use bort::delegate;

delegate! {
    fn Whee()
}

fn main() {
    dbg!(maz());
}

fn maz() -> Whee {
    Whee::new(|| {})
}
