//! PostgreSQL SQL dialect.

use navidog_model::object::{ObjectKind, Scope};
use navidog_sql::{
    ColumnType, ContextSelection, Dialect, DialectFeatures, ExplainStyle, Lex, TableOptionStyle,
    TypeArgs,
};

#[derive(Debug)]
pub struct PostgresDialect;

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
    fn extra_keywords(&self) -> &'static [&'static str] {
        crate::words::KEYWORDS
    }
    fn extra_functions(&self) -> &'static [&'static str] {
        crate::words::FUNCTIONS
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
    fn context_selection(&self) -> ContextSelection {
        ContextSelection::Schema
    }
    fn features(&self) -> DialectFeatures {
        DialectFeatures {
            has_schemas: true,
            folds_unquoted: true,
            console_boxed_tables: true,
            console_errors_have_code: false,
            like_text_cast: true,
            supports_unlogged: true,
            separate_comments: true,
            enriches_from_catalog: true,
            supports_identity: true,
            text_rejects_nul: true,
            case_insensitive_like: true,
            supports_virtual_generated: false,
            supports_exclude: true,
            supports_partitions: true,
            ..DialectFeatures::default()
        }
    }
    fn table_option_style(&self) -> TableOptionStyle {
        TableOptionStyle::StorageParameters
    }
    fn table_option_keys(&self) -> &'static [&'static str] {
        crate::words::TABLE_OPTIONS
    }
    fn explain_style(&self) -> ExplainStyle {
        ExplainStyle::JsonNodes
    }
    fn column_types(&self) -> &'static [ColumnType] {
        COLUMN_TYPES
    }
    fn default_column_type(&self) -> &'static str {
        "integer"
    }
    fn added_column_type(&self) -> &'static str {
        "text"
    }
    fn index_methods(&self) -> &'static [&'static str] {
        &["BTREE", "HASH", "GIN", "GIST", "BRIN"]
    }
    fn default_schema(&self) -> Option<&'static str> {
        Some("public")
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
    fn restore_search_path_sql(&self, path: &str) -> Option<String> {
        Some(format!(
            "SELECT pg_catalog.set_config('search_path', {}, false)",
            self.string_literal(path)
        ))
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

const LEX: Lex = Lex::POSTGRES;

// Names are what PostgreSQL `format_type` prints. Synonyms collapse onto those names.
const COLUMN_TYPES: &[ColumnType] = &[
    ColumnType {
        name: "integer",
        synonyms: &["INT", "INT4"],
        args: TypeArgs::None,
    },
    ColumnType {
        name: "bigint",
        synonyms: &["INT8"],
        args: TypeArgs::None,
    },
    ColumnType {
        name: "smallint",
        synonyms: &["INT2"],
        args: TypeArgs::None,
    },
    ColumnType {
        name: "character varying",
        synonyms: &["VARCHAR"],
        args: TypeArgs::Length,
    },
    ColumnType {
        name: "character",
        synonyms: &["CHAR"],
        args: TypeArgs::Length,
    },
    ColumnType {
        name: "text",
        synonyms: &[],
        args: TypeArgs::None,
    },
    ColumnType {
        name: "numeric",
        synonyms: &["DECIMAL"],
        args: TypeArgs::LengthAndScale,
    },
    ColumnType {
        name: "real",
        synonyms: &["FLOAT4"],
        args: TypeArgs::None,
    },
    ColumnType {
        name: "double precision",
        synonyms: &["FLOAT8"],
        args: TypeArgs::None,
    },
    ColumnType {
        name: "boolean",
        synonyms: &["BOOL"],
        args: TypeArgs::None,
    },
    ColumnType {
        name: "date",
        synonyms: &[],
        args: TypeArgs::None,
    },
    ColumnType {
        name: "time without time zone",
        synonyms: &["TIME"],
        args: TypeArgs::Length,
    },
    ColumnType {
        name: "time with time zone",
        synonyms: &["TIMETZ"],
        args: TypeArgs::Length,
    },
    ColumnType {
        name: "timestamp without time zone",
        synonyms: &["TIMESTAMP"],
        args: TypeArgs::Length,
    },
    ColumnType {
        name: "timestamp with time zone",
        synonyms: &["TIMESTAMPTZ"],
        args: TypeArgs::Length,
    },
    ColumnType {
        name: "bytea",
        synonyms: &["BLOB"],
        args: TypeArgs::None,
    },
    ColumnType {
        name: "json",
        synonyms: &[],
        args: TypeArgs::None,
    },
];
