use super::*;

use gpui_kit::component::WindowExt as _;
use gpui_kit::{Context, SharedString, Window};
use navidog_db::{ConnectParams, runtime};
use navidog_session::DriverRegistry;
use rust_i18n::t;

impl ConnectionForm {
    pub(crate) fn test(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.testing.take().is_some() {
            cx.notify();
            return;
        }
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
        self.testing = Some(cx.spawn_in(window, async move |this, cx| {
            let result = job
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r.map_err(|e| crate::db_text(&e)));
            let _ = this.update_in(cx, |this, window, cx| {
                this.testing = None;
                cx.notify();
                let (title, body): (SharedString, SharedString) = match result {
                    Ok(version) => (t!("conn.test_ok").to_string().into(), version.into()),
                    Err(e) => (t!("conn.test_failed").to_string().into(), e.into()),
                };
                window.open_alert_dialog(cx, move |alert, _, _| {
                    alert
                        .title(title.clone())
                        .description(body.clone())
                        .ok_text(t!("conn.ok").to_string())
                });
            });
        }));
        cx.notify();
    }
}
