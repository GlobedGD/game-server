use std::hash::{BuildHasher, Hash};

use dashmap::DashMap;

pub fn iter_dashmap<'a, K, V, H, F>(map: &'a DashMap<K, V, H>, mut f: F)
where
    K: 'a + Eq + Hash,
    V: 'a,
    H: BuildHasher + Clone,
    F: FnMut((&K, &V)),
{
    for shard in map.shards() {
        let shard = shard.read();

        unsafe {
            // SAFETY: the iterator cannot outlive the lock
            for item in shard.iter() {
                let (k, v) = item.as_ref();
                f((k, v.get()));
            }
        }
    }
}

pub fn iter_dashmap_mut<'a, K, V, H, F>(map: &'a DashMap<K, V, H>, mut f: F)
where
    K: 'a + Eq + Hash,
    V: 'a,
    H: BuildHasher + Clone,
    F: FnMut((&K, &mut V)),
{
    for shard in map.shards() {
        let shard = shard.write();

        unsafe {
            // SAFETY: the iterator cannot outlive the lock
            for item in shard.iter() {
                let (k, v) = item.as_mut();
                f((k, v.get_mut()));
            }
        }
    }
}
