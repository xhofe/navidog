//! MySQL and MariaDB SQL dialect. Both drivers share this implementation.

use navidog_model::object::{ObjectKind, Scope};
use navidog_sql::{ContextSelection, Dialect, ExplainStyle, Lex, TableOptionStyle};

#[derive(Debug)]
pub struct MysqlDialect;

impl Dialect for MysqlDialect {
    fn id(&self) -> &'static str {
        "mysql"
    }
    fn lex(&self) -> Lex {
        LEX
    }
    fn quote_ident(&self, name: &str) -> String {
        format!("`{}`", name.replace('`', "``"))
    }
    fn string_literal(&self, value: &str) -> String {
        let escaped = value.replace('\'', "''");
        format!("'{}'", escaped.replace('\\', "\\\\"))
    }
    fn qualify_prefix<'a>(&self, scope: &'a Scope) -> Option<&'a str> {
        Some(scope.database.as_str())
    }
    fn object_kinds(&self) -> &'static [ObjectKind] {
        &[
            ObjectKind::Table,
            ObjectKind::View,
            ObjectKind::Function,
            ObjectKind::Event,
        ]
    }
    fn begin_transaction(&self) -> &'static str {
        "START TRANSACTION"
    }
    fn is_system_database(&self, name: &str) -> bool {
        matches!(
            name,
            "information_schema" | "mysql" | "performance_schema" | "sys"
        )
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
    fn limit_unkeyed_writes(&self) -> bool {
        true
    }
    fn like_backslash_escape(&self) -> bool {
        true
    }
    fn supports_column_position(&self) -> bool {
        true
    }
    fn standalone_indexes(&self) -> bool {
        false
    }
    fn supports_zerofill(&self) -> bool {
        true
    }
    fn supports_on_update(&self) -> bool {
        true
    }
    fn transactional_ddl(&self) -> bool {
        false
    }
    fn ident_quote(&self) -> char {
        '`'
    }
    fn drop_index_alters_table(&self) -> bool {
        true
    }
    fn rewrites_columns(&self) -> bool {
        true
    }
    fn inline_table_options(&self) -> bool {
        true
    }
    fn drop_constraint_clause(&self, kind: &str, name: &str) -> Option<String> {
        Some(match kind {
            "PRIMARY" => "DROP PRIMARY KEY".into(),
            "FOREIGN" => format!("DROP FOREIGN KEY {}", self.quote_ident(name)),
            "INDEX" => format!("DROP INDEX {}", self.quote_ident(name)),
            _ => return None,
        })
    }
    fn context_selection(&self) -> ContextSelection {
        ContextSelection::Database
    }
    fn table_option_style(&self) -> TableOptionStyle {
        TableOptionStyle::Clauses
    }
    fn table_option_keys(&self) -> &'static [&'static str] {
        crate::words::TABLE_OPTIONS
    }
    fn explain_style(&self) -> ExplainStyle {
        ExplainStyle::TableAndJson
    }
    fn supports_profile(&self) -> bool {
        true
    }
    fn counts_changed_rows(&self) -> bool {
        true
    }
    fn switches_database(&self) -> bool {
        true
    }
    fn column_comment_inline(&self) -> bool {
        true
    }
    fn supports_index_comment(&self) -> bool {
        true
    }
    fn supports_index_expression(&self) -> bool {
        false
    }
    fn supports_column_charset(&self) -> bool {
        true
    }
    fn supports_unsigned(&self) -> bool {
        true
    }
    fn index_methods(&self) -> &'static [&'static str] {
        &["BTREE", "HASH"]
    }
    fn session_context_sql(&self) -> Option<&'static str> {
        Some("SELECT DATABASE()")
    }
    fn prepare_database_sql(&self, database: &str) -> Option<String> {
        (!database.is_empty()).then(|| format!("USE {}", self.quote_ident(database)))
    }
    fn console_prompt(&self, _: &str, _: bool, continuation: bool) -> String {
        if continuation {
            "    ->".into()
        } else {
            "mysql>".into()
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
            ObjectKind::Table | ObjectKind::View => Some(format!(
                "RENAME TABLE {} TO {}",
                self.qualified(scope, old),
                self.qualified(scope, new)
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
        _: Option<&str>,
    ) -> Option<Vec<String>> {
        let (from, to) = (self.qualified(scope, old), self.qualified(scope, new));
        let mut out = vec![format!("CREATE TABLE {to} LIKE {from}")];
        if with_data {
            out.push(format!("INSERT INTO {to} SELECT * FROM {from}"));
        }
        Some(out)
    }
}

const LEX: Lex = Lex::MYSQL;
