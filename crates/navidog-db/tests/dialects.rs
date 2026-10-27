//! Copy As and unkeyed writes. These used to live next to the `Dialect` enum; the per-database
//! crates cannot be dev-dependencies of `navidog-sql` without a cycle.

use navidog_model::object::Scope;
use navidog_mysql::MysqlDialect;
use navidog_postgres::PostgresDialect;
use navidog_sql::dialect::{ChangeSet, Dialect, RowKey, RowUpdate, Written};
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

#[test]
fn drop_keywords_follow_the_catalog_rules() {
    use navidog_model::object::ObjectKind;
    assert_eq!(MysqlDialect.drop_keyword(ObjectKind::Table), Some("TABLE"));
    assert_eq!(SqliteDialect.drop_keyword(ObjectKind::Index), Some("INDEX"));
    assert_eq!(
        SqliteDialect.drop_keyword(ObjectKind::Trigger),
        Some("TRIGGER")
    );
    // MySQL indexes belong to the table, and PostgreSQL functions need their signature.
    assert_eq!(MysqlDialect.drop_keyword(ObjectKind::Index), None);
    assert_eq!(PostgresDialect.drop_keyword(ObjectKind::Function), None);
    assert_eq!(
        PostgresDialect.drop_keyword(ObjectKind::Sequence),
        Some("SEQUENCE")
    );
}

#[test]
fn paging_and_count_quote_the_product() {
    let s = scope(None);
    assert_eq!(
        SqliteDialect.select_page(&s, "t", " id > 1 ", "name DESC", Some(1000), 2000, false),
        "SELECT * FROM \"app\".\"t\" WHERE id > 1 ORDER BY name DESC LIMIT 1000 OFFSET 2000"
    );
    assert_eq!(
        MysqlDialect.select_page(&s, "t", "", "", None, 0, true),
        "SELECT * FROM `app`.`t`"
    );
    assert_eq!(
        PostgresDialect.count(&scope(Some("public")), "t", "a = 1"),
        "SELECT COUNT(*) FROM \"public\".\"t\" WHERE a = 1"
    );
}

#[test]
fn reserved_words_are_quoted_when_completion_inserts_them() {
    // A table or column named after a reserved word needs quotes; the keyword list decides.
    assert_eq!(PostgresDialect.typed_name("user"), "\"user\"");
    assert_eq!(PostgresDialect.typed_name("window"), "\"window\"");
    assert_eq!(PostgresDialect.typed_name("variadic"), "\"variadic\"");
    assert_eq!(MysqlDialect.typed_name("rank"), "`rank`");
    assert_eq!(MysqlDialect.typed_name("system"), "`system`");
    assert_eq!(SqliteDialect.typed_name("pragma"), "\"pragma\"");
    // A plain identifier stays bare, and PostgreSQL folds unquoted upper case.
    assert_eq!(PostgresDialect.typed_name("actor"), "actor");
    assert_eq!(PostgresDialect.typed_name("Actor"), "\"Actor\"");
}
