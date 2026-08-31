pub trait VisibleMethod {
    fn value(&self) -> i32;
}

pub struct VisibleTarget;

impl VisibleMethod for VisibleTarget {
    pub fn value(&self) -> i32 {
        0i32
    }
}
