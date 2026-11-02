//! A single type implements multiple traits that have methods with the same name and signature.

pub struct Foo;

pub trait Bar {}

pub trait Baz {
    fn do_it() -> i32;
}

impl Bar for Foo {}

impl Baz for Foo {
    fn do_it() -> i32 { 42 }
}

impl Foo {
    pub fn new_method() {
        // New method added for testing
    }
}

#[test]
fn multiple_trait_impls() {
    Foo::new_method();
    <Foo as Baz>::do_it();
}
