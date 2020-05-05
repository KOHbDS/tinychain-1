use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::iter::FromIterator;
use std::sync::RwLock;

use crate::transaction::TransactionId;

#[derive(Debug)]
pub struct Map<K: Eq + Hash, V> {
    map: RwLock<HashMap<K, V>>,
}

impl<K: Eq + Hash, V: Clone> Map<K, V> {
    pub fn new() -> Map<K, V> {
        Map {
            map: RwLock::new(HashMap::new()),
        }
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.map.read().unwrap().contains_key(key)
    }

    pub fn get(&self, key: &K) -> Option<V> {
        match self.map.read().unwrap().get(key) {
            Some(val) => Some(val.clone()),
            None => None,
        }
    }

    pub fn insert(&self, key: K, value: V) -> Option<V> {
        self.map.write().unwrap().insert(key, value)
    }

    pub fn remove(&self, key: &K) -> Option<V> {
        self.map.write().unwrap().remove(key)
    }
}

impl<K: Eq + Hash, V> FromIterator<(K, V)> for Map<K, V> {
    fn from_iter<T: IntoIterator<Item = (K, V)>>(i: T) -> Map<K, V> {
        let mut map: HashMap<K, V> = HashMap::new();
        for (k, v) in i {
            map.insert(k, v);
        }
        Map {
            map: RwLock::new(map),
        }
    }
}

#[derive(Debug)]
pub struct Deque<V> {
    deque: RwLock<VecDeque<V>>,
}

impl<V> Deque<V> {
    pub fn new() -> Deque<V> {
        Deque {
            deque: RwLock::new(VecDeque::new()),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.deque.read().unwrap().is_empty()
    }

    pub fn len(&self) -> usize {
        self.deque.read().unwrap().len()
    }

    pub fn pop_front(&self) -> Option<V> {
        self.deque.write().unwrap().pop_front()
    }

    pub fn push_back(&self, item: V) {
        self.deque.write().unwrap().push_back(item)
    }
}

pub struct TransactionCache<K: Eq + Hash, V: Clone> {
    cache: RwLock<HashMap<TransactionId, HashMap<K, V>>>,
}

impl<K: Eq + Hash, V: Clone> TransactionCache<K, V> {
    pub fn new() -> TransactionCache<K, V> {
        TransactionCache {
            cache: RwLock::new(HashMap::new()),
        }
    }

    pub fn close(&self, txn_id: &TransactionId) -> HashMap<K, V> {
        println!("TransactionCache closing {}", txn_id);
        self.cache
            .write()
            .unwrap()
            .remove(txn_id)
            .unwrap_or_else(HashMap::new)
    }

    pub fn get(&self, txn_id: &TransactionId, key: &K) -> Option<V> {
        if let Some(entries) = self.cache.read().unwrap().get(txn_id) {
            entries.get(key).cloned()
        } else {
            None
        }
    }

    pub fn insert(&self, txn_id: TransactionId, key: K, value: V) {
        let mut cache = self.cache.write().unwrap();
        if let Some(map) = cache.get_mut(&txn_id) {
            map.insert(key, value);
        } else {
            let mut map: HashMap<K, V> = HashMap::new();
            map.insert(key, value);
            cache.insert(txn_id, map);
        }
    }
}
