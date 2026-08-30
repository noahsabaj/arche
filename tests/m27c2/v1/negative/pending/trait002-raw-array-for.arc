pub fn invalid(mut array: [i32; 3]) {
    for ref mut element in array {
        *element += 1i32;
    };
}
