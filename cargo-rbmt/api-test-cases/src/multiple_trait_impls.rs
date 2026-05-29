//! A single type implements multiple traits that have methods with the same name and signature.

pub struct Foo;

pub trait Bar {
    fn do_it();
}

pub trait Baz {
    fn do_it();
}

impl Bar for Foo {
    fn do_it() {}
}

impl Baz for Foo {
    fn do_it() {}
}

#[test]
fn multiple_trait_impls() {
    <Foo as Bar>::do_it();
    <Foo as Baz>::do_it();
}
