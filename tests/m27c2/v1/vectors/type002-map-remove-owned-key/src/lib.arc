pub fn invalid<K, V>(map: &mut Map<K, V>, key: K) -> Option<V>
where K: Eq + Ord {
    map.remove(key)
}
