//! A small LRU cache bounded by the total byte size of its values.

use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;

use crate::pixels::DecodedImage;

/// Values that know how much memory they hold.
pub trait Weighted {
    fn weight(&self) -> usize;
}

impl Weighted for DecodedImage {
    fn weight(&self) -> usize {
        self.byte_size()
    }
}

impl<T: Weighted> Weighted for Arc<T> {
    fn weight(&self) -> usize {
        (**self).weight()
    }
}

/// LRU cache that evicts least-recently-used entries once the summed weight
/// exceeds `budget`. The cache holds few entries (decoded images), so a linear
/// scan for the eviction victim is cheaper than maintaining a linked list.
pub struct ByteLru<K, V> {
    map: HashMap<K, (V, u64)>,
    clock: u64,
    used: usize,
    budget: usize,
}

impl<K: Eq + Hash + Clone, V: Weighted> ByteLru<K, V> {
    pub fn new(budget: usize) -> Self {
        Self {
            map: HashMap::new(),
            clock: 0,
            used: 0,
            budget,
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn used(&self) -> usize {
        self.used
    }

    pub fn budget(&self) -> usize {
        self.budget
    }

    pub fn set_budget(&mut self, budget: usize) {
        self.budget = budget;
        self.evict(None);
    }

    pub fn contains<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.map.contains_key(key)
    }

    /// Returns the value and marks it most recently used.
    pub fn get<Q>(&mut self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.clock += 1;
        let clock = self.clock;
        self.map.get_mut(key).map(|(v, t)| {
            *t = clock;
            &*v
        })
    }

    /// Inserts a value. A value larger than the whole budget is still kept
    /// (everything else is evicted) so the current image is never dropped.
    pub fn insert(&mut self, key: K, value: V) {
        self.clock += 1;
        self.used += value.weight();
        if let Some((old, _)) = self.map.insert(key.clone(), (value, self.clock)) {
            self.used -= old.weight();
        }
        self.evict(Some(&key));
    }

    pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        let (v, _) = self.map.remove(key)?;
        self.used -= v.weight();
        Some(v)
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.used = 0;
    }

    fn evict(&mut self, keep: Option<&K>) {
        while self.used > self.budget {
            let victim = self
                .map
                .iter()
                .filter(|(k, _)| Some(*k) != keep)
                .min_by_key(|(_, (_, t))| *t)
                .map(|(k, _)| k.clone());
            match victim {
                Some(k) => {
                    self.remove(&k);
                }
                None => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct W(usize);
    impl Weighted for W {
        fn weight(&self) -> usize {
            self.0
        }
    }

    #[test]
    fn evicts_least_recently_used() {
        let mut c = ByteLru::new(100);
        c.insert("a", W(40));
        c.insert("b", W(40));
        assert!(c.get("a").is_some()); // a is now newer than b
        c.insert("c", W(40));
        assert!(c.contains("a") && c.contains("c") && !c.contains("b"));
        assert_eq!(c.used(), 80);
    }

    #[test]
    fn oversized_entry_is_kept_alone() {
        let mut c = ByteLru::new(100);
        c.insert("a", W(10));
        c.insert("big", W(500));
        assert_eq!(c.len(), 1);
        assert!(c.contains("big"));
        assert_eq!(c.used(), 500);
    }

    #[test]
    fn replacing_updates_weight() {
        let mut c = ByteLru::new(100);
        c.insert("a", W(10));
        c.insert("a", W(30));
        assert_eq!(c.used(), 30);
        assert_eq!(c.remove("a").map(|w| w.0), Some(30));
        assert_eq!(c.used(), 0);
        assert!(c.is_empty());
    }

    #[test]
    fn shrinking_budget_evicts() {
        let mut c = ByteLru::new(100);
        c.insert(1, W(30));
        c.insert(2, W(30));
        c.insert(3, W(30));
        c.set_budget(50);
        assert_eq!(c.len(), 1);
        assert!(c.contains(&3));
    }
}
