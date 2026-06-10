//! Tests generic structs with both generic and specialized impl blocks.

pub struct DefaultMarker;
pub struct CustomMarker;

pub struct Value<V = DefaultMarker> {
    _marker: std::marker::PhantomData<V>,
}

impl<V> Value<V> {
    pub fn get(&self) {}
}

impl Value<DefaultMarker> {
    pub fn default_value() -> Self {
        unimplemented!()
    }
}

impl Value<CustomMarker> {
    pub fn custom_value() -> Self {
        unimplemented!()
    }
}
