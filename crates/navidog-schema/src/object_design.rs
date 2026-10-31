//! Plans and applies the shared object designer. The statement text lives on the dialect.

use navidog_db::{DbError, DbResult, Relational};
use navidog_model::object::Scope;
use navidog_sql::definition::DefinitionDraft;
use navidog_sql::dialect::Dialect;

pub use navidog_sql::definition::{DefinitionDraft as Draft, DefinitionKind};

pub fn plan(
    dialect: &dyn Dialect,
    scope: &Scope,
    draft: &DefinitionDraft,
) -> DbResult<Vec<String>> {
    dialect
        .definition_statements(scope, draft)
        .map_err(DbError::msg)
}

/// Runs the planned statements. An object that changed since it was loaded is refused.
pub async fn apply(r: &mut dyn Relational, scope: &Scope, draft: &DefinitionDraft) -> DbResult<()> {
    let d = r.dialect();
    if let (Some(prev), Some(kind)) = (draft.previous.as_deref(), draft.kind.catalog_kind())
        && !draft.loaded.is_empty()
    {
        let current = r.object_ddl(scope, kind, prev).await.unwrap_or_default();
        if !current.is_empty() && current != draft.loaded {
            return Err(DbError::msg(
                "The object changed outside this designer. Reload before saving.",
            ));
        }
    }
    if let Some(sql) = d.prepare_database_sql(&scope.database) {
        r.execute(&sql).await?;
    }
    if let Some(schema) = scope.schema.as_deref().or(d.default_schema())
        && let Some(sql) = d.set_search_path_sql(schema)
    {
        r.execute(&sql).await?;
    }
    let statements = plan(d.as_ref(), scope, draft)?;
    if statements.is_empty() {
        return Ok(());
    }
    // CREATE DATABASE, CREATE TABLESPACE and ATTACH cannot run inside a transaction.
    let transactional = d.features().transactional_ddl
        && !matches!(
            draft.kind,
            DefinitionKind::Database
                | DefinitionKind::Tablespace
                | DefinitionKind::Attach
                | DefinitionKind::Detach
        );
    if transactional {
        r.execute(if d.features().begin_immediate {
            "BEGIN IMMEDIATE"
        } else {
            d.begin_transaction()
        })
        .await?;
    }
    let result: DbResult<()> = async {
        for sql in &statements {
            r.execute(sql).await?;
        }
        Ok(())
    }
    .await;
    if transactional {
        let end = if result.is_ok() { "COMMIT" } else { "ROLLBACK" };
        let _ = r.execute(end).await;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn draft(kind: DefinitionKind, name: &str) -> DefinitionDraft {
        DefinitionDraft {
            kind,
            name: name.into(),
            previous: None,
            body: String::new(),
            raw: false,
            loaded: String::new(),
            options: BTreeMap::new(),
        }
    }

    fn opt(draft: &mut DefinitionDraft, key: &str, value: &str) {
        draft.options.insert(key.into(), value.into());
    }

    #[test]
    fn definition_plans_cover_each_object_kind() {
        let mysql = navidog_mysql::MysqlDialect;
        let pg = navidog_postgres::PostgresDialect;
        let sqlite = navidog_sqlite::SqliteDialect;
        let my = Scope::new("c", "app", None);
        let pg_scope = Scope::new("c", "app", Some("public".into()));
        let file = Scope::new("c", "main", None);

        let mut view = draft(DefinitionKind::View, "v");
        view.body = "SELECT 1".into();
        opt(&mut view, "algorithm", "MERGE");
        opt(&mut view, "security", "INVOKER");
        opt(&mut view, "check_option", "CASCADED");
        let sql = plan(&mysql, &my, &view).unwrap().join("\n");
        assert!(sql.contains("ALGORITHM = MERGE"), "{sql}");
        assert!(sql.contains("SQL SECURITY INVOKER"), "{sql}");
        assert!(sql.contains("WITH CASCADED CHECK OPTION"), "{sql}");

        let mut pg_view = draft(DefinitionKind::View, "v");
        pg_view.body = "SELECT 1".into();
        assert!(plan(&pg, &pg_scope, &pg_view).unwrap()[0].starts_with("CREATE OR REPLACE VIEW"));

        let mut lite = draft(DefinitionKind::View, "v");
        lite.previous = Some("old".into());
        lite.body = "SELECT 1".into();
        let sql = plan(&sqlite, &file, &lite).unwrap();
        assert!(sql[0].contains("DROP VIEW"));
        assert!(sql[1].starts_with("CREATE VIEW"));

        let mut mview = draft(DefinitionKind::MaterializedView, "mv");
        mview.body = "SELECT 1".into();
        opt(&mut mview, "with_data", "0");
        assert!(plan(&pg, &pg_scope, &mview).unwrap()[0].contains("WITH NO DATA"));

        let mut func = draft(DefinitionKind::Function, "f");
        func.body = "RETURN 1".into();
        opt(&mut func, "returns", "INT");
        opt(&mut func, "parameters", "IN a INT");
        let sql = plan(&mysql, &my, &func).unwrap();
        assert!(sql.iter().any(|s| s.starts_with("DROP FUNCTION")));
        assert!(sql.last().unwrap().contains("RETURNS INT"));

        let mut proc = draft(DefinitionKind::Procedure, "p");
        proc.body = "BEGIN PERFORM 1; END".into();
        opt(&mut proc, "language", "plpgsql");
        assert!(plan(&pg, &pg_scope, &proc).unwrap()[0].contains("CREATE OR REPLACE PROCEDURE"));

        let mut event = draft(DefinitionKind::Event, "e");
        event.body = "DO 0".into();
        opt(&mut event, "schedule", "EVERY 1 HOUR");
        opt(&mut event, "status", "DISABLE");
        let sql = plan(&mysql, &my, &event).unwrap()[0].clone();
        assert!(sql.contains("ON SCHEDULE EVERY 1 HOUR"), "{sql}");
        assert!(sql.contains("DISABLE"), "{sql}");

        let mut db = draft(DefinitionKind::Database, "app2");
        opt(&mut db, "charset", "utf8mb4");
        assert!(plan(&mysql, &my, &db).unwrap()[0].contains("CHARACTER SET utf8mb4"));
        db.previous = Some("app2".into());
        db.name = "other".into();
        assert!(plan(&mysql, &my, &db).is_err());

        let pdb = draft(DefinitionKind::Database, "app2");
        assert!(plan(&pg, &pg_scope, &pdb).unwrap()[0].contains("TEMPLATE"));
        let mut schema = draft(DefinitionKind::Schema, "s");
        opt(&mut schema, "owner", "bob");
        assert!(plan(&pg, &pg_scope, &schema).unwrap()[0].contains("AUTHORIZATION"));

        let mut space = draft(DefinitionKind::Tablespace, "ts");
        opt(&mut space, "datafile", "/tmp/ts.ibd");
        assert!(plan(&mysql, &my, &space).unwrap()[0].contains("ADD DATAFILE"));
        opt(&mut space, "datafile", "/var/lib/ts");
        assert!(plan(&pg, &pg_scope, &space).unwrap()[0].contains("LOCATION"));

        let mut seq = draft(DefinitionKind::Sequence, "seq");
        opt(&mut seq, "start", "1");
        opt(&mut seq, "increment", "2");
        assert!(plan(&pg, &pg_scope, &seq).unwrap()[0].contains("INCREMENT BY 2"));
        assert!(plan(&mysql, &my, &seq).unwrap()[0].contains("CREATE OR REPLACE SEQUENCE"));

        let mut ty = draft(DefinitionKind::Type, "mood");
        ty.body = "'ok', 'no'".into();
        opt(&mut ty, "type_kind", "enum");
        assert!(plan(&pg, &pg_scope, &ty).unwrap()[0].contains("AS ENUM"));

        let mut domain = draft(DefinitionKind::Domain, "pos");
        opt(&mut domain, "base_type", "integer");
        opt(&mut domain, "not_null", "1");
        assert!(plan(&pg, &pg_scope, &domain).unwrap()[0].contains("NOT NULL"));

        let mut ext = draft(DefinitionKind::Extension, "plpgsql");
        assert!(plan(&pg, &pg_scope, &ext).unwrap()[0].contains("CREATE EXTENSION"));
        opt(&mut ext, "action", "drop");
        assert!(plan(&pg, &pg_scope, &ext).unwrap()[0].starts_with("DROP EXTENSION"));

        let mut server = draft(DefinitionKind::ForeignServer, "fs");
        opt(&mut server, "wrapper", "postgres_fdw");
        assert!(plan(&pg, &pg_scope, &server).unwrap()[0].contains("FOREIGN DATA WRAPPER"));

        let mut agg = draft(DefinitionKind::Aggregate, "sum_int");
        opt(&mut agg, "input_type", "integer");
        opt(&mut agg, "state_func", "int4pl");
        opt(&mut agg, "state_type", "integer");
        assert!(plan(&pg, &pg_scope, &agg).unwrap()[0].contains("SFUNC"));

        let mut cast = draft(DefinitionKind::Cast, "c");
        opt(&mut cast, "source", "integer");
        opt(&mut cast, "target", "text");
        opt(&mut cast, "function", "text");
        assert!(plan(&pg, &pg_scope, &cast).unwrap()[0].contains("CREATE CAST"));

        let mut op = draft(DefinitionKind::Operator, "+");
        opt(&mut op, "left", "integer");
        opt(&mut op, "right", "integer");
        opt(&mut op, "function", "int4pl");
        let sql = plan(&pg, &pg_scope, &op).unwrap()[0].clone();
        assert!(sql.contains("CREATE OPERATOR +"), "{sql}");

        let mut coll = draft(DefinitionKind::Collation, "en");
        opt(&mut coll, "provider", "icu");
        opt(&mut coll, "locale", "en-US");
        assert!(plan(&pg, &pg_scope, &coll).unwrap()[0].contains("PROVIDER = icu"));

        let mut index = draft(DefinitionKind::Index, "ix");
        index.body = "id".into();
        opt(&mut index, "table", "t");
        assert!(plan(&sqlite, &file, &index).unwrap()[0].contains("CREATE INDEX"));

        let mut trigger = draft(DefinitionKind::Trigger, "tr");
        trigger.body = "SELECT 1".into();
        opt(&mut trigger, "table", "t");
        opt(&mut trigger, "timing", "AFTER");
        opt(&mut trigger, "events", "INSERT");
        assert!(plan(&sqlite, &file, &trigger).unwrap()[0].contains("BEGIN SELECT 1; END"));

        let mut attach = draft(DefinitionKind::Attach, "other");
        opt(&mut attach, "path", "/tmp/other.db");
        assert!(plan(&sqlite, &file, &attach).unwrap()[0].starts_with("ATTACH DATABASE"));
        let detach = draft(DefinitionKind::Detach, "other");
        assert!(plan(&sqlite, &file, &detach).unwrap()[0].starts_with("DETACH DATABASE"));
        assert!(plan(&sqlite, &file, &draft(DefinitionKind::Detach, "main")).is_err());

        assert!(mariadb_lists("mariadb", "10.3.1-MariaDB"));
        assert!(mariadb_lists("mariadb", "5.5.5-10.11.2-MariaDB"));
        assert!(!mariadb_lists("mariadb", "10.2.9-MariaDB"));
        assert!(!mariadb_lists("mysql", "10.11.2-MariaDB"));
    }

    fn mariadb_lists(driver: &str, version: &str) -> bool {
        navidog_mysql::MysqlDialect.lists_kind(
            navidog_model::object::ObjectKind::Sequence,
            driver,
            version,
        )
    }
}
