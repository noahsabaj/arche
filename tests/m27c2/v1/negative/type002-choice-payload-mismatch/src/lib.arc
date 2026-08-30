pub enum Choice<T> {
    Pair { left: T, right: T },
}

pub fn invalid(value: u32) -> Choice<i32> {
    Choice::Pair { left: 1i32, right: value }
}
