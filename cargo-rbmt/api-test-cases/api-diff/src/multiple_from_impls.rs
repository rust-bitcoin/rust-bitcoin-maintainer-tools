//! Tests multiple From trait impls for the same generic type with different type parameters.

pub struct DefaultMarker;
pub struct CustomMarker;

pub struct Value<V> {
    _marker: std::marker::PhantomData<V>,
}

impl From<DefaultMarker> for Value<DefaultMarker> {
    fn from(_: DefaultMarker) -> Self {
        unimplemented!()
    }
}

impl From<CustomMarker> for Value<CustomMarker> {
    fn from(_: CustomMarker) -> Self {
        unimplemented!()
    }
}
