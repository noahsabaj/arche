pub fn invalid<K, V>(entries: Map<K, V>)
where K: Eq + Ord {
    for entry in entries {
        let _ = entry;
    };
}
