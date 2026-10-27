use super::*;

use gpui_kit::component::WindowExt as _;
use gpui_kit::{App, Context, Window};
use navidog_app_state::store::ConnectionStore;
use navidog_app_state::workspace::{Selection, Workspace};
use navidog_db::Secrets;
use navidog_model::connection::SecretField;
use navidog_session::sessions::Sessions;
use rust_i18n::t;

impl ConnectionForm {
    pub(crate) fn secrets(&self, cx: &App) -> Secrets {
        Secrets {
            password: Self::opt(&self.password, cx),
            ssh_password: Self::opt(&self.ssh_password, cx),
            ssh_passphrase: Self::opt(&self.ssh_passphrase, cx),
        }
    }

    pub(crate) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let conn = match self.collect(cx) {
            Ok(c) => c,
            Err(e) => {
                self.error = Some(e);
                self.tab = Tab_::General;
                cx.notify();
                return;
            }
        };
        // When editing without the secrets filled in (master password locked), empty means keep
        let keep_if_empty = !self.is_new && !self.secrets_loaded;
        let pick = |input: &Text, cx: &App| {
            let v = input.read(cx).value().to_string();
            if v.is_empty() && keep_if_empty {
                None
            } else {
                Some(v)
            }
        };
        let secrets = vec![
            (SecretField::Password, pick(&self.password, cx)),
            (SecretField::SshPassword, pick(&self.ssh_password, cx)),
            (SecretField::SshPassphrase, pick(&self.ssh_passphrase, cx)),
        ];
        let id = conn.id.clone();
        let was_open = Sessions::global(cx).read(cx).state(&id).is_open();
        if ConnectionStore::global(cx).read(cx).is_locked() {
            let form = cx.entity().downgrade();
            crate::request_unlock(window, cx, move |ok, window, cx| {
                let _ = form.update(cx, |form, cx| {
                    if ok {
                        form.save(window, cx);
                    } else {
                        form.error = Some(
                            navidog_app_state::store::StoreError::Locked
                                .localized_message()
                                .into(),
                        );
                        cx.notify();
                    }
                });
            });
            return;
        }
        let store = ConnectionStore::global(cx);
        if let Err(e) = store.update(cx, |s, cx| s.upsert(conn, &secrets, cx)) {
            self.error = Some(e.localized_message().into());
            cx.notify();
            return;
        }
        window.close_dialog(cx);
        Workspace::global(cx).update(cx, |w, cx| w.select(Selection::Connection(id.clone()), cx));
        if was_open {
            window.open_alert_dialog(cx, move |alert, _, _| {
                let id = id.clone();
                alert
                    .title(t!("conn.reopen_required").to_string())
                    .show_cancel(true)
                    .ok_text(t!("conn.reopen_now").to_string())
                    .cancel_text(t!("conn.later").to_string())
                    .on_ok(move |_, window, cx| {
                        crate::reopen_connection(id.clone(), window, cx);
                        true
                    })
            });
        }
    }
}
