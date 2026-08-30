pub fn invalid<K>(values: &mut Vec<K>, key: K) -> K {
    values.push(key);
    key
}
