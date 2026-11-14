//! SQLite SQL dialect.

use navidog_model::object::{ObjectKind, Scope};
use navidog_sql::{
    ColumnType, ContextSelection, Dialect, DialectFeatures, ExplainStyle, Lex, TableOptionStyle,
    TypeArgs, rename_create_table,
};

#[derive(Debug)]
pub struct SqliteDialect;

impl Dialect for SqliteDialect {
    fn id(&self) -> &str {
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
    fn object_kinds(&self) -> &[ObjectKind] {
        &[
            ObjectKind::Table,
            ObjectKind::View,
            ObjectKind::Index,
            ObjectKind::Trigger,
        ]
    }
    fn definition_family(&self) -> Option<navidog_sql::definition::DefinitionFamily> {
        Some(navidog_sql::definition::DefinitionFamily::Sqlite)
    }
    fn master_table_sql(&self, database: &str) -> Option<String> {
        Some(format!(
            "SELECT * FROM {}.sqlite_master",
            self.quote_ident(database)
        ))
    }
    fn definition_statements(
        &self,
        scope: &navidog_model::object::Scope,
        draft: &navidog_sql::definition::DefinitionDraft,
    ) -> Result<Vec<String>, String> {
        navidog_sql::definition::plan(
            self,
            navidog_sql::definition::DefinitionFamily::Sqlite,
            scope,
            draft,
        )
    }
    fn begin_transaction(&self) -> &str {
        "BEGIN"
    }
    fn is_system_database(&self, _: &str) -> bool {
        false
    }
    fn is_system_schema(&self, _: &str) -> bool {
        false
    }
    fn is_system_object(&self, name: &str) -> bool {
        name.starts_with("sqlite_")
    }
    fn extra_keywords(&self) -> &[&str] {
        crate::words::KEYWORDS
    }
    fn extra_functions(&self) -> &[&str] {
        crate::words::FUNCTIONS
    }
    fn autoincrement_keyword(&self) -> &str {
        "AUTOINCREMENT"
    }
    fn context_selection(&self) -> ContextSelection {
        ContextSelection::File
    }
    fn features(&self) -> DialectFeatures {
        DialectFeatures {
            exposes_rowid: true,
            drop_index_by_name: true,
            drop_trigger_by_name: true,
            begin_immediate: true,
            rebuilds_tables: true,
            supports_column_comments: false,
            autoincrement_needs_integer_key: true,
            supports_index_method: false,
            fk_reference_schema: false,
            ..DialectFeatures::default()
        }
    }
    fn table_option_style(&self) -> TableOptionStyle {
        TableOptionStyle::TableFlags
    }
    fn bool_table_options(&self) -> &[&str] {
        &["without_rowid", "strict"]
    }
    fn explain_style(&self) -> ExplainStyle {
        ExplainStyle::QueryPlanRows
    }
    fn column_types(&self) -> &[ColumnType] {
        &COLUMN_TYPES
    }
    fn table_rebuild_catalog(&self) -> Option<&str> {
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

// SQLite stores the declared type text, so these spellings come back unchanged.
// `int` becomes `INTEGER` because only that exact name aliases rowid.
static COLUMN_TYPES: std::sync::LazyLock<Vec<ColumnType>> = std::sync::LazyLock::new(|| {
    vec![
        ColumnType {
            name: std::borrow::Cow::Borrowed("INTEGER"),
            synonyms: vec!["INT".into()],
            args: TypeArgs::None,
        },
        ColumnType {
            name: std::borrow::Cow::Borrowed("BIGINT"),
            synonyms: vec![],
            args: TypeArgs::None,
        },
        ColumnType {
            name: std::borrow::Cow::Borrowed("SMALLINT"),
            synonyms: vec![],
            args: TypeArgs::None,
        },
        ColumnType {
            name: std::borrow::Cow::Borrowed("VARCHAR"),
            synonyms: vec![],
            args: TypeArgs::Length,
        },
        ColumnType {
            name: std::borrow::Cow::Borrowed("CHAR"),
            synonyms: vec![],
            args: TypeArgs::Length,
        },
        ColumnType {
            name: std::borrow::Cow::Borrowed("TEXT"),
            synonyms: vec![],
            args: TypeArgs::None,
        },
        ColumnType {
            name: std::borrow::Cow::Borrowed("DECIMAL"),
            synonyms: vec![],
            args: TypeArgs::LengthAndScale,
        },
        ColumnType {
            name: std::borrow::Cow::Borrowed("NUMERIC"),
            synonyms: vec![],
            args: TypeArgs::LengthAndScale,
        },
        ColumnType {
            name: std::borrow::Cow::Borrowed("REAL"),
            synonyms: vec![],
            args: TypeArgs::None,
        },
        ColumnType {
            name: std::borrow::Cow::Borrowed("DOUBLE PRECISION"),
            synonyms: vec![],
            args: TypeArgs::None,
        },
        ColumnType {
            name: std::borrow::Cow::Borrowed("BOOLEAN"),
            synonyms: vec![],
            args: TypeArgs::None,
        },
        ColumnType {
            name: std::borrow::Cow::Borrowed("DATE"),
            synonyms: vec![],
            args: TypeArgs::None,
        },
        ColumnType {
            name: std::borrow::Cow::Borrowed("TIME"),
            synonyms: vec![],
            args: TypeArgs::Length,
        },
        ColumnType {
            name: std::borrow::Cow::Borrowed("TIMESTAMP"),
            synonyms: vec![],
            args: TypeArgs::Length,
        },
        ColumnType {
            name: std::borrow::Cow::Borrowed("BLOB"),
            synonyms: vec![],
            args: TypeArgs::None,
        },
        ColumnType {
            name: std::borrow::Cow::Borrowed("JSON"),
            synonyms: vec![],
            args: TypeArgs::None,
        },
    ]
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_master_table_is_qualified() {
        assert_eq!(
            SqliteDialect.master_table_sql("main").as_deref(),
            Some("SELECT * FROM \"main\".sqlite_master")
        );
    }
}
