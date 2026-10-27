//! MySQL and MariaDB SQL dialect. Both drivers share this implementation.

use navidog_model::object::{ObjectKind, Scope};
use navidog_sql::{
    ConstraintKind, ContextSelection, Dialect, DialectFeatures, ExplainStyle, Lex, TableOptionStyle,
};

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
    fn extra_keywords(&self) -> &'static [&'static str] {
        crate::words::KEYWORDS
    }
    fn extra_functions(&self) -> &'static [&'static str] {
        crate::words::FUNCTIONS
    }
    fn ident_quote(&self) -> char {
        '`'
    }
    fn drop_constraint_clause(&self, kind: ConstraintKind, name: &str) -> Option<String> {
        Some(match kind {
            ConstraintKind::Primary => "DROP PRIMARY KEY".into(),
            ConstraintKind::ForeignKey => format!("DROP FOREIGN KEY {}", self.quote_ident(name)),
            ConstraintKind::Index => format!("DROP INDEX {}", self.quote_ident(name)),
            // MySQL drops UNIQUE and CHECK constraints through DROP INDEX / DROP CHECK, which
            // this designer does not model; an EXCLUDE constraint does not exist there.
            ConstraintKind::Unique | ConstraintKind::Check | ConstraintKind::Exclude => {
                return None;
            }
        })
    }
    fn context_selection(&self) -> ContextSelection {
        ContextSelection::Database
    }
    fn features(&self) -> DialectFeatures {
        DialectFeatures {
            limit_unkeyed_writes: true,
            supports_column_position: true,
            standalone_indexes: false,
            supports_zerofill: true,
            supports_on_update: true,
            transactional_ddl: false,
            drop_index_alters_table: true,
            rewrites_columns: true,
            inline_table_options: true,
            supports_profile: true,
            counts_changed_rows: true,
            switches_database: true,
            column_comment_inline: true,
            supports_index_comment: true,
            supports_column_charset: true,
            supports_unsigned: true,
            supports_index_expression: false,
            ..DialectFeatures::default()
        }
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
