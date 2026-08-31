pub fn invalid(value: i32) -> i32 {
    match value {
        bound => bound,
        _ => 0i32,
    }
}
