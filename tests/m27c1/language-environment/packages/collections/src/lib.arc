pub struct Ordered<T> {
    pub values: Vec<T>,
}

pub fn empty<T>() -> Ordered<T> {
    Ordered { values: [] }
}
