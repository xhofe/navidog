//! Saved column display choices retain unavailable plugin ids for later recovery.
use crate::{json_store::JsonStore, object::Scope, paths};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Default, Serialize, Deserialize)]
struct Choices {
    tables: BTreeMap<String, BTreeMap<String, String>>,
}
fn key(scope: &Scope, table: &str) -> String {
    format!(
        "{}\u{1f}{}\u{1f}{table}",
        scope.database,
        scope.schema.as_deref().unwrap_or("")
    )
}
fn path(scope: &Scope) -> std::path::PathBuf {
    paths::profile_dir_in(&paths::config_dir(), &scope.connection).join(".cell-formatters.json")
}
pub fn load(scope: &Scope, table: &str) -> BTreeMap<String, String> {
    JsonStore::new(path(scope))
        .load_optional::<Choices>()
        .unwrap_or_default()
        .tables
        .remove(&key(scope, table))
        .unwrap_or_default()
}
pub fn store(
    scope: &Scope,
    table: &str,
    column: &str,
    formatter: Option<&str>,
) -> Result<(), String> {
    let store = JsonStore::new(path(scope));
    let mut choices = store.load_optional::<Choices>().unwrap_or_default();
    let columns = choices.tables.entry(key(scope, table)).or_default();
    if let Some(formatter) = formatter {
        columns.insert(column.into(), formatter.into());
    } else {
        columns.remove(column);
    }
    store.save(&choices).map_err(|e| e.to_string())
}
