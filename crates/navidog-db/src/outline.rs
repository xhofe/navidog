//! Pure outline fetch: tables, views, functions and columns of one database/schema.

use std::collections::HashMap;

use navidog_core::object::Scope;
use serde::{Deserialize, Serialize};

use crate::dialect::Dialect;
use crate::driver::{DbResult, Relational, ResultSet};

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

fn rows(sets: Vec<ResultSet>) -> Vec<Vec<String>> {
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

fn group_columns(tables: &mut [OutlineTable], columns: Vec<Vec<String>>) {
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

/// Fetches the outline of a scope. `system` adds the system databases or schemas (F-PREF-006).
pub async fn fetch(rel: &mut dyn Relational, scope: &Scope, system: bool) -> DbResult<Outline> {
    let d = rel.dialect();
    let lit = |s: &str| d.string_literal(s);
    let mut out = Outline::default();
    match d {
        Dialect::MySql => {
            let db = lit(&scope.database);
            out.tables = rows(
                rel.execute_read(&format!(
                    "SELECT TABLE_NAME, TABLE_TYPE FROM information_schema.TABLES \
                     WHERE TABLE_SCHEMA = {db} ORDER BY TABLE_NAME"
                ))
                .await?,
            )
            .into_iter()
            .map(|r| OutlineTable {
                name: r[0].clone(),
                view: r.get(1).is_some_and(|t| t.contains("VIEW")),
                columns: Vec::new(),
            })
            .collect();
            let columns = rows(
                rel.execute_read(&format!(
                    "SELECT TABLE_NAME, COLUMN_NAME, COLUMN_TYPE FROM information_schema.COLUMNS \
                     WHERE TABLE_SCHEMA = {db} ORDER BY TABLE_NAME, ORDINAL_POSITION"
                ))
                .await?,
            );
            group_columns(&mut out.tables, columns);
            out.functions = rows(
                rel.execute_read(&format!(
                    "SELECT ROUTINE_NAME FROM information_schema.ROUTINES \
                     WHERE ROUTINE_SCHEMA = {db} ORDER BY ROUTINE_NAME"
                ))
                .await?,
            )
            .into_iter()
            .map(|r| r[0].clone())
            .collect();
            out.schemas = rel.list_databases().await?;
            if !system {
                out.schemas.retain(|s| !d.is_system_database(s));
            }
        }
        Dialect::Postgres => {
            let schema = scope.schema.clone().unwrap_or_else(|| "public".into());
            let mut wanted = vec![lit(&schema)];
            if system {
                wanted.extend([lit("pg_catalog"), lit("information_schema")]);
            }
            let list = wanted.join(", ");
            out.tables = rows(
                rel.execute_read(&format!(
                    "SELECT c.relname, c.relkind FROM pg_class c \
                     JOIN pg_namespace n ON n.oid = c.relnamespace \
                     WHERE n.nspname IN ({list}) AND c.relkind IN ('r', 'p', 'v', 'm', 'f') \
                     ORDER BY c.relname"
                ))
                .await?,
            )
            .into_iter()
            .map(|r| OutlineTable {
                name: r[0].clone(),
                view: matches!(r.get(1).map(String::as_str), Some("v" | "m")),
                columns: Vec::new(),
            })
            .collect();
            let columns = rows(
                rel.execute_read(&format!(
                    "SELECT c.relname, a.attname, format_type(a.atttypid, a.atttypmod) \
                     FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid \
                     JOIN pg_namespace n ON n.oid = c.relnamespace \
                     WHERE n.nspname IN ({list}) AND a.attnum > 0 AND NOT a.attisdropped \
                     AND c.relkind IN ('r', 'p', 'v', 'm', 'f') ORDER BY c.relname, a.attnum"
                ))
                .await?,
            );
            group_columns(&mut out.tables, columns);
            out.functions = rows(
                rel.execute_read(&format!(
                    "SELECT DISTINCT p.proname FROM pg_proc p \
                     JOIN pg_namespace n ON n.oid = p.pronamespace \
                     WHERE n.nspname IN ({list}) ORDER BY p.proname"
                ))
                .await?,
            )
            .into_iter()
            .map(|r| r[0].clone())
            .collect();
            out.schemas = rel.list_schemas().await?;
            if !system {
                out.schemas.retain(|s| !d.is_system_schema(s));
            }
        }
        Dialect::Sqlite => {
            let db = d.quote_ident(&scope.database);
            let dbl = lit(&scope.database);
            out.tables = rows(
                rel.execute_read(&format!(
                    "SELECT name, type FROM {db}.sqlite_schema \
                     WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' \
                     ORDER BY name"
                ))
                .await?,
            )
            .into_iter()
            .map(|r| OutlineTable {
                name: r[0].clone(),
                view: r.get(1).is_some_and(|t| t == "view"),
                columns: Vec::new(),
            })
            .collect();
            let columns = rows(
                rel.execute_read(&format!(
                    "SELECT m.name, p.name, p.type FROM {db}.sqlite_schema m \
                     JOIN pragma_table_xinfo(m.name, {dbl}) p \
                     WHERE m.type IN ('table', 'view') AND m.name NOT LIKE 'sqlite\\_%' ESCAPE '\\' \
                     ORDER BY m.name, p.cid"
                ))
                .await?,
            );
            group_columns(&mut out.tables, columns);
            out.schemas = rel.list_databases().await?;
        }
    }
    Ok(out)
}
