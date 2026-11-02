//! A trait defines a default method implementation and a type implements the trait without
//! overriding that method.

pub struct Foo;

pub trait Bar {
    fn do_it() {}
}

impl Bar for Foo {}

#[test]
fn trait_default_dedup() { <Foo as Bar>::do_it(); }
