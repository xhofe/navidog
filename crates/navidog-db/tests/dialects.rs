//! Copy As and unkeyed writes. These used to live next to the `Dialect` enum; the per-database
//! crates cannot be dev-dependencies of `navidog-sql` without a cycle.

use navidog_db::dialect::{ChangeSet, Dialect, RowKey, RowUpdate, Written};
use navidog_model::object::Scope;
use navidog_mysql::MysqlDialect;
use navidog_postgres::PostgresDialect;
use navidog_sqlite::SqliteDialect;

fn scope(schema: Option<&str>) -> Scope {
    Scope::new("c", "app", schema.map(str::to_string))
}

#[test]
fn copy_as_insert_and_update_quote_nulls() {
    let s = scope(None);
    let cols = vec!["id".to_string(), "name".to_string()];
    let rows = vec![
        vec![Some("1".into()), Some("a".into())],
        vec![Some("2".into()), None],
    ];
    assert_eq!(
        SqliteDialect.insert_statements(&s, "t", &cols, &rows),
        "INSERT INTO \"app\".\"t\" (\"id\", \"name\") VALUES ('1', 'a');\nINSERT INTO \"app\".\"t\" (\"id\", \"name\") VALUES ('2', NULL);"
    );
    assert_eq!(
        SqliteDialect.update_statements(&s, "t", &cols, &["id".into()], &rows[..1]),
        "UPDATE \"app\".\"t\" SET \"name\" = 'a' WHERE \"id\" = '1';"
    );
    assert_eq!(
        MysqlDialect.insert_statements(&s, "t", &cols, &rows[1..]),
        "INSERT INTO `app`.`t` (`id`, `name`) VALUES ('2', NULL);"
    );
}

#[test]
fn mysql_limits_unkeyed_writes_and_renders_nulls() {
    let s = scope(None);
    let changes = ChangeSet {
        deletes: vec![RowKey(vec![("id".into(), Some("3".into()))])],
        updates: vec![RowUpdate {
            key: RowKey(vec![("id".into(), Some("1".into())), ("note".into(), None)]),
            values: vec![
                ("name".into(), Written::text(Some("O'Brien".into()))),
                ("note".into(), Written::text(None)),
            ],
        }],
        inserts: vec![vec![
            ("id".into(), Written::text(Some("9".into()))),
            ("name".into(), Written::text(None)),
        ]],
    };
    let sql = MysqlDialect.render_changes(&s, "t", &changes, false);
    assert_eq!(
        sql,
        [
            "DELETE FROM `app`.`t` WHERE `id` = '3' LIMIT 1",
            "UPDATE `app`.`t` SET `name` = 'O''Brien', `note` = NULL WHERE `id` = '1' AND `note` IS NULL LIMIT 1",
            "INSERT INTO `app`.`t` (`id`, `name`) VALUES ('9', NULL)",
        ]
    );
    let keyed = MysqlDialect.render_changes(&s, "t", &changes, true);
    assert!(
        keyed.iter().all(|s| !s.contains("LIMIT 1")),
        "a primary key does not add LIMIT 1: {keyed:?}"
    );
    let pg = PostgresDialect.render_changes(&scope(Some("public")), "t", &changes, false);
    assert!(pg[0].ends_with("WHERE \"id\" = '3'"), "{pg:?}");
    assert!(!pg[0].contains("LIMIT"), "{pg:?}");
}
