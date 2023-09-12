use std::collections::HashMap;

// merge two hashmaps using the provided update function
pub fn merge_maps<K, V, F>(
    base_map: &HashMap<K, V>,
    update_map: &HashMap<K, V>,
    update_fn: F,
    default: V,
) -> HashMap<K, V>
where
    K: std::hash::Hash + std::cmp::Eq + std::clone::Clone,
    V: std::clone::Clone,
    F: Fn(&V, &V, &K) -> V,
{
    let mut merged_map = base_map.clone();

    for (k, v2) in update_map.iter() {
        let entry = merged_map
            .entry(k.clone())
            .or_insert_with(|| default.clone());
        *entry = update_fn(entry, v2, k);
    }

    merged_map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_maps() {
        let base_map: HashMap<&str, i32> = [("foo", 1), ("bar", 2)].iter().cloned().collect();
        let update_map: HashMap<&str, i32> = [("foo", 3), ("baz", 4)].iter().cloned().collect();

        let sum_fn = |a: &i32, b: &i32, _k: &&str| *a + *b;

        let merged_map = merge_maps(&base_map, &update_map, sum_fn, 0);

        assert_eq!(merged_map.get("foo"), Some(&4));
        assert_eq!(merged_map.get("bar"), Some(&2));
        assert_eq!(merged_map.get("baz"), Some(&4));
    }

    #[test]
    fn test_merge_maps_empty() {
        let base_map: HashMap<&str, i32> = HashMap::new();
        let update_map: HashMap<&str, i32> = HashMap::new();

        let sum_fn = |a: &i32, b: &i32, _k: &&str| *a + *b;

        let merged_map = merge_maps(&base_map, &update_map, sum_fn, 0);

        assert_eq!(merged_map.len(), 0);
    }

    #[test]
    fn test_merge_maps_no_updates() {
        let base_map: HashMap<&str, i32> = [("foo", 1), ("bar", 2)].iter().cloned().collect();
        let update_map: HashMap<&str, i32> = HashMap::new();

        let sum_fn = |a: &i32, b: &i32, _k: &&str| *a + *b;

        let merged_map = merge_maps(&base_map, &update_map, sum_fn, 0);

        assert_eq!(merged_map.get("foo"), Some(&1));
        assert_eq!(merged_map.get("bar"), Some(&2));
        assert_eq!(merged_map.len(), 2);
    }
}
