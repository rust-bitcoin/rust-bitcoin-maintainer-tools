//! Tests blanket trait impls on generic types with multiple trait bounds in the impl signature.

pub struct Value<V> {
    _marker: std::marker::PhantomData<V>,
}

pub trait Handler {
    fn handle(&self);
}

impl<V: Clone> Handler for Value<V> {
    fn handle(&self) {}
}
