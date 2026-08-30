pub fn invalid<'a, K, V>(entries: &'a Map<K, V>)
where K: Eq + Ord {
    let iter = entries.iter();
    for entry in iter {
        let _ = entry;
    };
}
