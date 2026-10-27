//! Unit tests for the results helpers.

use super::*;

use std::collections::BTreeMap;
use std::time::Duration;

use navidog_db::Column;
use navidog_db::ColumnOrigin;
use navidog_db::ResultSet;
use navidog_sql::dialect::{ChangeSet, RowKey, RowUpdate, Written};
use rust_i18n::t;

use super::editing::{rename_changes, single_table};
use super::tabs::{message_block, summary_message};

fn col(name: &str, table: Option<(&str, &str)>) -> Column {
    Column {
        name: name.into(),
        type_name: String::new(),
        origin: table.map(|(t, c)| ColumnOrigin {
            schema: "s".into(),
            table: t.into(),
            column: c.into(),
        }),
    }
}

#[test]
fn single_table_needs_a_select_from_one_table() {
    let cols = [
        col("id", Some(("actor", "actor_id"))),
        col("n", Some(("actor", "first_name"))),
    ];
    assert_eq!(
        single_table("-- x\nselect * from actor", &cols),
        Some(("s".into(), "actor".into()))
    );
    assert_eq!(single_table("CALL p()", &cols), None);
    let joined = [
        col("id", Some(("actor", "actor_id"))),
        col("t", Some(("film", "title"))),
    ];
    assert_eq!(single_table("SELECT 1", &joined), None);
    let expr = [col("id", Some(("actor", "actor_id"))), col("x", None)];
    assert_eq!(single_table("SELECT 1", &expr), None);
    let twice = [
        col("a", Some(("actor", "actor_id"))),
        col("b", Some(("actor", "actor_id"))),
    ];
    assert_eq!(single_table("SELECT 1", &twice), None);
}

#[test]
fn aliases_become_table_columns() {
    let names = BTreeMap::from([("n".to_string(), "first_name".to_string())]);
    let changes = ChangeSet {
        updates: vec![RowUpdate {
            key: RowKey(vec![("actor_id".into(), Some("1".into()))]),
            values: vec![("n".into(), Written::text(Some("A".into())))],
        }],
        ..ChangeSet::default()
    };
    let out = rename_changes(changes, &names);
    assert_eq!(out.updates[0].values[0].0, "first_name");
    assert_eq!(out.updates[0].key.0[0].0, "actor_id");
}

#[test]
fn session_loss_guidance_reaches_query_messages_summary_and_console() {
    let mut error = navidog_db::DbError::coded("08006", "transport disconnected");
    error.disconnected = true;
    let result = Err(error);
    let detail = "08006 - transport disconnected";
    for text in [
        message_block("SELECT 1", &result, Duration::ZERO),
        summary_message(&result),
        crate::console::format_result(&navidog_postgres::PostgresDialect, &result, 0.0),
    ] {
        assert!(text.contains(detail));
        assert!(text.contains("Reopen the connection") || text.contains("重新打开连接"));
    }
}

#[test]
fn messages_follow_navicat() {
    let ok: navidog_db::DbResult<Vec<ResultSet>> = Ok(vec![ResultSet {
        columns: vec![col("a", None)],
        notices: vec!["NOTICE: hi".into()],
        ..ResultSet::default()
    }]);
    assert_eq!(
        message_block("SELECT 1", &ok, Duration::from_millis(2)),
        format!(
            "SELECT 1\n> NOTICE: hi\n> OK\n> {}: 0.002s\n\n",
            t!("query.time")
        )
    );
    let upd: navidog_db::DbResult<Vec<ResultSet>> = Ok(vec![ResultSet {
        affected_rows: Some(3),
        ..ResultSet::default()
    }]);
    assert!(message_block("UPDATE t", &upd, Duration::ZERO).contains(": 3\n"));
    let err: navidog_db::DbResult<Vec<ResultSet>> =
        Err(navidog_db::DbError::coded(1146, "Table 'x' doesn't exist"));
    assert!(
        message_block("SELECT", &err, Duration::ZERO)
            .contains("> 1146 - Table 'x' doesn't exist\n")
    );
}

#[test]
fn pinned_hover_lists_when_and_where() {
    let info = RunInfo {
        at: chrono::Local::now(),
        query_time: Duration::from_millis(1500),
        connection: "local".into(),
        connection_id: "c1".into(),
        database: Some("sakila".into()),
        schema: None,
        search_path: None,
        context_id: None,
        context_known: true,
        dialect: Some(std::sync::Arc::new(navidog_mysql::MysqlDialect)),
    };
    let lines = info.lines("SELECT 1");
    assert!(lines.iter().any(|(_, v)| v == "local"));
    assert!(lines.iter().any(|(_, v)| v == "sakila"));
    assert!(lines.iter().any(|(_, v)| v == "SELECT 1"));
    assert!(lines.iter().any(|(_, v)| v == "1.500s"));
}
