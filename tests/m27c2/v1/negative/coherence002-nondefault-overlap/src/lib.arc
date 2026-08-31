pub struct Holder {
    pub v: i32,
}
pub trait One {
    fn one(&self) -> i32;
}
impl One for Holder {
    fn one(&self) -> i32 {
        1i32
    }
}
impl One for Holder {
    fn one(&self) -> i32 {
        2i32
    }
}
