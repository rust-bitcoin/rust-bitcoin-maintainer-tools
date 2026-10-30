//! Internal utilities not meant for public use.

pub struct InternalHelper {
    pub value: i32,
}

impl InternalHelper {
    pub fn new(value: i32) -> Self {
        Self { value }
    }
}

pub trait InternalTrait {
    fn do_something(&self);
}
