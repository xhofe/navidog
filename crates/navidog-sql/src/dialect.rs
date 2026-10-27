//! SQL dialects: identifier quoting, literals, paged queries and SQL generation for grid change
//! sets (21-table-viewer.md §2).

use navidog_model::object::{ObjectKind, Scope};

use crate::span::Lex;

/// How a session's current namespace is chosen. Feature code branches on this, not on a product name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextSelection {
    /// `USE` changes the current database.
    Database,
    /// The database is fixed for the session; `search_path` changes the schema.
    Schema,
    /// A file database has no server-side current catalog.
    File,
}

/// One database product's SQL rules. Implementations live in the per-database crates.
pub trait Dialect: std::fmt::Debug + Send + Sync {
    fn id(&self) -> &'static str;
    fn lex(&self) -> Lex;
    fn quote_ident(&self, name: &str) -> String;
    fn string_literal(&self, value: &str) -> String;
    /// Database or schema prefixed before an object name, when the dialect wants one.
    fn qualify_prefix<'a>(&self, scope: &'a Scope) -> Option<&'a str>;
    fn object_kinds(&self) -> &'static [ObjectKind];
    fn begin_transaction(&self) -> &'static str;
    fn is_system_database(&self, name: &str) -> bool;
    fn is_system_schema(&self, name: &str) -> bool;
    fn has_schemas(&self) -> bool;
    fn extra_keywords(&self) -> &'static [&'static str];
    fn extra_functions(&self) -> &'static [&'static str];
    fn rename_object(
        &self,
        scope: &Scope,
        kind: ObjectKind,
        old: &str,
        new: &str,
    ) -> Option<String>;
    fn duplicate_table(
        &self,
        scope: &Scope,
        old: &str,
        new: &str,
        with_data: bool,
        ddl: Option<&str>,
    ) -> Option<Vec<String>>;
    fn console_prompt(&self, database: &str, superuser: bool, continuation: bool) -> String;
    fn context_selection(&self) -> ContextSelection;
    /// Query that reads the session's current database, and for [`ContextSelection::Schema`]
    /// also the schema and `search_path`. `None` when the product has no such state.
    fn session_context_sql(&self) -> Option<&'static str> {
        None
    }
    fn prepare_database_sql(&self, _database: &str) -> Option<String> {
        None
    }
    fn set_search_path_sql(&self, _schema: &str) -> Option<String> {
        None
    }
    /// Put back a recorded `search_path` setting, which may be a comma-separated list.
    /// [`set_search_path_sql`](Dialect::set_search_path_sql) quotes one schema name and would
    /// treat that list as a single identifier.
    fn restore_search_path_sql(&self, _path: &str) -> Option<String> {
        None
    }

    fn exposes_rowid(&self) -> bool {
        false
    }
    fn limit_unkeyed_writes(&self) -> bool {
        false
    }
    fn folds_unquoted(&self) -> bool {
        false
    }
    fn drops_index_by_name(&self) -> bool {
        false
    }
    fn drops_trigger_by_name(&self) -> bool {
        false
    }
    fn console_psql_grid(&self) -> bool {
        false
    }
    fn like_text_cast(&self) -> bool {
        false
    }
    fn like_backslash_escape(&self) -> bool {
        false
    }
    fn supports_unlogged(&self) -> bool {
        false
    }
    fn supports_column_position(&self) -> bool {
        false
    }
    fn separate_comments(&self) -> bool {
        false
    }
    /// Indexes are their own `CREATE INDEX` statements, not clauses of `ALTER TABLE`.
    fn standalone_indexes(&self) -> bool {
        true
    }
    fn supports_zerofill(&self) -> bool {
        false
    }
    fn supports_on_update(&self) -> bool {
        false
    }
    fn transactional_ddl(&self) -> bool {
        true
    }
    fn ident_quote(&self) -> char {
        '"'
    }
    fn drop_index_alters_table(&self) -> bool {
        false
    }
    /// `BEGIN IMMEDIATE` rather than `BEGIN` when applying DDL.
    fn begin_immediate(&self) -> bool {
        false
    }
    /// MySQL drops constraints with `DROP PRIMARY KEY` / `DROP FOREIGN KEY` / `DROP INDEX`.
    /// `None` means this designer does not know a safe statement for that kind.
    fn drop_constraint_clause(&self, _kind: &str, name: &str) -> Option<String> {
        Some(format!("DROP CONSTRAINT {}", self.quote_ident(name)))
    }
    /// One `CHANGE COLUMN` rewrites the name, type and position together.
    fn rewrites_columns(&self) -> bool {
        false
    }
    /// Table options are clauses of one `ALTER TABLE` (engine, charset), not storage parameters.
    fn inline_table_options(&self) -> bool {
        false
    }
    /// Changing an existing table means rebuilding it.
    fn rebuilds_tables(&self) -> bool {
        false
    }
    /// Statement that locks the table for the duration of a structure change.
    fn exclusive_lock_sql(&self, _qualified: &str) -> Option<String> {
        None
    }
    /// `ALTER TABLE ... OWNER TO`, when the product has table owners.
    fn table_owner_sql(&self, _table: &str, _owner: &str) -> Option<String> {
        None
    }
    /// Default storage of a generated column when the user does not pick one.
    fn generated_default_storage(&self) -> &'static str {
        "VIRTUAL"
    }
    fn autoincrement_keyword(&self) -> &'static str {
        "AUTO_INCREMENT"
    }
    /// After `SHOW`/`pg_get` DDL, read owner, persistence and column metadata from the catalog.
    fn enriches_from_catalog(&self) -> bool {
        false
    }

    fn literal(&self, value: Option<&str>) -> String {
        match value {
            None => "NULL".into(),
            Some(v) => self.string_literal(v),
        }
    }

    fn written_sql(&self, written: &Written) -> String {
        if written.raw
            && let Some(expr) = written
                .value
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
        {
            expr.to_string()
        } else {
            self.literal(written.value.as_deref())
        }
    }

    fn qualified(&self, scope: &Scope, name: &str) -> String {
        match self.qualify_prefix(scope) {
            Some(p) => format!("{}.{}", self.quote_ident(p), self.quote_ident(name)),
            None => self.quote_ident(name),
        }
    }

    fn drop_keyword(&self, kind: ObjectKind) -> Option<&'static str> {
        Some(match kind {
            ObjectKind::Table => "TABLE",
            ObjectKind::View => "VIEW",
            ObjectKind::MaterializedView => "MATERIALIZED VIEW",
            ObjectKind::Event => "EVENT",
            ObjectKind::Trigger if self.drops_trigger_by_name() => "TRIGGER",
            ObjectKind::Index if self.drops_index_by_name() => "INDEX",
            ObjectKind::Sequence => "SEQUENCE",
            _ => return None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn select_page(
        &self,
        scope: &Scope,
        table: &str,
        filter: &str,
        order: &str,
        limit: Option<u64>,
        offset: u64,
        rowid: bool,
    ) -> String {
        let list = if rowid && self.exposes_rowid() {
            "*, rowid AS \"_rowid\""
        } else {
            "*"
        };
        let mut sql = format!("SELECT {list} FROM {}", self.qualified(scope, table));
        if !filter.trim().is_empty() {
            sql.push_str(&format!(" WHERE {}", filter.trim()));
        }
        if !order.trim().is_empty() {
            sql.push_str(&format!(" ORDER BY {}", order.trim()));
        }
        if let Some(limit) = limit {
            sql.push_str(&format!(" LIMIT {limit} OFFSET {offset}"));
        }
        sql
    }

    fn count(&self, scope: &Scope, table: &str, filter: &str) -> String {
        let mut sql = format!("SELECT COUNT(*) FROM {}", self.qualified(scope, table));
        if !filter.trim().is_empty() {
            sql.push_str(&format!(" WHERE {}", filter.trim()));
        }
        sql
    }

    fn render_changes(
        &self,
        scope: &Scope,
        table: &str,
        changes: &ChangeSet,
        has_primary_key: bool,
    ) -> Vec<String> {
        let target = self.qualified(scope, table);
        let limit = if self.limit_unkeyed_writes() && !has_primary_key {
            " LIMIT 1"
        } else {
            ""
        };
        let mut out = Vec::new();
        for key in &changes.deletes {
            out.push(format!(
                "DELETE FROM {target} WHERE {}{limit}",
                assignments(self, &key.0, " AND ")
            ));
        }
        for u in &changes.updates {
            if u.values.is_empty() {
                continue;
            }
            out.push(format!(
                "UPDATE {target} SET {} WHERE {}{limit}",
                written_list(self, &u.values),
                assignments(self, &u.key.0, " AND ")
            ));
        }
        for row in &changes.inserts {
            let cols: Vec<_> = row.iter().map(|(c, _)| self.quote_ident(c)).collect();
            let vals: Vec<_> = row.iter().map(|(_, v)| self.written_sql(v)).collect();
            out.push(format!(
                "INSERT INTO {target} ({}) VALUES ({})",
                cols.join(", "),
                vals.join(", ")
            ));
        }
        out
    }

    fn insert_statements(
        &self,
        scope: &Scope,
        table: &str,
        columns: &[String],
        rows: &[Vec<Option<String>>],
    ) -> String {
        let target = self.qualified(scope, table);
        let cols: Vec<_> = columns.iter().map(|c| self.quote_ident(c)).collect();
        rows.iter()
            .map(|r| {
                let vals: Vec<_> = r.iter().map(|v| self.literal(v.as_deref())).collect();
                format!(
                    "INSERT INTO {target} ({}) VALUES ({});",
                    cols.join(", "),
                    vals.join(", ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn update_statements(
        &self,
        scope: &Scope,
        table: &str,
        columns: &[String],
        key_columns: &[String],
        rows: &[Vec<Option<String>>],
    ) -> String {
        let target = self.qualified(scope, table);
        rows.iter()
            .map(|r| {
                let (keys, sets): (Values, Values) = columns
                    .iter()
                    .cloned()
                    .zip(r.iter().cloned())
                    .partition(|(c, _)| key_columns.contains(c));
                let keys = if keys.is_empty() { sets.clone() } else { keys };
                format!(
                    "UPDATE {target} SET {} WHERE {};",
                    assignments(self, &sets, ", "),
                    assignments(self, &keys, " AND ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn keywords(&self) -> Vec<&'static str> {
        crate::keywords::COMMON
            .iter()
            .chain(self.extra_keywords())
            .copied()
            .collect()
    }

    fn functions(&self) -> Vec<&'static str> {
        crate::keywords::COMMON_FUNCTIONS
            .iter()
            .chain(self.extra_functions())
            .copied()
            .collect()
    }

    fn is_keyword(&self, word: &str) -> bool {
        self.keywords().iter().any(|k| k.eq_ignore_ascii_case(word))
    }

    fn typed_name(&self, name: &str) -> String {
        let plain = name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        let folds = self.folds_unquoted() && name.chars().any(|c| c.is_ascii_uppercase());
        if plain && !folds && !self.is_keyword(name) {
            name.to_string()
        } else {
            self.quote_ident(name)
        }
    }

    fn format_console_error(&self, code: Option<&str>, message: &str) -> String {
        if self.console_psql_grid() {
            format!("ERROR:  {message}\n")
        } else if let Some(code) = code {
            format!("ERROR {code}: {message}\n")
        } else {
            format!("ERROR: {message}\n")
        }
    }
}

fn written_list<D: Dialect + ?Sized>(dialect: &D, values: &Writes) -> String {
    values
        .iter()
        .map(|(c, v)| format!("{} = {}", dialect.quote_ident(c), dialect.written_sql(v)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn assignments<D: Dialect + ?Sized>(dialect: &D, values: &Values, sep: &str) -> String {
    values
        .iter()
        .map(|(c, v)| match (sep, v) {
            (" AND ", None) => format!("{} IS NULL", dialect.quote_ident(c)),
            _ => format!(
                "{} = {}",
                dialect.quote_ident(c),
                dialect.literal(v.as_deref())
            ),
        })
        .collect::<Vec<_>>()
        .join(sep)
}

/// Values of a row keyed by column name; `None` is NULL.
pub type Values = Vec<(String, Option<String>)>;

/// A value written by an edit or an insert. When `raw` is set and the text is non-empty, it is SQL
/// (raw data mode) and is not quoted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Written {
    pub value: Option<String>,
    pub raw: bool,
}

impl Written {
    pub fn text(value: Option<String>) -> Self {
        Self { value, raw: false }
    }

    pub fn expr(sql: impl Into<String>) -> Self {
        Self {
            value: Some(sql.into()),
            raw: true,
        }
    }
}

pub type Writes = Vec<(String, Written)>;

/// Locates a row: only the primary key columns when there is a primary key, otherwise the whole
/// row's old values.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RowKey(pub Values);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowUpdate {
    pub key: RowKey,
    pub values: Writes,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangeSet {
    pub updates: Vec<RowUpdate>,
    pub inserts: Vec<Writes>,
    pub deletes: Vec<RowKey>,
}

impl ChangeSet {
    pub fn is_empty(&self) -> bool {
        self.updates.is_empty() && self.inserts.is_empty() && self.deletes.is_empty()
    }
}

pub fn rename_create_table(ddl: &str, old: &str, new_qualified: &str) -> Option<String> {
    let head_len = {
        let upper = ddl.to_ascii_uppercase();
        let start = upper.find("CREATE TABLE")? + "CREATE TABLE".len();
        let rest = &upper[start..];
        let skipped = rest.len() - rest.trim_start().len();
        let after_if = rest
            .trim_start()
            .strip_prefix("IF NOT EXISTS")
            .map(|r| rest.len() - r.len());
        let pos = after_if.unwrap_or(skipped);
        let rest = &upper[start + pos..];
        start + pos + (rest.len() - rest.trim_start().len())
    };
    let tail = &ddl[head_len..];
    let forms = [
        format!("\"{old}\""),
        format!("`{old}`"),
        format!("[{old}]"),
        old.to_string(),
    ];
    let name = forms.iter().find(|f| tail.starts_with(f.as_str()))?;
    Some(format!(
        "{}{new_qualified}{}",
        &ddl[..head_len],
        &tail[name.len()..]
    ))
}
