pub struct Values<T> {
    pub values: Vec<T>,
}

pub fn invalid<T>() -> Values<T> {
    Values { values: [] }
}
