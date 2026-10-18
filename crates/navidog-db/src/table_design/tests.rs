use super::*;
use std::sync::Arc;
fn scope() -> Scope {
    Scope::new("test", "main", Some("public".into()))
}
#[test]
fn quoted_definitions_round_trip_and_change_by_identity() {
    let sql = "CREATE TABLE `t` (`id` int NOT NULL AUTO_INCREMENT, `a``b` enum('x,y','z') DEFAULT 'x,y', PRIMARY KEY (`id`), KEY `ix` (`a``b`)) ENGINE=InnoDB";
    let design = super::parse::create(sql, "t", Dialect::MySql).unwrap();
    assert_eq!(design.columns[1].name, "a`b");
    assert_eq!(design.constraints.len(), 2);
    assert!(
        design
            .create_sql(Dialect::MySql, &scope(), "t")
            .unwrap()
            .contains("enum('x,y','z') DEFAULT 'x,y'")
    );
    let before = TableSnapshot {
        design: design.clone(),
        ddl: sql.into(),
        triggers: Vec::new(),
    };
    let mut after = design;
    after.columns[1].name = "renamed".into();
    let plan = plan(Dialect::MySql, &scope(), Some(&before), &after).unwrap();
    assert_eq!(plan.len(), 1);
    assert!(plan[0].contains("CHANGE COLUMN `a``b` `renamed` enum('x,y','z') DEFAULT 'x,y'"));
}
#[test]
fn invalid_fragments_and_unsupported_attributes_are_rejected() {
    let mut draft = TableDesign::new();
    draft.name = "t".into();
    draft.columns[0].definition = "int); DROP TABLE x; --".into();
    assert!(plan(Dialect::MySql, &scope(), None, &draft).is_err());
    assert!(super::plan::index_name("CREATE TABLE x (id int)").is_err());
    assert!(super::parse::fragment("TEXT DEFAULT 'semi;hash#-- $$/*'").is_ok());
    assert!(super::parse::fragment("TEXT DEFAULT 1 -- hidden suffix").is_err());
}
#[test]
fn sqlite_rebuild_preserves_rows_indexes_triggers_and_rolls_back() {
    crate::runtime::runtime().block_on(async {
        let mut session=crate::connect(Arc::new(crate::sqlite::SqliteDriver::default()),crate::ConnectParams {file:Some(":memory:".into()),create_new:true,..Default::default()}).await.unwrap();
        let r=session.relational().unwrap(); let scope=Scope::new("test","main",None);
        r.execute("PRAGMA foreign_keys=ON; CREATE TABLE p(id INTEGER PRIMARY KEY); INSERT INTO p VALUES(1); CREATE TABLE t(id INTEGER PRIMARY KEY AUTOINCREMENT, value TEXT NOT NULL, parent INTEGER REFERENCES p(id), CHECK(length(value)>0)); CREATE INDEX ix_t ON t(value); CREATE TABLE audit(v TEXT); CREATE TRIGGER audit_t AFTER INSERT ON t BEGIN INSERT INTO audit VALUES(new.value); END; INSERT INTO t(value,parent) VALUES('before',1)").await.unwrap();
        let old=load(r,&scope,"t").await.unwrap();assert_eq!(old.triggers.len(),1);assert_eq!(old.design.indexes.len(),1);
        let mut draft=old.design.clone();draft.columns.push(DesignColumn::new(99,"extra","TEXT DEFAULT 'new'"));
        let saved=apply(r,&scope,Some(&old),&draft).await.unwrap();
        let rows=r.execute("SELECT id,value,parent,extra FROM t").await.unwrap();assert_eq!(rows[0].rows[0],vec![Some("1".into()),Some("before".into()),Some("1".into()),Some("new".into())]);
        assert_eq!(saved.triggers.len(),1);assert_eq!(saved.design.indexes.len(),1);
        r.execute("INSERT INTO t(value,parent) VALUES('after',1)").await.unwrap();
        assert_eq!(r.execute("SELECT count(*) FROM audit").await.unwrap()[0].rows[0][0].as_deref(),Some("2"));
        let mut bad=saved.design.clone();bad.columns[1].definition="TEXT NOT NULL CHECK(value='invalid')".into();
        assert!(apply(r,&scope,Some(&saved),&bad).await.is_err());
        assert_eq!(load(r,&scope,"t").await.unwrap(),saved);
        assert_eq!(r.execute("PRAGMA foreign_keys").await.unwrap()[0].rows[0][0].as_deref(),Some("1"));
        let mut drop_dependency=saved.design.clone();
        drop_dependency.columns.retain(|c|c.name!="value");
        assert!(apply(r,&scope,Some(&saved),&drop_dependency).await.unwrap_err().message.contains("dependent schema"));
        assert_eq!(load(r,&scope,"t").await.unwrap(),saved);
        let mut invalid_foreign_key=saved.design.clone();
        invalid_foreign_key.constraints.push("FOREIGN KEY(id) REFERENCES p(id)".into());
        assert!(apply(r,&scope,Some(&saved),&invalid_foreign_key).await.unwrap_err().message.contains("Foreign key check"));
        assert_eq!(load(r,&scope,"t").await.unwrap(),saved);
        r.execute("ALTER TABLE t ADD COLUMN external TEXT").await.unwrap();
        assert!(apply(r,&scope,Some(&saved),&draft).await.unwrap_err().message.contains("outside"));
        session.close().await.unwrap();
    });
}

#[test]
fn sqlite_rebuild_keeps_inbound_references_views_sequence_and_rowid() {
    crate::runtime::runtime().block_on(async {
        let mut session=crate::connect(Arc::new(crate::sqlite::SqliteDriver::default()),crate::ConnectParams {file:Some(":memory:".into()),create_new:true,..Default::default()}).await.unwrap();
        let r=session.relational().unwrap(); let scope=Scope::new("test","main",None);
        r.execute("PRAGMA foreign_keys=ON; CREATE TABLE t(id INTEGER PRIMARY KEY AUTOINCREMENT,v TEXT); INSERT INTO t VALUES(1,'one'); UPDATE sqlite_sequence SET seq=100 WHERE name='t'; CREATE TABLE child(id INTEGER REFERENCES t(id)); INSERT INTO child VALUES(1); CREATE VIEW visible AS SELECT v FROM t").await.unwrap();
        let old=load(r,&scope,"t").await.unwrap();let mut draft=old.design.clone();draft.columns.push(DesignColumn::new(9,"new_field","TEXT"));
        let saved=apply(r,&scope,Some(&old),&draft).await.unwrap();
        assert_eq!(r.execute("SELECT * FROM visible").await.unwrap()[0].rows[0][0].as_deref(),Some("one"));
        assert_eq!(r.execute("SELECT id FROM child").await.unwrap()[0].rows[0][0].as_deref(),Some("1"));
        r.execute("INSERT INTO t(v) VALUES('two')").await.unwrap();
        assert_eq!(r.execute("SELECT max(id) FROM t").await.unwrap()[0].rows[0][0].as_deref(),Some("101"));
        let mut renamed=saved.design.clone();renamed.columns[1].name="renamed".into();
        apply(r,&scope,Some(&saved),&renamed).await.unwrap();
        assert_eq!(r.execute("SELECT * FROM visible").await.unwrap()[0].rows[0][0].as_deref(),Some("one"));
        r.execute("CREATE TABLE sparse(v TEXT); INSERT INTO sparse(rowid,v) VALUES(40,'forty')").await.unwrap();
        let old=load(r,&scope,"sparse").await.unwrap();let mut draft=old.design.clone();draft.columns.push(DesignColumn::new(9,"extra","TEXT"));
        apply(r,&scope,Some(&old),&draft).await.unwrap();
        assert_eq!(r.execute("SELECT rowid FROM sparse").await.unwrap()[0].rows[0][0].as_deref(),Some("40"));
        r.execute("CREATE TABLE generated_table(v INTEGER, doubled INTEGER GENERATED ALWAYS AS(v*2) STORED); INSERT INTO generated_table(v) VALUES(4)").await.unwrap();
        let original=load(r,&scope,"generated_table").await.unwrap();
        let mut draft=original.design.clone();draft.indexes.push("CREATE INDEX generated_idx ON generated_table(doubled)".into());
        let statements=plan(Dialect::Sqlite,&scope,Some(&original),&draft).unwrap();assert_eq!(statements.len(),1);
        apply(r,&scope,Some(&original),&draft).await.unwrap();
        assert_eq!(r.execute("SELECT doubled FROM generated_table").await.unwrap()[0].rows[0][0].as_deref(),Some("8"));
        session.close().await.unwrap();
    });
}

#[test]
fn live_table_design_create_alter_rename_and_constraints() {
    if std::env::var_os("NAVIDOG_LIVE").is_none() {
        return;
    }
    crate::runtime::runtime().block_on(async {
        for (driver, port, db, user) in [
            (
                Arc::new(crate::mysql::MySqlDriver::mysql()) as Arc<dyn crate::Driver>,
                13306,
                "sakila",
                "root",
            ),
            (
                Arc::new(crate::mysql::MySqlDriver::mariadb()) as Arc<dyn crate::Driver>,
                13307,
                "sakila",
                "root",
            ),
            (
                Arc::new(crate::postgres::PostgresDriver::default()) as Arc<dyn crate::Driver>,
                15432,
                "pagila",
                "postgres",
            ),
        ] {
            let mut session = crate::connect(
                driver,
                crate::ConnectParams {
                    host: "127.0.0.1".into(),
                    port,
                    database: Some(db.into()),
                    user: user.into(),
                    password: Some("navidog".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            let r = session.relational().unwrap();
            let d = r.dialect();
            let scope = Scope::new("test", db, Some("public".into()));
            let name = format!("navidog_design_{}", std::process::id());
            let table = d.qualified(&scope, &name);
            r.execute(&format!("DROP TABLE IF EXISTS {table}"))
                .await
                .unwrap();
            let mut draft = TableDesign::new();
            draft.name = name.clone();
            draft.columns.push(DesignColumn::new(
                2,
                "value",
                "VARCHAR(80) NOT NULL DEFAULT 'before'",
            ));
            draft.constraints.push(format!(
                "CONSTRAINT {} CHECK (id > 0)",
                d.quote_ident("positive_id")
            ));
            draft
                .columns
                .push(DesignColumn::new(3, "parent_id", "INTEGER"));
            draft.constraints.push(format!(
                "CONSTRAINT {} FOREIGN KEY (parent_id) REFERENCES {table}(id)",
                d.quote_ident(&format!("fk_design_{}_{port}", std::process::id()))
            ));
            draft.indexes.push(format!(
                "CREATE INDEX {} ON {table}(value)",
                d.quote_ident(&format!("ix_design_{}_{port}", std::process::id()))
            ));
            let saved = apply(r, &scope, None, &draft).await.unwrap();
            r.execute(&format!("INSERT INTO {table} (id) VALUES(1)"))
                .await
                .unwrap();
            let mut changed = saved.design.clone();
            changed.columns[1].name = "renamed".into();
            changed.columns[1].definition = "VARCHAR(100) NOT NULL DEFAULT 'after'".into();
            changed
                .columns
                .push(DesignColumn::new(99, "extra", "INTEGER DEFAULT 7"));
            let changed = apply(r, &scope, Some(&saved), &changed).await.unwrap();
            let result = r
                .execute(&format!("SELECT renamed,extra FROM {table}"))
                .await
                .unwrap();
            assert_eq!(
                result[0].rows[0],
                vec![Some("before".into()), Some("7".into())],
                "{port}"
            );
            let mut bad = changed.design.clone();
            bad.columns[0].definition = "INTEGER NOT NULL DEFAULT 'invalid_number'".into();
            assert!(apply(r, &scope, Some(&changed), &bad).await.is_err());
            assert_eq!(
                r.execute(&format!("SELECT renamed FROM {table}"))
                    .await
                    .unwrap()[0]
                    .rows[0][0]
                    .as_deref(),
                Some("before")
            );
            r.execute(&format!("DROP TABLE {table}")).await.unwrap();
            session.close().await.unwrap();
        }
    });
}
