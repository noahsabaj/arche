pub struct Holder<T> {
    pub value: T,
}

impl<T> Clone for Holder<T>
where T: Clone {
    fn clone(&self) -> Holder<T> {
        Holder { value: self.value }
    }
}
