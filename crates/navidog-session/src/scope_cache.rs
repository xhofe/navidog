//! A keyed async cache: one loading/ready/failed entry and one task per key. The navigator's
//! metadata catalog and the completion outline share the same state machine; only what a value
//! is and how a key is formed differ.

use std::collections::HashMap;
use std::hash::Hash;

use gpui_kit::{SharedString, Task};

#[derive(Clone, Debug, PartialEq)]
pub enum Load<T> {
    Loading,
    Ready(T),
    Failed(SharedString),
}

pub struct ScopeCache<K, T> {
    entries: HashMap<K, Load<T>>,
    tasks: HashMap<K, Task<()>>,
}

impl<K, T> Default for ScopeCache<K, T> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            tasks: HashMap::new(),
        }
    }
}

impl<K: Eq + Hash, T> ScopeCache<K, T> {
    pub fn get(&self, key: &K) -> Option<&Load<T>> {
        self.entries.get(key)
    }

    pub fn ready(&self, key: &K) -> Option<T>
    where
        T: Clone,
    {
        match self.entries.get(key)? {
            Load::Ready(value) => Some(value.clone()),
            _ => None,
        }
    }

    pub fn contains(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }

    pub fn task_pending(&self, key: &K) -> bool {
        self.tasks.contains_key(key)
    }

    pub fn has_pending_tasks(&self) -> bool {
        !self.tasks.is_empty()
    }

    pub fn insert_loading(&mut self, key: K) {
        self.entries.insert(key, Load::Loading);
    }

    pub fn insert(&mut self, key: K, state: Load<T>) {
        self.entries.insert(key, state);
    }

    pub fn set_task(&mut self, key: K, task: Task<()>) {
        self.tasks.insert(key, task);
    }

    /// Removes the in-flight task marker; false when an invalidation already
    /// dropped it and the result must not be written back.
    pub fn finish_task(&mut self, key: &K) -> bool {
        self.tasks.remove(key).is_some()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.tasks.clear();
    }

    /// Drops the entries and tasks whose key `keep` rejects (a closed or deleted connection).
    pub fn retain(&mut self, mut keep: impl FnMut(&K) -> bool) {
        self.entries.retain(|key, _| keep(key));
        self.tasks.retain(|key, _| keep(key));
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.entries.keys()
    }
}
