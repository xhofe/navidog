//! SQLite catalog SQL.

use navidog_db::driver::{ColumnInfo, DbResult, ObjectInfo, Relational};
use navidog_db::metadata::{self, cell};
use navidog_db::outline::{self, Outline, OutlineTable};
use navidog_model::object::{ObjectKind, Scope};

pub async fn list_databases<R: Relational + ?Sized>(r: &mut R) -> DbResult<Vec<String>> {
    // seq, name, file; `temp` is the per-connection temporary schema.
    let names = metadata::query(r, "PRAGMA database_list").await?;
    Ok(names
        .rows
        .iter()
        .map(|row| cell(row, 1))
        .filter(|name| name != "temp")
        .collect())
}

pub async fn list_schemas<R: Relational + ?Sized>(_r: &mut R) -> DbResult<Vec<String>> {
    Ok(Vec::new())
}

pub async fn list_objects<R: Relational + ?Sized>(
    r: &mut R,
    scope: &Scope,
    kind: ObjectKind,
) -> DbResult<Vec<ObjectInfo>> {
    let (sql, props) = object_sql(r.dialect().as_ref(), scope, kind)?;
    let set = metadata::query(r, &sql).await?;
    Ok(metadata::objects(&set.rows, kind, props))
}

fn object_sql(
    d: &dyn navidog_sql::Dialect,
    scope: &Scope,
    kind: ObjectKind,
) -> DbResult<(String, &'static [&'static str])> {
    let ty = match kind {
        ObjectKind::Table => "table",
        ObjectKind::View => "view",
        ObjectKind::Index => "index",
        ObjectKind::Trigger => "trigger",
        other => return Err(metadata::unsupported(other)),
    };
    Ok((
        format!(
            "SELECT name, tbl_name FROM {}.sqlite_schema \
             WHERE type = '{ty}' AND sql IS NOT NULL ORDER BY name",
            d.quote_ident(&scope.database)
        ),
        &["table"],
    ))
}

pub async fn object_ddl<R: Relational + ?Sized>(
    r: &mut R,
    scope: &Scope,
    _kind: ObjectKind,
    name: &str,
) -> DbResult<String> {
    let d = r.dialect();
    let set = metadata::query(
        r,
        &format!(
            "SELECT sql FROM {}.sqlite_schema WHERE name = {}",
            d.quote_ident(&scope.database),
            d.string_literal(name)
        ),
    )
    .await?;
    Ok(set.rows.first().map(|row| cell(row, 0)).unwrap_or_default() + ";")
}

pub async fn table_columns<R: Relational + ?Sized>(
    r: &mut R,
    scope: &Scope,
    table: &str,
) -> DbResult<Vec<ColumnInfo>> {
    let d = r.dialect();
    let sql = format!(
        "PRAGMA {}.table_xinfo({})",
        d.quote_ident(&scope.database),
        d.quote_ident(table)
    );
    Ok(metadata::query(r, &sql)
        .await?
        .rows
        .iter()
        .map(|row| ColumnInfo {
            name: cell(row, 1),
            type_name: cell(row, 2),
            nullable: cell(row, 3) == "0",
            primary_key: cell(row, 5) != "0",
            default: row.get(4).cloned().flatten(),
            comment: None,
        })
        .collect())
}

pub async fn table_children<R: Relational + ?Sized>(
    r: &mut R,
    scope: &Scope,
    table: &str,
) -> DbResult<navidog_db::TableChildren> {
    let d = r.dialect();
    let t = d.string_literal(table);
    let schema = format!("{}.sqlite_schema", d.quote_ident(&scope.database));
    metadata::children_from(
        r,
        format!("SELECT name FROM {schema} WHERE type = 'index' AND tbl_name = {t} ORDER BY name"),
        // SQLite foreign keys have no names; label them by the referenced table.
        format!(
            "SELECT DISTINCT '→ ' || \"table\" FROM pragma_foreign_key_list({t}, {}) ORDER BY 1",
            d.string_literal(&scope.database)
        ),
        format!(
            "SELECT name FROM {schema} WHERE type = 'trigger' AND tbl_name = {t} ORDER BY name"
        ),
    )
    .await
}

pub fn foreign_key_sql(dialect: &dyn navidog_sql::Dialect, scope: &Scope, table: &str) -> String {
    let schema = dialect.string_literal(&scope.database);
    let t = dialect.string_literal(table);
    format!(
        "SELECT \"from\", {schema}, \"table\", \"to\" \
         FROM pragma_foreign_key_list({t}, {schema}) ORDER BY id, seq"
    )
}

pub async fn foreign_keys<R: Relational + ?Sized>(
    r: &mut R,
    scope: &Scope,
    table: &str,
) -> DbResult<Vec<navidog_db::ForeignKey>> {
    let sql = foreign_key_sql(r.dialect().as_ref(), scope, table);
    Ok(metadata::foreign_keys_from_rows(
        &metadata::query(r, &sql).await?.rows,
    ))
}

pub async fn outline<R: Relational + ?Sized>(
    rel: &mut R,
    scope: &Scope,
    _system: bool,
) -> DbResult<Outline> {
    let d = rel.dialect();
    let db = d.quote_ident(&scope.database);
    let dbl = d.string_literal(&scope.database);
    let mut tables: Vec<OutlineTable> = outline::rows(
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
    let columns = outline::rows(
        rel.execute_read(&format!(
            "SELECT m.name, p.name, p.type FROM {db}.sqlite_schema m \
             JOIN pragma_table_xinfo(m.name, {dbl}) p \
             WHERE m.type IN ('table', 'view') AND m.name NOT LIKE 'sqlite\\_%' ESCAPE '\\' \
             ORDER BY m.name, p.cid"
        ))
        .await?,
    );
    outline::group_columns(&mut tables, columns);
    let schemas = rel.list_databases().await?;
    Ok(Outline {
        tables,
        schemas,
        ..Outline::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreign_key_sql_reads_the_catalog() {
        let scope = Scope::new("c", "sakila", None);
        let sql = foreign_key_sql(&crate::SqliteDialect, &scope, "film_actor");
        assert!(sql.contains("pragma_foreign_key_list"));
        assert!(sql.contains("\"from\""));
        assert!(sql.contains("\"to\""));
    }
}
