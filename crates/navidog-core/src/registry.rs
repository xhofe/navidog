//! Extension-point registries (03-plugin-api.md section 2). Built-in features and plugins
//! share one registry; every entry carries a `Source` and plugins are removed as a group.

use gpui_kit::{Action, App, Global, SharedString};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    Builtin,
    Plugin(SharedString),
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RegistryError {
    #[error("`{0}` is already registered")]
    Duplicate(String),
}

/// A registry indexed by key that keeps insertion order.
pub struct Registry<V> {
    entries: Vec<(SharedString, Source, V)>,
}

impl<V> Default for Registry<V> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl<V> Registry<V> {
    pub fn register(
        &mut self,
        key: impl Into<SharedString>,
        source: Source,
        value: V,
    ) -> Result<(), RegistryError> {
        let key = key.into();
        if self.get(&key).is_some() {
            return Err(RegistryError::Duplicate(key.to_string()));
        }
        self.entries.push((key, source, value));
        Ok(())
    }

    pub fn unregister_source(&mut self, source: &Source) {
        self.entries.retain(|(_, s, _)| s != source);
    }

    pub fn get(&self, key: &str) -> Option<&V> {
        self.entries
            .iter()
            .find(|(k, _, _)| k == key)
            .map(|(_, _, v)| v)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&SharedString, &Source, &V)> {
        self.entries.iter().map(|(k, s, v)| (k, s, v))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// A command: one Action shared by menus, toolbars, context menus and shortcuts.
pub struct CommandInfo {
    /// i18n key
    pub title: SharedString,
    /// Icon asset path, for example `icons/plug.svg`
    pub icon: Option<SharedString>,
    pub shortcut: Option<SharedString>,
    pub action: fn() -> Box<dyn Action>,
    /// Whether it is currently enabled (for example a database must be selected); `None` means always
    pub enabled: Option<fn(&App) -> bool>,
}

impl CommandInfo {
    pub fn is_enabled(&self, cx: &App) -> bool {
        self.enabled.is_none_or(|f| f(cx))
    }
}

macro_rules! global_registry {
    ($name:ident, $value:ty) => {
        #[derive(Default)]
        pub struct $name(pub Registry<$value>);
        impl Global for $name {}
        impl std::ops::Deref for $name {
            type Target = Registry<$value>;
            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }
        impl std::ops::DerefMut for $name {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.0
            }
        }
    };
}

global_registry!(CommandRegistry, CommandInfo);

pub(crate) fn init(cx: &mut App) {
    cx.set_global(CommandRegistry::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_duplicates_and_removes_by_source() {
        let mut r: Registry<u8> = Registry::default();
        r.register("a", Source::Builtin, 1).unwrap();
        r.register("p/b", Source::Plugin("p".into()), 2).unwrap();
        assert_eq!(
            r.register("a", Source::Builtin, 3),
            Err(RegistryError::Duplicate("a".into()))
        );
        r.unregister_source(&Source::Plugin("p".into()));
        assert_eq!(
            r.iter().map(|(k, _, _)| k.to_string()).collect::<Vec<_>>(),
            ["a"]
        );
    }
}
