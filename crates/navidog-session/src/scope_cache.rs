//! A scope-keyed async cache: one loading/ready/failed entry and one task per
//! scope. The navigator's metadata catalog and the completion outline share
//! the same state machine; only how they fetch a value differs.

use std::collections::HashMap;

use gpui_kit::{SharedString, Task};
use navidog_core::object::Scope;

#[derive(Clone, Debug, PartialEq)]
pub enum Load<T> {
    Loading,
    Ready(T),
    Failed(SharedString),
}

pub struct ScopeCache<T> {
    entries: HashMap<Scope, Load<T>>,
    tasks: HashMap<Scope, Task<()>>,
}

impl<T> Default for ScopeCache<T> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            tasks: HashMap::new(),
        }
    }
}

impl<T> ScopeCache<T> {
    pub fn get(&self, scope: &Scope) -> Option<&Load<T>> {
        self.entries.get(scope)
    }

    pub fn ready(&self, scope: &Scope) -> Option<T>
    where
        T: Clone,
    {
        match self.entries.get(scope)? {
            Load::Ready(value) => Some(value.clone()),
            _ => None,
        }
    }

    pub fn contains(&self, scope: &Scope) -> bool {
        self.entries.contains_key(scope)
    }

    pub fn task_pending(&self, scope: &Scope) -> bool {
        self.tasks.contains_key(scope)
    }

    pub fn has_pending_tasks(&self) -> bool {
        !self.tasks.is_empty()
    }

    pub fn insert_loading(&mut self, scope: Scope) {
        self.entries.insert(scope, Load::Loading);
    }

    pub fn insert(&mut self, scope: Scope, state: Load<T>) {
        self.entries.insert(scope, state);
    }

    pub fn set_task(&mut self, scope: Scope, task: Task<()>) {
        self.tasks.insert(scope, task);
    }

    /// Removes the in-flight task marker; false when an invalidation already
    /// dropped it and the result must not be written back.
    pub fn finish_task(&mut self, scope: &Scope) -> bool {
        self.tasks.remove(scope).is_some()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.tasks.clear();
    }

    pub fn retain_connection(&mut self, id: &str) {
        self.entries.retain(|scope, _| scope.connection != id);
        self.tasks.retain(|scope, _| scope.connection != id);
    }
}
