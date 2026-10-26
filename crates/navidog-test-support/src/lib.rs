//! Test-only helpers shared by headless UI tests. Never linked into the app.

use std::time::{Duration, Instant};

use std::path::Path;
use std::sync::Arc;

use gpui_kit::TestAppContext;
use navidog_db::{ConnectParams, connect, runtime};
use navidog_sqlite::SqliteDriver;

/// Runs the app and the tokio driver until `done`, or fails after ten seconds.
/// The 20 ms sleep lets driver threads make progress; `run_until_parked` alone
/// would spin without giving them time.
pub fn wait(cx: &mut TestAppContext, what: &str, done: impl Fn(&mut TestAppContext) -> bool) {
    wait_with(
        cx,
        what,
        Duration::from_secs(10),
        Duration::from_millis(20),
        false,
        done,
    );
}

/// Same loop with explicit budget and clock policy. `advance_clock` is needed
/// by dialog animations, which only move when headless time advances.
pub fn wait_with(
    cx: &mut TestAppContext,
    what: &str,
    timeout: Duration,
    tick: Duration,
    advance_clock: bool,
    done: impl Fn(&mut TestAppContext) -> bool,
) {
    let deadline = Instant::now() + timeout;
    while !done(cx) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        cx.run_until_parked();
        if advance_clock {
            cx.executor().advance_clock(tick);
        }
        std::thread::sleep(tick);
    }
}

/// Runs `statements` against a SQLite file and returns the last result set's rows.
pub fn sql(file: &Path, statements: &str) -> Vec<Vec<Option<String>>> {
    runtime::runtime().block_on(async {
        let params = ConnectParams {
            file: Some(file.to_path_buf()),
            create_new: !file.exists(),
            ..Default::default()
        };
        let mut session = connect(Arc::new(SqliteDriver::default()), params)
            .await
            .unwrap();
        let sets = session
            .relational()
            .unwrap()
            .execute(statements)
            .await
            .unwrap();
        session.close().await.unwrap();
        sets.into_iter()
            .last()
            .map(|set| set.rows)
            .unwrap_or_default()
    })
}
