use super::*;

use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::radio::{Radio, RadioGroup};
use gpui_kit::component::select::Select;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _,
    TestSupportExt as _, div,
};
use navidog_model::connection::{AttachedDatabase, DatabaseItem};
use navidog_ui::skin::{FormRow, form_note};
use navidog_ui::{palette, tokens};
use rust_i18n::t;

impl ConnectionForm {
    fn check(
        &self,
        id: &'static str,
        label: String,
        checked: bool,
        f: fn(&mut Self, bool),
        cx: &mut Context<Self>,
    ) -> Checkbox {
        Checkbox::new(id)
            .label(label)
            .checked(checked)
            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                f(this, *checked);
                cx.notify();
            }))
    }

    pub(crate) fn render_general(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let w = tokens::form::FIELD_WIDTH.rems();
        let mut rows = vec![
            FormRow::new(
                t!("conn.name").to_string(),
                Input::new(&self.name).id("conn-name").w(w),
            )
            .into_any_element(),
            FormRow::new(
                t!("conn.add_to").to_string(),
                Select::new(&self.group).w(tokens::form::POPUP_WIDTH.rems()),
            )
            .into_any_element(),
        ];
        if self.draft.supports_network() {
            let postgres = super::driver_flag(cx, &self.draft.driver, |info| info.multi_host);
            if postgres {
                rows.push(
                    FormRow::new(
                        t!("conn.node_type").to_string(),
                        Select::new(&self.node_type).w(tokens::form::POPUP_WIDTH_SHORT.rems()),
                    )
                    .into_any_element(),
                );
            }
            if self.multi_node(cx) {
                rows.push(
                    FormRow::new(
                        t!("conn.members").to_string(),
                        Textarea::new(&self.members)
                            .w(w)
                            .h(tokens::form::LIST_HEIGHT.rems()),
                    )
                    .into_any_element(),
                );
                rows.push(
                    FormRow::new(
                        t!("conn.host_preference").to_string(),
                        Select::new(&self.host_preference)
                            .w(tokens::form::POPUP_WIDTH_MEDIUM.rems()),
                    )
                    .into_any_element(),
                );
            } else {
                rows.push(
                    FormRow::new(t!("conn.host").to_string(), Input::new(&self.host).w(w))
                        .into_any_element(),
                );
                rows.push(
                    FormRow::new(
                        t!("conn.port").to_string(),
                        Input::new(&self.port).w(tokens::form::FIELD_WIDTH_SHORT.rems()),
                    )
                    .into_any_element(),
                );
            }
            if postgres {
                rows.push(
                    FormRow::new(
                        t!("conn.initial_database").to_string(),
                        Input::new(&self.database).w(w),
                    )
                    .into_any_element(),
                );
            }
        } else {
            let create_new = self.draft.general.create_new;
            rows.push(
                FormRow::new(
                    t!("conn.sqlite_type").to_string(),
                    RadioGroup::vertical("sqlite-type")
                        .selected_index(Some(usize::from(create_new)))
                        .child(Radio::new("existing").label(t!("conn.sqlite_existing").to_string()))
                        // The group stretches to the column, so the row's center is
                        // empty space past the label. The label hugs its text.
                        .child(
                            Radio::new("new")
                                .accessibility_label(t!("conn.sqlite_new").to_string())
                                .child(
                                    div()
                                        .id("sqlite-new")
                                        .self_start()
                                        .child(t!("conn.sqlite_new").to_string())
                                        .test_support(),
                                ),
                        )
                        .on_click(cx.listener(|this, ix: &usize, _, cx| {
                            this.draft.general.create_new = *ix == 1;
                            cx.notify();
                        })),
                )
                .into_any_element(),
            );
            rows.push(
                FormRow::new(
                    t!("conn.database_file").to_string(),
                    self.path_field("file-browse", &self.file, false, cx),
                )
                .into_any_element(),
            );
        }
        rows.push(
            FormRow::new(t!("conn.user").to_string(), Input::new(&self.user).w(w))
                .into_any_element(),
        );
        rows.push(
            FormRow::new(
                t!("conn.password").to_string(),
                Input::new(&self.password).w(w),
            )
            .into_any_element(),
        );
        rows.push(
            FormRow::bare(self.check(
                "save-password",
                t!("conn.save_password").to_string(),
                self.draft.general.save_password,
                |t, v| t.draft.general.save_password = v,
                cx,
            ))
            .into_any_element(),
        );
        rows.push(
            FormRow::bare(form_note(t!("conn.password_note").to_string(), cx)).into_any_element(),
        );
        rows
    }

    fn toggled_number(
        &self,
        id: &'static str,
        label: String,
        on: bool,
        input: &Text,
        f: fn(&mut Self, bool),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        FormRow::bare(
            h_flex()
                .gap_2()
                .child(self.check(id, label, on, f, cx))
                .child(
                    Input::new(input)
                        .w(tokens::form::FIELD_WIDTH_SHORT.rems())
                        .disabled(!on),
                ),
        )
        .compact()
        .into_any_element()
    }

    pub(crate) fn render_advanced(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut rows = vec![
            FormRow::new(
                t!("conn.settings_location").to_string(),
                div().text_color(palette(cx).label_secondary).child(
                    navidog_model::paths::profile_dir(&self.draft.id)
                        .display()
                        .to_string(),
                ),
            )
            .compact()
            .into_any_element(),
        ];
        let network = self.draft.supports_network();
        let mysql = super::driver_flag(cx, &self.draft.driver, |info| info.protocol_timeouts);
        let pg = super::driver_flag(cx, &self.draft.driver, |info| info.connection_params);
        if network {
            rows.push(self.toggled_number(
                "keepalive",
                t!("conn.keepalive").to_string(),
                self.keepalive_on,
                &self.keepalive,
                |t, v| t.keepalive_on = v,
                cx,
            ));
        }
        if mysql {
            rows.push(
                FormRow::new(
                    t!("conn.connect_timeout")
                        .to_string()
                        .trim_end_matches(':')
                        .trim_end_matches('：')
                        .to_string(),
                    Input::new(&self.connect_timeout).w(tokens::form::FIELD_WIDTH_SHORT.rems()),
                )
                .compact()
                .into_any_element(),
            );
            rows.push(self.toggled_number(
                "read-timeout",
                t!("conn.read_timeout").to_string(),
                self.read_timeout_on,
                &self.read_timeout,
                |t, v| t.read_timeout_on = v,
                cx,
            ));
            rows.push(self.toggled_number(
                "write-timeout",
                t!("conn.write_timeout").to_string(),
                self.write_timeout_on,
                &self.write_timeout,
                |t, v| t.write_timeout_on = v,
                cx,
            ));
            rows.push(
                FormRow::bare(self.check(
                    "compression",
                    t!("conn.compression").to_string(),
                    self.draft.advanced.compression,
                    |t, v| t.draft.advanced.compression = v,
                    cx,
                ))
                .compact()
                .into_any_element(),
            );
        }
        rows.push(
            FormRow::bare(self.check(
                "auto-connect",
                t!("conn.auto_connect").to_string(),
                self.draft.advanced.auto_connect,
                |t, v| t.draft.advanced.auto_connect = v,
                cx,
            ))
            .compact()
            .into_any_element(),
        );
        if mysql {
            rows.push(
                FormRow::bare(
                    h_flex()
                        .gap_2()
                        .child(self.check(
                            "socket",
                            t!("conn.socket").to_string(),
                            self.socket_on,
                            |t, v| t.socket_on = v,
                            cx,
                        ))
                        .child(
                            Input::new(&self.socket)
                                .w(tokens::form::POPUP_WIDTH_SHORT.rems())
                                .disabled(!self.socket_on),
                        ),
                )
                .compact()
                .into_any_element(),
            );
        }
        if network {
            rows.push(self.toggled_number(
                "session-limit",
                t!("conn.session_limit").to_string(),
                self.session_limit_on,
                &self.session_limit,
                |t, v| t.session_limit_on = v,
                cx,
            ));
        }
        if pg {
            rows.push(
                FormRow::new(
                    t!("conn.params").to_string(),
                    Input::new(&self.params).w(tokens::form::FIELD_WIDTH.rems()),
                )
                .compact()
                .into_any_element(),
            );
        }
        rows.push(
            FormRow::new(
                t!("conn.initial_queries").to_string(),
                Input::new(&self.initial_queries).w(tokens::form::FIELD_WIDTH.rems()),
            )
            .compact()
            .into_any_element(),
        );
        rows
    }

    pub(crate) fn render_databases(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let p = palette(cx);
        let list = |rows: Vec<AnyElement>| {
            v_flex()
                .w_full()
                .min_w_0()
                .h(tokens::form::LIST_HEIGHT.rems())
                .overflow_hidden()
                .border_1()
                .border_color(p.separator)
                .bg(p.control_bg)
                .children(rows)
        };
        let check_col = tokens::form::FIELD_WIDTH_SHORT;
        let header = |first_fixed: bool, cols: Vec<String>| {
            let mut row = h_flex()
                .w_full()
                .min_w_0()
                .h(tokens::form::LIST_HEADER_HEIGHT.rems())
                .px_2()
                .gap_2()
                .border_b_1()
                .border_color(p.separator)
                .text_color(p.label);
            for (ix, col) in cols.into_iter().enumerate() {
                let cell = div().min_w_0().overflow_hidden().child(col);
                row = row.child(if first_fixed && ix == 0 {
                    cell.w(check_col.rems()).flex_none()
                } else {
                    cell.flex_1()
                });
            }
            row
        };
        if self.draft.supports_network() {
            let items = self.draft.databases.items.clone();
            let mut rows = vec![
                header(
                    true,
                    vec![
                        t!("conn.col_auto_open").to_string(),
                        t!("conn.col_name").to_string(),
                    ],
                )
                .into_any_element(),
            ];
            for (ix, item) in items.iter().enumerate() {
                rows.push(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .h(tokens::form::LIST_ROW_HEIGHT.rems())
                        .px_2()
                        .gap_2()
                        .child(
                            div().w(check_col.rems()).flex_none().child(
                                Checkbox::new(("auto-open", ix))
                                    .checked(item.auto_open)
                                    .on_click(cx.listener(move |this, v: &bool, _, cx| {
                                        this.draft.databases.items[ix].auto_open = *v;
                                        cx.notify();
                                    })),
                            ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .child(item.name.clone()),
                        )
                        .into_any_element(),
                );
            }
            vec![
                FormRow::bare(self.check(
                    "custom-list",
                    t!("conn.use_custom_db_list").to_string(),
                    self.draft.databases.custom_list,
                    |t, v| t.draft.databases.custom_list = v,
                    cx,
                ))
                .into_any_element(),
                FormRow::bare(list(rows)).into_any_element(),
                FormRow::bare(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .gap_1()
                        .child(Input::new(&self.new_db).flex_1().min_w_0())
                        .child(Button::new("db-add").label("+").on_click(cx.listener(
                            |this, _, window, cx| {
                                let name = Self::value(&this.new_db, cx);
                                if !name.is_empty()
                                    && !this.draft.databases.items.iter().any(|i| i.name == name)
                                {
                                    this.draft.databases.items.push(DatabaseItem {
                                        name,
                                        auto_open: false,
                                    });
                                    this.new_db.update(cx, |s, cx| s.set_value("", window, cx));
                                }
                                cx.notify();
                            },
                        )))
                        .child(Button::new("db-remove").label("−").on_click(cx.listener(
                            |this, _, _, cx| {
                                this.draft.databases.items.pop();
                                cx.notify();
                            },
                        ))),
                )
                .into_any_element(),
            ]
        } else {
            let mut rows = vec![
                header(
                    false,
                    vec![
                        t!("conn.col_name").to_string(),
                        t!("conn.col_file").to_string(),
                    ],
                )
                .into_any_element(),
            ];
            for item in &self.draft.databases.attached {
                rows.push(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .h(tokens::form::LIST_ROW_HEIGHT.rems())
                        .px_2()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .child(item.name.clone()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .child(item.file.clone()),
                        )
                        .into_any_element(),
                );
            }
            vec![
                FormRow::bare(list(rows)).into_any_element(),
                FormRow::new(
                    t!("conn.new_database_name").to_string(),
                    Input::new(&self.attach_name).w(tokens::form::FIELD_WIDTH.rems()),
                )
                .into_any_element(),
                FormRow::new(
                    t!("conn.database_file").to_string(),
                    self.path_field("attach-browse", &self.attach_file, false, cx),
                )
                .into_any_element(),
                FormRow::bare(
                    h_flex()
                        .gap_1()
                        .child(Button::new("attach-add").label("+").on_click(cx.listener(
                            |this, _, window, cx| {
                                let name = Self::value(&this.attach_name, cx);
                                let file = Self::value(&this.attach_file, cx);
                                if !name.is_empty() && !file.is_empty() {
                                    this.draft
                                        .databases
                                        .attached
                                        .push(AttachedDatabase { name, file });
                                    this.attach_name
                                        .update(cx, |s, cx| s.set_value("", window, cx));
                                    this.attach_file
                                        .update(cx, |s, cx| s.set_value("", window, cx));
                                }
                                cx.notify();
                            },
                        )))
                        .child(
                            Button::new("attach-remove")
                                .label("−")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.draft.databases.attached.pop();
                                    cx.notify();
                                })),
                        ),
                )
                .into_any_element(),
            ]
        }
    }

    pub(crate) fn render_ssl(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let pg = super::driver_flag(cx, &self.draft.driver, |info| info.chooses_ssl_mode);
        let mut rows = vec![
            FormRow::bare(self.check(
                "use-ssl",
                t!("conn.use_ssl").to_string(),
                self.draft.ssl.enabled,
                |t, v| t.draft.ssl.enabled = v,
                cx,
            ))
            .into_any_element(),
        ];
        if !self.draft.ssl.enabled {
            return rows;
        }
        if pg {
            rows.push(
                FormRow::new(
                    t!("conn.ssl_mode").to_string(),
                    Select::new(&self.ssl_mode).w(tokens::form::POPUP_WIDTH_MEDIUM.rems()),
                )
                .into_any_element(),
            );
        }
        rows.push(
            FormRow::bare(self.check(
                "use-auth",
                t!("conn.use_auth").to_string(),
                self.draft.ssl.use_auth,
                |t, v| t.draft.ssl.use_auth = v,
                cx,
            ))
            .into_any_element(),
        );
        if self.draft.ssl.use_auth {
            rows.push(
                FormRow::new(
                    t!("conn.client_key").to_string(),
                    self.path_field("key-browse", &self.client_key, false, cx),
                )
                .into_any_element(),
            );
            rows.push(
                FormRow::new(
                    t!("conn.client_cert").to_string(),
                    self.path_field("cert-browse", &self.client_cert, false, cx),
                )
                .into_any_element(),
            );
            let ca_label = if pg {
                t!("conn.root_cert")
            } else {
                t!("conn.ca_cert")
            };
            rows.push(
                FormRow::new(
                    ca_label.to_string(),
                    self.path_field("ca-browse", &self.ca_cert, false, cx),
                )
                .into_any_element(),
            );
            if pg {
                rows.push(
                    FormRow::new(
                        t!("conn.crl").to_string(),
                        self.path_field("crl-browse", &self.crl, false, cx),
                    )
                    .into_any_element(),
                );
            } else {
                rows.push(
                    FormRow::bare(self.check(
                        "verify-ca",
                        t!("conn.verify_ca").to_string(),
                        self.draft.ssl.verify_server_cert,
                        |t, v| t.draft.ssl.verify_server_cert = v,
                        cx,
                    ))
                    .into_any_element(),
                );
                rows.push(
                    FormRow::new(
                        t!("conn.cipher").to_string(),
                        Input::new(&self.cipher).w(tokens::form::FIELD_WIDTH.rems()),
                    )
                    .into_any_element(),
                );
            }
        }
        rows
    }

    pub(crate) fn render_ssh(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let s = &self.draft.ssh;
        let w = tokens::form::FIELD_WIDTH.rems();
        let mut rows = vec![
            FormRow::bare(self.check(
                "use-ssh",
                t!("conn.use_ssh").to_string(),
                s.enabled,
                |t, v| t.draft.ssh.enabled = v,
                cx,
            ))
            .into_any_element(),
        ];
        if !s.enabled {
            return rows;
        }
        let auth_ix = self
            .ssh_auth
            .read(cx)
            .selected_index(cx)
            .map(|ix| ix.row)
            .unwrap_or(0);
        let auth = SSH_AUTHS[auth_ix.min(2)];
        rows.push(
            FormRow::new(t!("conn.host").to_string(), Input::new(&self.ssh_host).w(w))
                .into_any_element(),
        );
        rows.push(
            FormRow::new(
                t!("conn.port").to_string(),
                Input::new(&self.ssh_port).w(tokens::form::FIELD_WIDTH_SHORT.rems()),
            )
            .into_any_element(),
        );
        rows.push(
            FormRow::bare(self.check(
                "verify-host-key",
                t!("conn.verify_host_key").to_string(),
                s.verify_host_key,
                |t, v| t.draft.ssh.verify_host_key = v,
                cx,
            ))
            .into_any_element(),
        );
        rows.push(
            FormRow::new(t!("conn.user").to_string(), Input::new(&self.ssh_user).w(w))
                .into_any_element(),
        );
        rows.push(
            FormRow::new(
                t!("conn.auth_method").to_string(),
                Select::new(&self.ssh_auth).w(tokens::form::POPUP_WIDTH_SHORT.rems()),
            )
            .into_any_element(),
        );
        if auth.uses_password() {
            rows.push(
                FormRow::new(
                    t!("conn.password").to_string(),
                    Input::new(&self.ssh_password).w(w),
                )
                .into_any_element(),
            );
            rows.push(
                FormRow::bare(self.check(
                    "ssh-save-password",
                    t!("conn.save_password").to_string(),
                    s.save_password,
                    |t, v| t.draft.ssh.save_password = v,
                    cx,
                ))
                .into_any_element(),
            );
        }
        if auth.uses_key() {
            rows.push(
                FormRow::new(
                    t!("conn.private_key").to_string(),
                    self.path_field("ssh-key-browse", &self.ssh_key, false, cx),
                )
                .into_any_element(),
            );
            rows.push(
                FormRow::new(
                    t!("conn.passphrase").to_string(),
                    Input::new(&self.ssh_passphrase).w(w),
                )
                .into_any_element(),
            );
            rows.push(
                FormRow::bare(self.check(
                    "ssh-save-passphrase",
                    t!("conn.save_passphrase").to_string(),
                    s.save_passphrase,
                    |t, v| t.draft.ssh.save_passphrase = v,
                    cx,
                ))
                .into_any_element(),
            );
        }
        rows.push(
            FormRow::bare(form_note(t!("conn.password_note").to_string(), cx)).into_any_element(),
        );
        rows
    }
}
