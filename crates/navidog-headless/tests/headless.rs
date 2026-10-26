//! Opening a connection and running SQL without linking GPUI.

use std::sync::Arc;

use navidog_db::{ConnectParams, connect, runtime, sqlite::SqliteDriver};
use navidog_model::connection::Connection;

#[test]
fn a_sqlite_connection_round_trips_sql_without_the_ui_stack() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("headless.db");
    runtime::runtime().block_on(async {
        let mut session = connect(
            Arc::new(SqliteDriver::default()),
            ConnectParams {
                file: Some(file.clone()),
                create_new: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let rel = session.relational().unwrap();
        rel.execute(
            "CREATE TABLE items(id INTEGER PRIMARY KEY, name TEXT); \
             INSERT INTO items(name) VALUES ('a'), ('b')",
        )
        .await
        .unwrap();
        let sets = rel
            .execute("SELECT name FROM items ORDER BY id")
            .await
            .unwrap();
        assert_eq!(
            sets[0].rows,
            vec![vec![Some("a".to_string())], vec![Some("b".to_string())]]
        );
        session.close().await.unwrap();
    });
}

#[test]
fn the_domain_model_serializes_without_the_ui_stack() {
    let mut connection = Connection::new("sqlite");
    connection.name = "headless".into();
    let json = serde_json::to_string(&connection).unwrap();
    let read: Connection = serde_json::from_str(&json).unwrap();
    assert_eq!(read, connection);
}
