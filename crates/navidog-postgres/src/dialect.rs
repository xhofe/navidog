//! PostgreSQL SQL dialect.

use std::sync::Arc;

use navidog_model::object::{ObjectKind, Scope};
use navidog_sql::{ContextSelection, Dialect, Lex};

pub struct PostgresDialect;

pub fn dialect() -> Arc<dyn Dialect> {
    Arc::new(PostgresDialect)
}

impl Dialect for PostgresDialect {
    fn id(&self) -> &'static str {
        "postgres"
    }
    fn lex(&self) -> Lex {
        LEX
    }
    fn quote_ident(&self, name: &str) -> String {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
    fn string_literal(&self, value: &str) -> String {
        let escaped = value.replace('\'', "''");
        if value.contains('\\') {
            format!("E'{}'", escaped.replace('\\', "\\\\"))
        } else {
            format!("'{escaped}'")
        }
    }
    fn qualify_prefix<'a>(&self, scope: &'a Scope) -> Option<&'a str> {
        scope.schema.as_deref()
    }
    fn object_kinds(&self) -> &'static [ObjectKind] {
        &[
            ObjectKind::Table,
            ObjectKind::View,
            ObjectKind::MaterializedView,
            ObjectKind::Function,
            ObjectKind::Sequence,
        ]
    }
    fn begin_transaction(&self) -> &'static str {
        "BEGIN"
    }
    fn is_system_database(&self, _: &str) -> bool {
        false
    }
    fn is_system_schema(&self, name: &str) -> bool {
        matches!(name, "pg_catalog" | "information_schema")
    }
    fn has_schemas(&self) -> bool {
        true
    }
    fn extra_keywords(&self) -> &'static [&'static str] {
        crate::words::KEYWORDS
    }
    fn extra_functions(&self) -> &'static [&'static str] {
        crate::words::FUNCTIONS
    }
    fn folds_unquoted(&self) -> bool {
        true
    }
    fn console_psql_grid(&self) -> bool {
        true
    }
    fn like_text_cast(&self) -> bool {
        true
    }
    fn supports_unlogged(&self) -> bool {
        true
    }
    fn separate_comments(&self) -> bool {
        true
    }
    fn generated_default_storage(&self) -> &'static str {
        "STORED"
    }
    fn exclusive_lock_sql(&self, qualified: &str) -> Option<String> {
        Some(format!("LOCK TABLE {qualified} IN ACCESS EXCLUSIVE MODE"))
    }
    fn table_owner_sql(&self, table: &str, owner: &str) -> Option<String> {
        Some(format!(
            "ALTER TABLE {table} OWNER TO {}",
            self.quote_ident(owner)
        ))
    }
    fn enriches_from_catalog(&self) -> bool {
        true
    }
    fn context_selection(&self) -> ContextSelection {
        ContextSelection::Schema
    }
    fn session_context_sql(&self) -> Option<&'static str> {
        Some(
            "SELECT pg_catalog.current_database(), pg_catalog.current_schema(), \
             pg_catalog.current_setting('search_path')",
        )
    }
    fn set_search_path_sql(&self, schema: &str) -> Option<String> {
        Some(format!("SET search_path TO {}", self.quote_ident(schema)))
    }
    fn console_prompt(&self, database: &str, superuser: bool, continuation: bool) -> String {
        let mark = if superuser { '#' } else { '>' };
        format!("{database}{}{mark}", if continuation { '-' } else { '=' })
    }
    fn rename_object(
        &self,
        scope: &Scope,
        kind: ObjectKind,
        old: &str,
        new: &str,
    ) -> Option<String> {
        let (from, to) = (self.qualified(scope, old), self.quote_ident(new));
        Some(match kind {
            ObjectKind::Table => format!("ALTER TABLE {from} RENAME TO {to}"),
            ObjectKind::View => format!("ALTER VIEW {from} RENAME TO {to}"),
            ObjectKind::MaterializedView => {
                format!("ALTER MATERIALIZED VIEW {from} RENAME TO {to}")
            }
            ObjectKind::Sequence => format!("ALTER SEQUENCE {from} RENAME TO {to}"),
            _ => return None,
        })
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
        let mut out = vec![format!("CREATE TABLE {to} (LIKE {from} INCLUDING ALL)")];
        if with_data {
            out.push(format!("INSERT INTO {to} SELECT * FROM {from}"));
        }
        Some(out)
    }
}

const LEX: Lex = Lex {
    backtick_idents: true,
    hash_comments: false,
    dollar_quotes: true,
    bracket_idents: false,
    backslash_strings: false,
    escape_string_prefix: true,
    double_quote_strings: false,
    nested_block_comments: true,
    delimiter_command: false,
};

#[cfg(test)]
mod tests {
    use super::*;
    use navidog_model::object::Scope;

    #[test]
    fn quotes_escape_strings_and_folds_names() {
        assert_eq!(PostgresDialect.quote_ident("a\"b"), "\"a\"\"b\"");
        assert_eq!(PostgresDialect.literal(Some(r"it's \n")), r"E'it''s \\n'");
        assert_eq!(PostgresDialect.literal(Some("it's")), "'it''s'");
        let scope = Scope::new("c", "db", Some("public".into()));
        assert_eq!(PostgresDialect.qualified(&scope, "t"), "\"public\".\"t\"");
        assert_eq!(
            PostgresDialect
                .rename_object(&scope, ObjectKind::View, "v", "w")
                .unwrap(),
            "ALTER VIEW \"public\".\"v\" RENAME TO \"w\""
        );
        assert!(
            PostgresDialect
                .rename_object(&scope, ObjectKind::Function, "f", "g")
                .is_none()
        );
        assert_eq!(PostgresDialect.typed_name("Actor"), "\"Actor\"");
        assert_eq!(
            PostgresDialect.console_prompt("pagila", true, false),
            "pagila=#"
        );
    }
}
