//! SQLite SQL dialect.

use navidog_model::object::{ObjectKind, Scope};
use navidog_sql::{
    ContextSelection, Dialect, ExplainStyle, Lex, TableOptionStyle, rename_create_table,
};

#[derive(Debug)]
pub struct SqliteDialect;

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
    fn table_option_style(&self) -> TableOptionStyle {
        TableOptionStyle::TableFlags
    }
    fn bool_table_options(&self) -> &'static [&'static str] {
        &["without_rowid", "strict"]
    }
    fn explain_style(&self) -> ExplainStyle {
        ExplainStyle::QueryPlanRows
    }
    fn supports_column_comments(&self) -> bool {
        false
    }
    fn autoincrement_needs_integer_key(&self) -> bool {
        true
    }
    fn supports_index_method(&self) -> bool {
        false
    }
    fn fk_reference_schema(&self) -> bool {
        false
    }
    fn table_rebuild_catalog(&self) -> Option<&'static str> {
        Some("main")
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
        let create = rename_create_table(ddl?, old, &to, self.lex())?;
        let mut out = vec![create];
        if with_data {
            out.push(format!("INSERT INTO {to} SELECT * FROM {from}"));
        }
        Some(out)
    }
}

const LEX: Lex = Lex::SQLITE;
