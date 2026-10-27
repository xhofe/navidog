//! Driver-side SQL logging. Storage and the history viewer live in
//! `navidog-sql-log`; this module only times a driver future and records it.

use std::future::Future;

use crate::driver::DbResult;

pub use navidog_sql_log::*;

/// Runs `fut`, which sends `sql`, and records it with its time and error.
pub async fn logged<T>(
    tag: &Tag,
    sql: &str,
    fut: impl Future<Output = DbResult<T>>,
) -> DbResult<T> {
    let time = chrono::Local::now();
    let started = std::time::Instant::now();
    let result = fut.await;
    record(LogEntry {
        time,
        connection: tag.connection.clone(),
        session: tag.session,
        kind: tag.kind.clone(),
        elapsed: started.elapsed(),
        sql: sql.to_string(),
        error: result.as_ref().err().map(|e| e.to_string()),
    });
    result
}
