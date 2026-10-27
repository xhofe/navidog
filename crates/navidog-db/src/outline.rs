//! Completion outline: tables, views, functions and columns of one database or schema.
//! Each database crate fetches its own; this module holds the value and the row grouping.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::driver::ResultSet;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OutlineTable {
    pub name: String,
    pub view: bool,
    /// (name, type) in column order
    pub columns: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Outline {
    /// Databases (MySQL, SQLite) or schemas (PostgreSQL) to complete after nothing or before `.`
    pub schemas: Vec<String>,
    pub tables: Vec<OutlineTable>,
    pub functions: Vec<String>,
}

impl Outline {
    pub fn table(&self, name: &str) -> Option<&OutlineTable> {
        self.tables.iter().find(|t| t.name == name).or_else(|| {
            self.tables
                .iter()
                .find(|t| t.name.eq_ignore_ascii_case(name))
        })
    }
}

pub fn rows(sets: Vec<ResultSet>) -> Vec<Vec<String>> {
    sets.into_iter()
        .find(|s| !s.columns.is_empty())
        .map(|s| {
            s.rows
                .into_iter()
                .map(|r| r.into_iter().map(Option::unwrap_or_default).collect())
                .collect()
        })
        .unwrap_or_default()
}

pub fn group_columns(tables: &mut [OutlineTable], columns: Vec<Vec<String>>) {
    let mut by_table: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for c in columns {
        if let [table, name, ty, ..] = c.as_slice() {
            by_table
                .entry(table.clone())
                .or_default()
                .push((name.clone(), ty.clone()));
        }
    }
    for t in tables {
        t.columns = by_table.remove(&t.name).unwrap_or_default();
    }
}
