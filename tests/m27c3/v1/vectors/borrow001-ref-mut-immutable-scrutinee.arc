pub fn invalid(reference: &mut i32) -> i32 {
    let ref mut borrowed = reference;
    **borrowed
}
