/// A normal dependency whose provenance must remain in resolved HIR.
pub struct Scalar(pub i32);

pub fn twice(value: i32) -> i32 {
    value + value
}
