//! SQLite SQL dialect.

use std::sync::Arc;

use navidog_model::object::{ObjectKind, Scope};
use navidog_sql::{ContextSelection, Dialect, Lex, rename_create_table};

pub struct SqliteDialect;

pub fn dialect() -> Arc<dyn Dialect> {
    Arc::new(SqliteDialect)
}

impl Dialect for SqliteDialect {
    fn id(&self) -> &'static str {
        "sqlite"
    }
    fn lex(&self) -> Lex {
        LEX
    }
    fn quote_ident(&self, name: &str) -> String {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
    fn string_literal(&self, value: &str) -> String {
        format!("'{}'", value.replace('\'', "''"))
    }
    fn qualify_prefix<'a>(&self, scope: &'a Scope) -> Option<&'a str> {
        Some(scope.database.as_str())
    }
    fn object_kinds(&self) -> &'static [ObjectKind] {
        &[
            ObjectKind::Table,
            ObjectKind::View,
            ObjectKind::Index,
            ObjectKind::Trigger,
        ]
    }
    fn begin_transaction(&self) -> &'static str {
        "BEGIN"
    }
    fn is_system_database(&self, _: &str) -> bool {
        false
    }
    fn is_system_schema(&self, _: &str) -> bool {
        false
    }
    fn has_schemas(&self) -> bool {
        false
    }
    fn extra_keywords(&self) -> &'static [&'static str] {
        crate::words::KEYWORDS
    }
    fn extra_functions(&self) -> &'static [&'static str] {
        crate::words::FUNCTIONS
    }
    fn exposes_rowid(&self) -> bool {
        true
    }
    fn drops_index_by_name(&self) -> bool {
        true
    }
    fn drops_trigger_by_name(&self) -> bool {
        true
    }
    fn begin_immediate(&self) -> bool {
        true
    }
    fn rebuilds_tables(&self) -> bool {
        true
    }
    fn autoincrement_keyword(&self) -> &'static str {
        "AUTOINCREMENT"
    }
    fn context_selection(&self) -> ContextSelection {
        ContextSelection::File
    }
    fn console_prompt(&self, _: &str, _: bool, continuation: bool) -> String {
        if continuation {
            "   ...>".into()
        } else {
            "sqlite>".into()
        }
    }
    fn rename_object(
        &self,
        scope: &Scope,
        kind: ObjectKind,
        old: &str,
        new: &str,
    ) -> Option<String> {
        match kind {
            ObjectKind::Table => Some(format!(
                "ALTER TABLE {} RENAME TO {}",
                self.qualified(scope, old),
                self.quote_ident(new)
            )),
            _ => None,
        }
    }
    fn duplicate_table(
        &self,
        scope: &Scope,
        old: &str,
        new: &str,
        with_data: bool,
        ddl: Option<&str>,
    ) -> Option<Vec<String>> {
        let (from, to) = (self.qualified(scope, old), self.qualified(scope, new));
        let create = rename_create_table(ddl?, old, &to)?;
        let mut out = vec![create];
        if with_data {
            out.push(format!("INSERT INTO {to} SELECT * FROM {from}"));
        }
        Some(out)
    }
}

const LEX: Lex = Lex {
    backtick_idents: true,
    hash_comments: false,
    dollar_quotes: false,
    bracket_idents: true,
    backslash_strings: false,
    escape_string_prefix: false,
    double_quote_strings: false,
    nested_block_comments: false,
    delimiter_command: false,
};

#[cfg(test)]
mod tests {
    use super::*;
    use navidog_model::object::Scope;
    use navidog_sql::dialect::ChangeSet;

    #[test]
    fn quotes_rowid_and_rewrites_create_table() {
        let scope = Scope::new("c", "app", None);
        assert_eq!(SqliteDialect.literal(None), "NULL");
        assert_eq!(SqliteDialect.typed_name("名称"), "\"名称\"");
        assert!(
            SqliteDialect
                .rename_object(&scope, ObjectKind::View, "v", "w")
                .is_none()
        );
        let ddl = "CREATE TABLE IF NOT EXISTS \"actor\" (id INTEGER PRIMARY KEY)";
        let lite = SqliteDialect
            .duplicate_table(&scope, "actor", "actor_copy1", false, Some(ddl))
            .unwrap();
        assert_eq!(
            lite,
            ["CREATE TABLE IF NOT EXISTS \"app\".\"actor_copy1\" (id INTEGER PRIMARY KEY)"]
        );
        assert_eq!(
            SqliteDialect.select_page(&scope, "t", "", "", None, 0, true),
            "SELECT *, rowid AS \"_rowid\" FROM \"app\".\"t\""
        );
        let _ = ChangeSet::default();
    }
}
