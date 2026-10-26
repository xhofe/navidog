//! PostgreSQL catalog SQL.

use navidog_db::driver::{ColumnInfo, DbResult, ObjectInfo, Relational, ResultSet};
use navidog_db::metadata::{self, cell};
use navidog_db::outline::{self, Outline, OutlineTable};
use navidog_model::object::{ObjectKind, Scope};

pub async fn list_databases<R: Relational + ?Sized>(r: &mut R) -> DbResult<Vec<String>> {
    metadata::first_column(
        r,
        "SELECT datname FROM pg_database WHERE NOT datistemplate ORDER BY datname",
    )
    .await
}

pub async fn list_schemas<R: Relational + ?Sized>(r: &mut R) -> DbResult<Vec<String>> {
    // System schemas (pg_catalog, information_schema) are listed; the navigator hides them
    // unless "Show Hidden Items" is on. Toast and temp schemas are internal and never shown.
    metadata::first_column(
        r,
        "SELECT nspname FROM pg_namespace \
         WHERE nspname NOT LIKE 'pg\\_toast%' AND nspname NOT LIKE 'pg\\_temp%' ORDER BY nspname",
    )
    .await
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

fn relkind(kind: ObjectKind) -> Option<&'static str> {
    Some(match kind {
        ObjectKind::Table => "'r', 'p'",
        ObjectKind::View => "'v'",
        ObjectKind::MaterializedView => "'m'",
        ObjectKind::Sequence => "'S'",
        _ => return None,
    })
}

fn object_sql(
    d: &dyn navidog_sql::Dialect,
    scope: &Scope,
    kind: ObjectKind,
) -> DbResult<(String, &'static [&'static str])> {
    let schema = d.string_literal(scope.schema.as_deref().unwrap_or("public"));
    if kind == ObjectKind::Function {
        return Ok((
            format!(
                "SELECT p.proname, CASE p.prokind WHEN 'p' THEN 'PROCEDURE' ELSE 'FUNCTION' END, \
                 pg_get_function_identity_arguments(p.oid), pg_get_function_result(p.oid), \
                 pg_get_userbyid(p.proowner), obj_description(p.oid, 'pg_proc') \
                 FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
                 WHERE n.nspname = {schema} AND p.prokind IN ('f', 'p') ORDER BY p.proname"
            ),
            &["type", "arguments", "returns", "owner", "comment"],
        ));
    }
    let relkind = relkind(kind).ok_or_else(|| metadata::unsupported(kind))?;
    Ok((
        format!(
            "SELECT c.relname, CASE WHEN c.reltuples < 0 THEN NULL ELSE c.reltuples::bigint END, \
             pg_total_relation_size(c.oid), pg_get_userbyid(c.relowner), obj_description(c.oid, 'pg_class') \
             FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = {schema} AND c.relkind IN ({relkind}) ORDER BY c.relname"
        ),
        &["rows", "data_length", "owner", "comment"],
    ))
}

pub async fn object_ddl<R: Relational + ?Sized>(
    r: &mut R,
    scope: &Scope,
    kind: ObjectKind,
    name: &str,
) -> DbResult<String> {
    let d = r.dialect();
    let target = d.qualified(scope, name);
    let regclass = d.string_literal(&target);
    let first = |set: ResultSet| set.rows.first().map(|row| cell(row, 0)).unwrap_or_default();
    match kind {
        ObjectKind::View => {
            let def = first(
                metadata::query(
                    r,
                    &format!("SELECT pg_get_viewdef({regclass}::regclass, true)"),
                )
                .await?,
            );
            Ok(format!("CREATE VIEW {target} AS\n{def}"))
        }
        ObjectKind::MaterializedView => {
            let def = first(
                metadata::query(
                    r,
                    &format!("SELECT pg_get_viewdef({regclass}::regclass, true)"),
                )
                .await?,
            );
            Ok(format!("CREATE MATERIALIZED VIEW {target} AS\n{def}"))
        }
        ObjectKind::Function => {
            let schema = d.string_literal(scope.schema.as_deref().unwrap_or("public"));
            let sql = format!(
                "SELECT pg_get_functiondef(p.oid) FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
                 WHERE n.nspname = {schema} AND p.proname = {} AND p.prokind IN ('f', 'p') LIMIT 1",
                d.string_literal(name)
            );
            Ok(first(metadata::query(r, &sql).await?))
        }
        ObjectKind::Sequence => {
            let set = metadata::query(
                r,
                &format!(
                    "SELECT seqstart, seqincrement, seqmin, seqmax, seqcache, seqcycle FROM pg_sequence WHERE seqrelid = {regclass}::regclass"
                ),
            )
            .await?;
            let row = set.rows.first().cloned().unwrap_or_default();
            Ok(format!(
                "CREATE SEQUENCE {target}\n  START WITH {}\n  INCREMENT BY {}\n  MINVALUE {}\n  MAXVALUE {}\n  CACHE {}{};",
                cell(&row, 0),
                cell(&row, 1),
                cell(&row, 2),
                cell(&row, 3),
                cell(&row, 4),
                if cell(&row, 5) == "t" {
                    "\n  CYCLE"
                } else {
                    ""
                }
            ))
        }
        ObjectKind::Table => {
            let cols = metadata::query(
                r,
                &format!(
                    "SELECT a.attname, format_type(a.atttypid, a.atttypmod), a.attnotnull, pg_get_expr(ad.adbin, ad.adrelid) \
                     FROM pg_attribute a LEFT JOIN pg_attrdef ad ON ad.adrelid = a.attrelid AND ad.adnum = a.attnum \
                     WHERE a.attrelid = {regclass}::regclass AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum"
                ),
            )
            .await?;
            let cons = metadata::query(
                r,
                &format!(
                    "SELECT conname, pg_get_constraintdef(oid) FROM pg_constraint WHERE conrelid = {regclass}::regclass \
                     ORDER BY contype DESC, conname"
                ),
            )
            .await?;
            let idx = metadata::query(
                r,
                &format!(
                    "SELECT pg_get_indexdef(i.indexrelid) FROM pg_index i \
                     WHERE i.indrelid = {regclass}::regclass AND NOT i.indisprimary \
                     AND NOT EXISTS (SELECT 1 FROM pg_constraint c WHERE c.conindid = i.indexrelid)"
                ),
            )
            .await?;
            let comment = first(
                metadata::query(
                    r,
                    &format!("SELECT obj_description({regclass}::regclass, 'pg_class')"),
                )
                .await?,
            );
            let mut lines: Vec<String> = cols
                .rows
                .iter()
                .map(|row| {
                    let mut line = format!("  {} {}", d.quote_ident(&cell(row, 0)), cell(row, 1));
                    if cell(row, 2) == "t" {
                        line.push_str(" NOT NULL");
                    }
                    let default = cell(row, 3);
                    if !default.is_empty() {
                        line.push_str(&format!(" DEFAULT {default}"));
                    }
                    line
                })
                .collect();
            lines.extend(cons.rows.iter().map(|row| {
                format!(
                    "  CONSTRAINT {} {}",
                    d.quote_ident(&cell(row, 0)),
                    cell(row, 1)
                )
            }));
            let mut ddl = format!("CREATE TABLE {target} (\n{}\n);", lines.join(",\n"));
            for row in &idx.rows {
                ddl.push_str(&format!("\n{};", cell(row, 0)));
            }
            if !comment.is_empty() {
                ddl.push_str(&format!(
                    "\nCOMMENT ON TABLE {target} IS {};",
                    d.string_literal(&comment)
                ));
            }
            Ok(ddl)
        }
        other => Err(metadata::unsupported(other)),
    }
}

pub async fn table_columns<R: Relational + ?Sized>(
    r: &mut R,
    scope: &Scope,
    table: &str,
) -> DbResult<Vec<ColumnInfo>> {
    let d = r.dialect();
    let regclass = d.string_literal(&d.qualified(scope, table));
    let sql = format!(
        "SELECT a.attname, format_type(a.atttypid, a.atttypmod), NOT a.attnotnull, \
         COALESCE((SELECT true FROM pg_index i WHERE i.indrelid = a.attrelid AND i.indisprimary AND a.attnum = ANY(i.indkey)), false), \
         pg_get_expr(ad.adbin, ad.adrelid), col_description(a.attrelid, a.attnum) \
         FROM pg_attribute a LEFT JOIN pg_attrdef ad ON ad.adrelid = a.attrelid AND ad.adnum = a.attnum \
         WHERE a.attrelid = {regclass}::regclass AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum"
    );
    Ok(metadata::query(r, &sql)
        .await?
        .rows
        .iter()
        .map(|row| ColumnInfo {
            name: cell(row, 0),
            type_name: cell(row, 1),
            nullable: cell(row, 2) == "t",
            primary_key: cell(row, 3) == "t",
            default: row.get(4).cloned().flatten(),
            comment: row.get(5).cloned().flatten(),
        })
        .collect())
}

pub async fn table_children<R: Relational + ?Sized>(
    r: &mut R,
    scope: &Scope,
    table: &str,
) -> DbResult<navidog_db::TableChildren> {
    let d = r.dialect();
    let rel = format!("{}::regclass", d.string_literal(&d.qualified(scope, table)));
    metadata::children_from(
        r,
        format!(
            "SELECT c.relname FROM pg_index i JOIN pg_class c ON c.oid = i.indexrelid WHERE i.indrelid = {rel} ORDER BY 1"
        ),
        format!("SELECT conname FROM pg_constraint WHERE conrelid = {rel} AND contype = 'f' ORDER BY 1"),
        format!(
            "SELECT tgname FROM pg_trigger WHERE tgrelid = {rel} AND NOT tgisinternal ORDER BY 1"
        ),
    )
    .await
}

pub fn foreign_key_sql(dialect: &dyn navidog_sql::Dialect, scope: &Scope, table: &str) -> String {
    let rel = format!(
        "{}::regclass",
        dialect.string_literal(&dialect.qualified(scope, table))
    );
    format!(
        "SELECT a.attname, nf.nspname, cf.relname, af.attname \
         FROM pg_constraint c \
         JOIN LATERAL unnest(c.conkey, c.confkey) WITH ORDINALITY AS cols(attnum, refnum, ord) ON true \
         JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = cols.attnum \
         JOIN pg_class cf ON cf.oid = c.confrelid \
         JOIN pg_namespace nf ON nf.oid = cf.relnamespace \
         JOIN pg_attribute af ON af.attrelid = c.confrelid AND af.attnum = cols.refnum \
         WHERE c.contype = 'f' AND c.conrelid = {rel} \
         ORDER BY c.oid, cols.ord"
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
    system: bool,
) -> DbResult<Outline> {
    let d = rel.dialect();
    let schema = scope.schema.clone().unwrap_or_else(|| "public".into());
    let mut wanted = vec![d.string_literal(&schema)];
    if system {
        wanted.extend([
            d.string_literal("pg_catalog"),
            d.string_literal("information_schema"),
        ]);
    }
    let list = wanted.join(", ");
    let mut tables: Vec<OutlineTable> = outline::rows(
        rel.execute_read(&format!(
            "SELECT c.relname, c.relkind FROM pg_class c \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname IN ({list}) AND c.relkind IN ('r', 'p', 'v', 'm', 'f') \
             ORDER BY c.relname"
        ))
        .await?,
    )
    .into_iter()
    .map(|r| OutlineTable {
        name: r[0].clone(),
        view: matches!(r.get(1).map(String::as_str), Some("v" | "m")),
        columns: Vec::new(),
    })
    .collect();
    let columns = outline::rows(
        rel.execute_read(&format!(
            "SELECT c.relname, a.attname, format_type(a.atttypid, a.atttypmod) \
             FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname IN ({list}) AND a.attnum > 0 AND NOT a.attisdropped \
             AND c.relkind IN ('r', 'p', 'v', 'm', 'f') ORDER BY c.relname, a.attnum"
        ))
        .await?,
    );
    outline::group_columns(&mut tables, columns);
    let functions = outline::rows(
        rel.execute_read(&format!(
            "SELECT DISTINCT p.proname FROM pg_proc p \
             JOIN pg_namespace n ON n.oid = p.pronamespace \
             WHERE n.nspname IN ({list}) ORDER BY p.proname"
        ))
        .await?,
    )
    .into_iter()
    .map(|r| r[0].clone())
    .collect();
    let mut schemas = rel.list_schemas().await?;
    if !system {
        schemas.retain(|s| !d.is_system_schema(s));
    }
    Ok(Outline {
        tables,
        functions,
        schemas,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreign_key_sql_reads_the_catalog() {
        let scope = Scope::new("c", "sakila", Some("public".into()));
        let sql = foreign_key_sql(&crate::PostgresDialect, &scope, "film_actor");
        assert!(sql.contains("pg_constraint"));
        assert!(sql.contains("unnest(c.conkey, c.confkey)"));
        assert!(sql.contains("\"public\".\"film_actor\""));
    }
}
