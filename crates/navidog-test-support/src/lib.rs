//! Test-only helpers shared by headless UI tests. Never linked into the app.

use std::time::{Duration, Instant};

use gpui_kit::TestAppContext;

/// Runs the app and the tokio driver until `done`, or fails after ten seconds.
/// The 20 ms sleep lets driver threads make progress; `run_until_parked` alone
/// would spin without giving them time.
pub fn wait(cx: &mut TestAppContext, what: &str, done: impl Fn(&mut TestAppContext) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(cx) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
}
