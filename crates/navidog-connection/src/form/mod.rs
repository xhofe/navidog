//! Connection form (11-connection.md §1.2, §2, §3).

mod browse;
mod pages;
mod secrets;
mod test_connection;
mod validation;

#[cfg(test)]
mod tests;

use gpui_kit::component::Disableable as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{InputState, Textarea, TextareaState};
use gpui_kit::component::select::SelectState;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{IndexPath, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Window, div,
};
use navidog_app_state::store::ConnectionStore;
use navidog_app_state::workspace::{Selection, Workspace};
use navidog_model::connection::{
    Connection, HostPreference, Member, SecretField, SshAuth, SslMode,
};
use navidog_ui::skin::confirm_with;
use navidog_ui::{palette, tokens};
use rust_i18n::t;

fn driver_flag(cx: &App, driver: &str, flag: impl FnOnce(&navidog_db::DriverInfo) -> bool) -> bool {
    navidog_session::DriverRegistry::get(cx, driver).is_some_and(|driver| flag(driver.info()))
}

pub fn open_new(driver: SharedString, window: &mut Window, cx: &mut App) {
    let mut conn = Connection::new(&driver);
    if let Selection::Group(g) = Workspace::global(cx).read(cx).selection() {
        conn.group = Some(g.clone());
    }
    open(conn, None, window, cx);
}

/// New connection prefilled from a URI (F-CONN-016).
pub fn open_from_uri(uri: navidog_app_state::uri::DatabaseUri, window: &mut Window, cx: &mut App) {
    let mut conn = Connection::new(&uri.driver);
    conn.name = match &uri.database {
        Some(db) if !uri.host.is_empty() => format!("{}/{db}", uri.host),
        _ => uri.host.clone(),
    };
    if !uri.host.is_empty() {
        conn.general.host = uri.host;
    }
    if uri.port.is_some() {
        conn.general.port = uri.port;
    }
    if !uri.user.is_empty() {
        conn.general.user = uri.user;
    }
    if uri.database.is_some() {
        conn.general.database = uri.database;
    }
    conn.general.save_password |= uri.password.is_some();
    open(conn, Some([uri.password, None, None]), window, cx);
}

pub fn open_edit(id: String, window: &mut Window, cx: &mut App) {
    crate::request_unlock(window, cx, move |ok, window, cx| {
        let store = ConnectionStore::global(cx);
        let Some(conn) = store.read(cx).get(&id).cloned() else {
            return;
        };
        // A failed unlock leaves the fields empty; saving then keeps the stored ciphertext.
        let secrets = if ok {
            SecretField::ALL.map(|f| store.update(cx, |s, _| s.reveal(&id, f)).ok().flatten())
        } else {
            [None, None, None]
        };
        open(conn, Some(secrets), window, cx);
    });
}

fn open(conn: Connection, secrets: Option<[Option<String>; 3]>, window: &mut Window, cx: &mut App) {
    let form = cx.new(|cx| ConnectionForm::new(conn, secrets, window, cx));
    let focus = form.clone();
    window.open_dialog(cx, move |dialog, window, _| {
        // Enter in a single-line field saves, like OK, instead of closing and losing the input
        confirm_with(dialog, &form, ConnectionForm::save)
            .w(tokens::conn_form::WIDTH.to_pixels(window.rem_size()))
            .close_button(false)
            .child(form.clone())
    });
    // `open_dialog` focuses the dialog itself, so focus the first field after it.
    window.defer(cx, move |window, cx| {
        focus.update(cx, |form, cx| {
            form.name.update(cx, |input, cx| input.focus(window, cx));
        });
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab_ {
    General,
    Advanced,
    Databases,
    Ssl,
    Ssh,
    Remark,
}

impl Tab_ {
    fn label(self) -> String {
        match self {
            Self::General => t!("conn.tab_general"),
            Self::Advanced => t!("conn.tab_advanced"),
            Self::Databases => t!("conn.tab_databases"),
            Self::Ssl => t!("conn.tab_ssl"),
            Self::Ssh => t!("conn.tab_ssh"),
            Self::Remark => t!("conn.tab_remark"),
        }
        .to_string()
    }
}

type Text = Entity<InputState>;
type Choice = Entity<SelectState<Vec<SharedString>>>;

struct ConnectionForm {
    draft: Connection,
    is_new: bool,
    /// The secrets were filled in for editing (not locked), so saving writes them all back
    secrets_loaded: bool,
    tab: Tab_,
    name: Text,
    /// PostgreSQL node type: single or multiple
    node_type: Choice,
    was_multi: bool,
    /// PostgreSQL multiple nodes, one `host:port` per line, the first one is `host`
    members: Entity<TextareaState>,
    host_preference: Choice,
    host: Text,
    port: Text,
    database: Text,
    file: Text,
    user: Text,
    password: Text,
    group: Choice,
    group_ids: Vec<Option<String>>,
    keepalive: Text,
    connect_timeout: Text,
    read_timeout: Text,
    write_timeout: Text,
    socket: Text,
    session_limit: Text,
    params: Text,
    initial_queries: Text,
    keepalive_on: bool,
    read_timeout_on: bool,
    write_timeout_on: bool,
    socket_on: bool,
    session_limit_on: bool,
    new_db: Text,
    attach_name: Text,
    attach_file: Text,
    ssl_mode: Choice,
    client_key: Text,
    client_cert: Text,
    ca_cert: Text,
    cipher: Text,
    crl: Text,
    ssh_host: Text,
    ssh_port: Text,
    ssh_user: Text,
    ssh_auth: Choice,
    ssh_password: Text,
    ssh_key: Text,
    ssh_passphrase: Text,
    remark: Entity<TextareaState>,
    testing: Option<test_connection::ConnectionTest>,
    test_outcome: Option<test_connection::TestOutcome>,
    error: Option<SharedString>,
}

fn text(
    value: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut Context<ConnectionForm>,
) -> Text {
    let value = value.into();
    cx.new(|cx| InputState::new(window, cx).default_value(value))
}

fn secret(value: Option<String>, window: &mut Window, cx: &mut Context<ConnectionForm>) -> Text {
    cx.new(|cx| {
        InputState::new(window, cx)
            .masked(true)
            .default_value(value.unwrap_or_default())
    })
}

fn choice(
    items: Vec<SharedString>,
    selected: usize,
    window: &mut Window,
    cx: &mut Context<ConnectionForm>,
) -> Choice {
    cx.new(|cx| SelectState::new(items, Some(IndexPath::default().row(selected)), window, cx))
}

fn opt_num(v: Option<u32>) -> String {
    v.map(|n| n.to_string()).unwrap_or_default()
}

const SSH_AUTHS: [SshAuth; 3] = [SshAuth::Password, SshAuth::PublicKey, SshAuth::Both];

impl ConnectionForm {
    fn new(
        conn: Connection,
        secrets: Option<[Option<String>; 3]>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let is_new = secrets.is_none();
        let secrets_loaded = secrets
            .as_ref()
            .is_some_and(|s| s.iter().any(Option::is_some))
            || is_new;
        let [pw, ssh_pw, passphrase] = secrets.unwrap_or_default();

        let store = ConnectionStore::global(cx);
        let groups = store.read(cx).groups().to_vec();
        let mut group_ids = vec![None];
        let mut group_labels: Vec<SharedString> =
            vec![t!("conn.my_connections").to_string().into()];
        for g in &groups {
            group_ids.push(Some(g.id.clone()));
            group_labels.push(format!("{}{}", "    ".repeat(depth(&groups, &g.id)), g.name).into());
        }
        let group_ix = group_ids.iter().position(|g| *g == conn.group).unwrap_or(0);

        let a = &conn.advanced;
        let ssl_ix = SslMode::ALL
            .iter()
            .position(|m| *m == conn.ssl.mode)
            .unwrap_or(1);
        let auth_ix = SSH_AUTHS
            .iter()
            .position(|m| *m == conn.ssh.auth)
            .unwrap_or(0);
        let auth_labels = vec![
            t!("conn.auth_password").to_string().into(),
            t!("conn.auth_public_key").to_string().into(),
            t!("conn.auth_both").to_string().into(),
        ];
        let name = text(conn.name.clone(), window, cx);
        let g = &conn.general;
        let node_type = choice(
            vec![
                t!("conn.node_single").to_string().into(),
                t!("conn.node_multiple").to_string().into(),
            ],
            usize::from(!g.members.is_empty()),
            window,
            cx,
        );
        cx.observe_in(&node_type, window, |this: &mut Self, _, window, cx| {
            let multi = this.multi_node(cx);
            if multi != this.was_multi {
                this.was_multi = multi;
                this.sync_members(multi, window, cx);
            }
            cx.notify();
        })
        .detach();
        let members = std::iter::once(Member {
            host: g.host.clone(),
            port: g.port,
        })
        .chain(g.members.iter().cloned())
        .map(|m| m.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        let pref_ix = HostPreference::ALL
            .iter()
            .position(|p| *p == g.host_preference)
            .unwrap_or(0);
        let host_preference = choice(
            vec![
                t!("conn.pref_any").to_string().into(),
                t!("conn.pref_read_write").to_string().into(),
                t!("conn.pref_read_only").to_string().into(),
            ],
            pref_ix,
            window,
            cx,
        );
        Self {
            is_new,
            secrets_loaded,
            tab: Tab_::General,
            name,
            was_multi: !g.members.is_empty(),
            node_type,
            members: cx.new(|cx| TextareaState::new(window, cx).default_value(members)),
            host_preference,
            host: text(conn.general.host.clone(), window, cx),
            port: text(
                conn.general.port.map(|p| p.to_string()).unwrap_or_default(),
                window,
                cx,
            ),
            database: text(
                conn.general.database.clone().unwrap_or_default(),
                window,
                cx,
            ),
            file: text(conn.general.file.clone().unwrap_or_default(), window, cx),
            user: text(conn.general.user.clone(), window, cx),
            password: secret(pw, window, cx),
            group: choice(group_labels, group_ix, window, cx),
            group_ids,
            keepalive: text(
                a.keepalive_secs.map_or("240".into(), |s| s.to_string()),
                window,
                cx,
            ),
            connect_timeout: text(opt_num(a.connect_timeout_secs), window, cx),
            read_timeout: text(opt_num(a.read_timeout_secs), window, cx),
            write_timeout: text(opt_num(a.write_timeout_secs), window, cx),
            socket: text(a.socket.clone().unwrap_or_default(), window, cx),
            session_limit: text(
                a.session_limit.map_or("10".into(), |s| s.to_string()),
                window,
                cx,
            ),
            params: text(a.params.clone(), window, cx),
            initial_queries: text(a.initial_queries.clone(), window, cx),
            keepalive_on: a.keepalive_secs.is_some(),
            read_timeout_on: a.read_timeout_secs.is_some(),
            write_timeout_on: a.write_timeout_secs.is_some(),
            socket_on: a.socket.is_some(),
            session_limit_on: a.session_limit.is_some(),
            new_db: text("", window, cx),
            attach_name: text("", window, cx),
            attach_file: text("", window, cx),
            ssl_mode: choice(
                SslMode::ALL
                    .iter()
                    .map(|m| SharedString::from(m.as_str()))
                    .collect(),
                ssl_ix,
                window,
                cx,
            ),
            client_key: text(conn.ssl.client_key.clone().unwrap_or_default(), window, cx),
            client_cert: text(conn.ssl.client_cert.clone().unwrap_or_default(), window, cx),
            ca_cert: text(conn.ssl.ca_cert.clone().unwrap_or_default(), window, cx),
            cipher: text(conn.ssl.cipher.clone().unwrap_or_default(), window, cx),
            crl: text(conn.ssl.crl.clone().unwrap_or_default(), window, cx),
            ssh_host: text(conn.ssh.host.clone(), window, cx),
            ssh_port: text(conn.ssh.port.to_string(), window, cx),
            ssh_user: text(conn.ssh.user.clone(), window, cx),
            ssh_auth: choice(auth_labels, auth_ix, window, cx),
            ssh_password: secret(ssh_pw, window, cx),
            ssh_key: text(conn.ssh.private_key.clone().unwrap_or_default(), window, cx),
            ssh_passphrase: secret(passphrase, window, cx),
            remark: {
                let value = conn.remark.clone();
                cx.new(|cx| TextareaState::new(window, cx).default_value(value))
            },
            testing: None,
            test_outcome: None,
            error: None,
            draft: conn,
        }
    }

    fn tabs(&self) -> Vec<Tab_> {
        if self.draft.supports_network() {
            vec![
                Tab_::General,
                Tab_::Advanced,
                Tab_::Databases,
                Tab_::Ssl,
                Tab_::Ssh,
                Tab_::Remark,
            ]
        } else {
            vec![Tab_::General, Tab_::Advanced, Tab_::Databases, Tab_::Remark]
        }
    }

    fn value(input: &Text, cx: &App) -> String {
        input.read(cx).value().trim().to_string()
    }

    fn opt(input: &Text, cx: &App) -> Option<String> {
        Some(Self::value(input, cx)).filter(|v| !v.is_empty())
    }

    fn selected(choice: &Choice, cx: &App) -> usize {
        choice
            .read(cx)
            .selected_index(cx)
            .map(|ix| ix.row)
            .unwrap_or(0)
    }

    fn multi_node(&self, cx: &App) -> bool {
        driver_flag(cx, &self.draft.driver, |info| info.multi_host)
            && Self::selected(&self.node_type, cx) == 1
    }

    /// Carries the host between the host/port fields and the first member line on a node type switch.
    fn sync_members(&mut self, to_multi: bool, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.members.read(cx).value().to_string();
        let mut lines: Vec<String> = text.lines().map(String::from).collect();
        if to_multi {
            let first = Member {
                host: Self::value(&self.host, cx),
                port: Self::value(&self.port, cx).parse().ok(),
            };
            match lines.first_mut() {
                Some(l) => *l = first.to_string(),
                None => lines.push(first.to_string()),
            }
            let text = lines.join("\n");
            self.members
                .update(cx, |s, cx| s.set_value(text, window, cx));
        } else if let Some(first) = lines.first().and_then(|l| Member::parse(l)) {
            let port = first.port.map(|p| p.to_string()).unwrap_or_default();
            self.host
                .update(cx, |s, cx| s.set_value(first.host, window, cx));
            self.port.update(cx, |s, cx| s.set_value(port, window, cx));
        }
    }
}

fn depth(groups: &[navidog_model::connection::Group], id: &str) -> usize {
    let mut d = 0;
    let mut cur = groups
        .iter()
        .find(|g| g.id == id)
        .and_then(|g| g.parent.clone());
    while let Some(p) = cur {
        d += 1;
        cur = groups
            .iter()
            .find(|g| g.id == p)
            .and_then(|g| g.parent.clone());
    }
    d
}

impl Render for ConnectionForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _ = window;
        let tabs = self.tabs();
        let selected = tabs.iter().position(|t| *t == self.tab).unwrap_or(0);
        let title = if self.is_new {
            t!(
                "conn.new_connection_of",
                driver = navidog_session::DriverRegistry::name(cx, &self.draft.driver)
            )
            .to_string()
        } else {
            t!("conn.edit_connection_title", name = self.draft.name).to_string()
        };
        let body: Vec<AnyElement> = match self.tab {
            Tab_::General => self.render_general(cx),
            Tab_::Advanced => self.render_advanced(cx),
            Tab_::Databases => self.render_databases(cx),
            Tab_::Ssl => self.render_ssl(cx),
            Tab_::Ssh => self.render_ssh(cx),
            Tab_::Remark => vec![
                div()
                    .w_full()
                    .child(
                        Textarea::new(&self.remark)
                            .w_full()
                            .h(tokens::form::LIST_HEIGHT.rems()),
                    )
                    .into_any_element(),
            ],
        };
        let testing = self.testing.is_some();
        let outcome = self.test_outcome.clone();
        let form = cx.entity().downgrade();
        let p = palette(cx);
        v_flex()
            .w_full()
            .min_w_0()
            .gap_3()
            .child(navidog_ui::skin::heading(title))
            .child(
                h_flex().justify_center().child(
                    TabBar::new("conn-tabs")
                        .segmented()
                        .selected_index(selected)
                        .children(tabs.iter().map(|t| Tab::new().label(t.label())))
                        .on_click(cx.listener(move |this, ix: &usize, _, cx| {
                            this.tab = tabs[*ix];
                            cx.notify();
                        })),
                ),
            )
            .child(
                v_flex()
                    .w_full()
                    .min_w_0()
                    .min_h(tokens::conn_form::BODY_HEIGHT.rems())
                    .px(tokens::form::MARGIN.rems())
                    .gap(tokens::form::ROW_GAP.rems())
                    .children(body),
            )
            .when_some(self.error.clone(), |this, e| {
                this.child(
                    div()
                        .px(tokens::form::MARGIN.rems())
                        .text_color(p.danger)
                        .child(e),
                )
            })
            .when(testing, |this| {
                let form = form.clone();
                this.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .px(tokens::form::MARGIN.rems())
                        .child(
                            Alert::info("test-status", t!("conn.test_in_progress").to_string())
                                .small()
                                .on_close(move |_, _, cx| {
                                    let _ = form.update(cx, |this, cx| this.cancel_test(cx));
                                }),
                        ),
                )
            })
            .when_some(outcome.filter(|_| !testing), |this, outcome| {
                let form = form.clone();
                let alert = match outcome {
                    test_connection::TestOutcome::Ok(text) => {
                        Alert::success("test-status", text.to_string())
                    }
                    test_connection::TestOutcome::Failed(text) => {
                        Alert::error("test-status", text.to_string())
                    }
                };
                this.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .px(tokens::form::MARGIN.rems())
                        .child(alert.small().on_close(move |_, _, cx| {
                            let _ = form.update(cx, |this, cx| this.dismiss_test_outcome(cx));
                        })),
                )
            })
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .justify_between()
                    .child(
                        Button::new("test")
                            .label(t!("conn.test").to_string())
                            .disabled(testing)
                            .on_click(cx.listener(|this, _, window, cx| this.test(window, cx))),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("cancel")
                                    .label(t!("conn.cancel").to_string())
                                    .on_click(|_, window, cx| window.close_dialog(cx)),
                            )
                            .child(
                                Button::new("ok")
                                    .primary()
                                    .label(t!("conn.ok").to_string())
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.save(window, cx)),
                                    ),
                            ),
                    ),
            )
    }
}
