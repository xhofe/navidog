use super::*;

use gpui_kit::{Context, SharedString, Task, Window};
use navidog_db::{ConnectParams, runtime};
use navidog_session::DriverRegistry;
use rust_i18n::t;

#[derive(Clone)]
pub(super) enum TestOutcome {
    Ok(SharedString),
    Failed(SharedString),
}

/// The UI waiter plus a handle that actually stops the database probe.
/// Dropping the waiter only detaches the Tokio task.
pub(super) struct ConnectionTest {
    /// Dropping this cancels the waiter. The database probe is stopped separately.
    #[allow(dead_code)]
    task: Task<()>,
    abort: runtime::AbortHandle,
}

impl ConnectionForm {
    pub(crate) fn cancel_test(&mut self, cx: &mut Context<Self>) {
        if let Some(running) = self.testing.take() {
            running.abort.abort();
            cx.notify();
        }
    }

    pub(crate) fn dismiss_test_outcome(&mut self, cx: &mut Context<Self>) {
        self.test_outcome = None;
        cx.notify();
    }

    pub(crate) fn test(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.testing.is_some() {
            return;
        }
        self.test_outcome = None;
        let conn = match self.collect(cx) {
            Ok(c) => c,
            Err(e) => {
                self.error = Some(e);
                cx.notify();
                return;
            }
        };
        let Some(driver) = DriverRegistry::get(cx, &conn.driver) else {
            return;
        };
        let params = ConnectParams::from_connection(&conn, self.secrets(cx), driver.info());
        let job = runtime::spawn(navidog_db::test_connection(driver, params));
        let abort = job.abort_handle();
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = job
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r.map_err(|e| crate::db_text(&e)));
            let _ = this.update_in(cx, |this, _, cx| {
                this.testing = None;
                this.test_outcome = Some(match result {
                    Ok(version) => TestOutcome::Ok(
                        t!("conn.test_ok_version", version = version)
                            .to_string()
                            .into(),
                    ),
                    Err(e) => TestOutcome::Failed(e.into()),
                });
                cx.notify();
            });
        });
        self.testing = Some(ConnectionTest { task, abort });
        cx.notify();
    }
}
