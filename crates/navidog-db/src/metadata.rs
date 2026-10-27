//! Shared readers for catalog result sets. The SQL itself lives in the database crates.

use navidog_model::object::ObjectKind;

use crate::driver::{
    DbError, DbResult, ForeignKey, ObjectInfo, Relational, ResultSet, TableChildren,
};
use navidog_sql::dialect::Dialect;

pub async fn query<R: Relational + ?Sized>(r: &mut R, sql: &str) -> DbResult<ResultSet> {
    let mut sets = r.execute_read(sql).await?;
    sets.drain(..)
        .find(|s| !s.columns.is_empty())
        .ok_or_else(|| DbError::coded("navidog.no_result", "The query returned no result set."))
}

pub fn cell(row: &[Option<String>], ix: usize) -> String {
    row.get(ix).cloned().flatten().unwrap_or_default()
}

pub fn unsupported(kind: ObjectKind) -> DbError {
    DbError::coded(
        "navidog.unsupported_kind",
        format!("Unsupported object category: {}", kind.key()),
    )
}

pub async fn first_column<R: Relational + ?Sized>(r: &mut R, sql: &str) -> DbResult<Vec<String>> {
    Ok(query(r, sql)
        .await?
        .rows
        .iter()
        .map(|row| cell(row, 0))
        .collect())
}

pub fn objects(rows: &[Vec<Option<String>>], kind: ObjectKind, props: &[&str]) -> Vec<ObjectInfo> {
    rows.iter()
        .map(|row| {
            let properties = props
                .iter()
                .enumerate()
                .map(|(i, k)| (k.to_string(), cell(row, i + 1)))
                .filter(|(_, v)| !v.is_empty())
                .collect::<Vec<_>>();
            let comment = properties
                .iter()
                .find(|(k, _)| k == "comment")
                .map(|(_, v)| v.clone());
            ObjectInfo {
                name: cell(row, 0),
                kind,
                comment,
                properties,
            }
        })
        .collect()
}

pub async fn children_from<R: Relational + ?Sized>(
    r: &mut R,
    indexes: String,
    foreign_keys: String,
    triggers: String,
) -> DbResult<TableChildren> {
    Ok(TableChildren {
        indexes: first_column(r, &indexes).await?,
        foreign_keys: first_column(r, &foreign_keys).await?,
        triggers: first_column(r, &triggers).await?,
    })
}

pub fn foreign_keys_from_rows(rows: &[Vec<Option<String>>]) -> Vec<ForeignKey> {
    let mut out = Vec::new();
    for row in rows {
        let column = cell(row, 0);
        let schema = cell(row, 1);
        let table = cell(row, 2);
        let referenced = cell(row, 3);
        if column.is_empty() || table.is_empty() || referenced.is_empty() {
            continue;
        }
        if out.iter().any(|key: &ForeignKey| key.column == column) {
            continue;
        }
        out.push(ForeignKey {
            column,
            schema,
            table,
            referenced,
        });
    }
    out
}

/// Values of the referenced column, for the picker. `limit` None loads the whole column.
pub fn referenced_values_sql(
    dialect: &dyn Dialect,
    key: &ForeignKey,
    search: &str,
    limit: Option<u64>,
) -> String {
    let column = dialect.quote_ident(&key.referenced);
    let table = if key.schema.is_empty() {
        dialect.quote_ident(&key.table)
    } else {
        format!(
            "{}.{}",
            dialect.quote_ident(&key.schema),
            dialect.quote_ident(&key.table)
        )
    };
    let mut sql = format!("SELECT DISTINCT {column} FROM {table}");
    if !search.is_empty() {
        let escaped = search
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        // PostgreSQL has no LIKE for integers, and most foreign keys are integers.
        let cast = if dialect.like_text_cast() {
            "::text"
        } else {
            ""
        };
        sql.push_str(&format!(
            " WHERE {column}{cast} LIKE {} ESCAPE {}",
            dialect.string_literal(&format!("%{escaped}%")),
            dialect.string_literal("\\")
        ));
    }
    sql.push_str(" ORDER BY 1");
    if let Some(limit) = limit {
        sql.push_str(&format!(" LIMIT {limit}"));
    }
    sql
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_without_a_referenced_column_are_dropped() {
        let rows = vec![
            vec![
                Some("actor_id".into()),
                Some("sakila".into()),
                Some("actor".into()),
                Some("actor_id".into()),
            ],
            vec![
                Some("actor_id".into()),
                Some("other".into()),
                Some("actor".into()),
                Some("id".into()),
            ],
            vec![Some("film_id".into()), None, Some("film".into()), None],
        ];
        let keys = foreign_keys_from_rows(&rows);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].column, "actor_id");
        assert_eq!(keys[0].referenced, "actor_id");
        let sql = referenced_values_sql(&navidog_mysql::MysqlDialect, &keys[0], "a%_", Some(1000));
        assert_eq!(
            sql,
            "SELECT DISTINCT `actor_id` FROM `sakila`.`actor` WHERE `actor_id` LIKE '%a\\\\%\\\\_%' ESCAPE '\\\\' ORDER BY 1 LIMIT 1000"
        );
        // Integer keys on PostgreSQL are searched as text.
        let sql = referenced_values_sql(&navidog_postgres::PostgresDialect, &keys[0], "1", None);
        assert!(sql.contains("\"actor_id\"::text LIKE '%1%'"), "{sql}");
    }
}
