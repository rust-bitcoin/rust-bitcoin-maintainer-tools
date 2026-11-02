//! Tests that associated types and constants are correctly annotated with their impl trait.

pub struct Foo;

pub trait Container {
    type Item;
    type Error;
}

impl Container for Foo {
    type Item = String;
    type Error = std::io::Error;
}
