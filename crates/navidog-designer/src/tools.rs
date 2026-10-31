//! Users, privileges, maintenance, server monitor, and find. Statement text comes from
//! `navidog-sql`; this panel edits the draft and shows what the server returns.

use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dock::{
    BasePanel, Panel, PanelEvent, PanelId, PanelInfo, PanelState, TabGroup, panel_handle,
    register_panel,
};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{
    Disableable as _, Selectable as _, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement,
    KeyBinding, Render, SharedString, Task, WeakEntity, Window, div, prelude::*,
};
use navidog_db::{DbError, DbResult, ResultSet};
use navidog_model::object::{ObjectKind, Scope};
use navidog_session::sessions::Sessions;
use navidog_sql::dialect::Dialect;
use navidog_sql::security::{
    self, AccountDraft, AccountForm, FindRequest, GrantDraft, MatchMode, Probe,
};
use navidog_ui::assets::lucide;
use navidog_ui::skin::FormRow;
use navidog_ui::{palette, tabs, tokens};
use rust_i18n::t;
use serde::{Deserialize, Serialize};

use crate::panel::{self, Save};

const PANEL: &str = "ToolPanel";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Tool {
    Users,
    Privileges,
    Monitor,
    Find,
    Maintain,
}

impl Tool {
    fn parse(key: &str) -> Option<Self> {
        match key {
            "users" => Some(Self::Users),
            "privileges" => Some(Self::Privileges),
            "monitor" => Some(Self::Monitor),
            "find" => Some(Self::Find),
            "maintain" => Some(Self::Maintain),
            _ => None,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Users => "users",
            Self::Privileges => "privileges",
            Self::Monitor => "monitor",
            Self::Find => "find",
            Self::Maintain => "maintain",
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Saved {
    scope: Scope,
    tool: String,
    target: Option<String>,
}

pub struct ToolPanel {
    scope: Scope,
    tool: Tool,
    target: Option<String>,
    dialect: Arc<dyn Dialect>,
    focus: FocusHandle,
    busy: bool,
    close_approved: bool,
    error: Option<String>,
    output: String,
    preview: Vec<String>,
    name: Entity<InputState>,
    host: Entity<InputState>,
    password: Entity<InputState>,
    role: bool,
    locked: bool,
    login: bool,
    superuser: bool,
    createdb: bool,
    createrole: bool,
    inherit: bool,
    replication: bool,
    expire: Entity<InputState>,
    max_queries: Entity<InputState>,
    max_updates: Entity<InputState>,
    max_connections: Entity<InputState>,
    max_user_connections: Entity<InputState>,
    connection_limit: Entity<InputState>,
    valid_until: Entity<InputState>,
    privilege: Entity<InputState>,
    object_name: Entity<InputState>,
    grantee: Entity<InputState>,
    grant_option: bool,
    auto: bool,
    interval: Entity<InputState>,
    kill_id: Entity<InputState>,
    var_name: Entity<InputState>,
    var_value: Entity<InputState>,
    needle: Entity<InputState>,
    mode: MatchMode,
    case_sensitive: bool,
    search_data: bool,
    kinds: Vec<(ObjectKind, bool)>,
    task: Option<Task<()>>,
    watch: Option<Task<()>>,
}

impl ToolPanel {
    pub fn open(
        scope: Scope,
        tool: &str,
        target: Option<String>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Entity<Self>> {
        let tool = Tool::parse(tool)?;
        let dialect = Sessions::global(cx).read(cx).dialect(&scope.connection)?;
        Some(cx.new(|cx| Self::new(scope, tool, target, dialect, window, cx)))
    }

    fn new(
        scope: Scope,
        tool: Tool,
        target: Option<String>,
        dialect: Arc<dyn Dialect>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut input = |value: &str, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).default_value(value))
        };
        let kinds = dialect
            .object_kinds()
            .iter()
            .copied()
            .filter(|kind| *kind != ObjectKind::Query)
            .map(|kind| (kind, true))
            .collect();
        Self {
            scope,
            tool,
            target,
            dialect,
            focus: cx.focus_handle(),
            busy: false,
            close_approved: false,
            error: None,
            output: String::new(),
            preview: Vec::new(),
            name: input("", cx),
            host: input("%", cx),
            password: input("", cx),
            role: false,
            locked: false,
            login: true,
            superuser: false,
            createdb: false,
            createrole: false,
            inherit: true,
            replication: false,
            expire: input("", cx),
            max_queries: input("", cx),
            max_updates: input("", cx),
            max_connections: input("", cx),
            max_user_connections: input("", cx),
            connection_limit: input("", cx),
            valid_until: input("", cx),
            privilege: input("SELECT", cx),
            object_name: input("", cx),
            grantee: input("", cx),
            grant_option: false,
            auto: false,
            interval: input("5", cx),
            kill_id: input("", cx),
            var_name: input("", cx),
            var_value: input("", cx),
            needle: input("", cx),
            mode: MatchMode::Contains,
            case_sensitive: false,
            search_data: false,
            kinds,
            task: None,
            watch: None,
        }
    }

    fn text(input: &Entity<InputState>, cx: &App) -> String {
        input.read(cx).value().to_string()
    }

    fn account_draft(&self, cx: &App) -> AccountDraft {
        AccountDraft {
            name: Self::text(&self.name, cx),
            host: Self::text(&self.host, cx),
            password: Self::text(&self.password, cx),
            role: self.role,
            locked: self.locked,
            expire: Self::text(&self.expire, cx),
            max_queries: Self::text(&self.max_queries, cx),
            max_updates: Self::text(&self.max_updates, cx),
            max_connections: Self::text(&self.max_connections, cx),
            max_user_connections: Self::text(&self.max_user_connections, cx),
            login: self.login,
            superuser: self.superuser,
            createdb: self.createdb,
            createrole: self.createrole,
            inherit: self.inherit,
            replication: self.replication,
            connection_limit: Self::text(&self.connection_limit, cx),
            valid_until: Self::text(&self.valid_until, cx),
            ..AccountDraft::default()
        }
    }

    fn show_account(&mut self, cx: &mut Context<Self>) {
        if matches!(self.dialect.account_form(), AccountForm::Unavailable(_)) {
            self.preview.clear();
            self.error = Some(security::SQLITE_AUTH_REASON.into());
            return;
        }
        match security::account_statements(
            self.dialect.as_ref(),
            self.dialect.definition_family(),
            &self.account_draft(cx),
        ) {
            Ok(sql) => {
                self.output = sql.join(";\n");
                self.preview = sql;
                self.error = None;
            }
            Err(error) => {
                self.preview.clear();
                self.error = Some(error);
            }
        }
        cx.notify();
    }

    fn show_grant(&mut self, revoke: bool, cx: &mut Context<Self>) {
        if !self.dialect.manages_privileges() {
            self.preview.clear();
            self.error = Some(t!("designer.tool_no_privileges").to_string());
            return;
        }
        let draft = GrantDraft {
            privilege: Self::text(&self.privilege, cx),
            object: Self::text(&self.object_name, cx),
            grantee: Self::text(&self.grantee, cx),
            host: Self::text(&self.host, cx),
            grant_option: self.grant_option,
            revoke,
        };
        match security::grant_statements(
            self.dialect.as_ref(),
            self.dialect.definition_family(),
            &draft,
        ) {
            Ok(sql) => {
                self.output = sql.join(";\n");
                self.preview = sql;
                self.error = None;
            }
            Err(error) => {
                self.preview.clear();
                self.error = Some(error);
            }
        }
        cx.notify();
    }

    fn execute_preview(&mut self, cx: &mut Context<Self>) {
        if self.preview.is_empty() || self.busy {
            return;
        }
        let sql = self.preview.clone();
        self.run(sql, false, cx);
    }

    fn run(&mut self, sqls: Vec<String>, read: bool, cx: &mut Context<Self>) {
        if sqls.is_empty() {
            return;
        }
        let database = if self.scope.database.is_empty() {
            None
        } else {
            Some(self.scope.database.clone())
        };
        let Some(job) = Sessions::global(cx)
            .read(cx)
            .new_session(&self.scope.connection, database.as_deref())
        else {
            self.error = Some(t!("designer.tool_not_connected").to_string());
            return;
        };
        self.busy = true;
        self.error = None;
        self.task = Some(cx.spawn(async move |this, cx| {
            let result: DbResult<String> = async {
                let mut session = job.await.map_err(|e| DbError::msg(e.to_string()))??;
                let job = navidog_db::runtime::spawn(async move {
                    let result = async {
                        let r = session.relational().ok_or_else(|| {
                            DbError::msg("This connection is not a relational database.")
                        })?;
                        if let Some(database) = database.as_deref()
                            && let Some(sql) = r.dialect().prepare_database_sql(database)
                        {
                            r.execute(&sql).await?;
                        }
                        let mut out = String::new();
                        for sql in &sqls {
                            let sets = if read {
                                r.execute_read(sql).await?
                            } else {
                                r.execute(sql).await?
                            };
                            out.push_str(&format_sets(sets));
                        }
                        Ok(out)
                    }
                    .await;
                    let _ = session.close().await;
                    result
                });
                job.await.map_err(|e| DbError::msg(e.to_string()))?
            }
            .await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(text) => {
                        this.output = text;
                        this.preview.clear();
                        this.error = None;
                    }
                    Err(error) => this.error = Some(error.to_string()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn list_accounts(&mut self, cx: &mut Context<Self>) {
        let Some(sql) = self.dialect.account_list_sql() else {
            self.error = Some(security::SQLITE_AUTH_REASON.into());
            cx.notify();
            return;
        };
        self.run(vec![sql.into()], true, cx);
    }

    fn show_members(&mut self, cx: &mut Context<Self>) {
        let name = Self::text(&self.name, cx);
        if name.trim().is_empty() {
            self.error = Some(t!("designer.tool_need_name").to_string());
            cx.notify();
            return;
        }
        let object = navidog_model::object::ObjectRef {
            scope: self.scope.clone(),
            kind: ObjectKind::Table,
            name,
        };
        let plan = security::relation_plan(
            self.dialect.as_ref(),
            self.dialect.definition_family(),
            &object,
            true,
        );
        if let Some(note) = plan.note {
            self.output = note.to_string();
            self.error = None;
            cx.notify();
            return;
        }
        self.run(
            plan.queries.into_iter().map(|query| query.sql).collect(),
            true,
            cx,
        );
    }

    fn load_monitor(&mut self, sql: Option<&str>, cx: &mut Context<Self>) {
        let Some(sql) = sql else {
            self.error = Some(t!("designer.tool_no_monitor").to_string());
            cx.notify();
            return;
        };
        self.run(vec![sql.into()], true, cx);
    }

    fn kill(&mut self, cx: &mut Context<Self>) {
        match self.dialect.kill_sql(&Self::text(&self.kill_id, cx)) {
            Ok(Some(sql)) => self.run(vec![sql], false, cx),
            Ok(None) => {
                self.error = Some(t!("designer.tool_no_monitor").to_string());
                cx.notify();
            }
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    fn set_variable(&mut self, cx: &mut Context<Self>) {
        match security::set_variable_sql(
            self.dialect.as_ref(),
            self.dialect.definition_family(),
            &Self::text(&self.var_name, cx),
            &Self::text(&self.var_value, cx),
        ) {
            Ok(sql) if !sql.is_empty() => self.run(sql, false, cx),
            Ok(_) => {
                self.error = Some(t!("designer.tool_no_monitor").to_string());
                cx.notify();
            }
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    fn toggle_auto(&mut self, on: bool, cx: &mut Context<Self>) {
        self.auto = on;
        self.watch.take();
        if !on || !self.dialect.monitors_server() {
            return;
        }
        let secs = Self::text(&self.interval, cx)
            .trim()
            .parse::<u64>()
            .unwrap_or(5)
            .clamp(1, 3600);
        self.watch = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(secs))
                    .await;
                let go = this
                    .update(cx, |this, cx| {
                        if !this.auto {
                            return false;
                        }
                        if let Some(sql) = this.dialect.process_sql() {
                            this.run(vec![sql.into()], true, cx);
                        }
                        true
                    })
                    .unwrap_or(false);
                if !go {
                    break;
                }
            }
        }));
    }

    fn find(&mut self, cx: &mut Context<Self>) {
        let kinds: Vec<ObjectKind> = self
            .kinds
            .iter()
            .filter(|(_, on)| *on)
            .map(|(kind, _)| *kind)
            .collect();
        let needle = Self::text(&self.needle, cx);
        let plan = match security::find_plan(
            self.dialect.as_ref(),
            self.dialect.definition_family(),
            &self.scope,
            FindRequest {
                needle: &needle,
                mode: self.mode,
                case_sensitive: self.case_sensitive,
                kinds: &kinds,
                data: self.search_data,
            },
        ) {
            Ok(plan) => plan,
            Err(error) => {
                self.error = Some(error);
                cx.notify();
                return;
            }
        };
        let database = if self.scope.database.is_empty() {
            None
        } else {
            Some(self.scope.database.clone())
        };
        let Some(job) = Sessions::global(cx)
            .read(cx)
            .new_session(&self.scope.connection, database.as_deref())
        else {
            self.error = Some(t!("designer.tool_not_connected").to_string());
            cx.notify();
            return;
        };
        self.busy = true;
        self.error = None;
        let scope = self.scope.clone();
        self.task = Some(cx.spawn(async move |this, cx| {
            let result: DbResult<String> = async {
                let mut session = job.await.map_err(|e| DbError::msg(e.to_string()))??;
                let job = navidog_db::runtime::spawn(async move {
                    let result = async {
                        let r = session.relational().ok_or_else(|| {
                            DbError::msg("This connection is not a relational database.")
                        })?;
                        if let Some(database) = database.as_deref()
                            && let Some(sql) = r.dialect().prepare_database_sql(database)
                        {
                            r.execute(&sql).await?;
                        }
                        let mut out = String::new();
                        for sql in &plan.structure {
                            out.push_str(&format_sets(r.execute_read(sql).await?));
                        }
                        if let Some(columns_sql) = &plan.text_columns {
                            let sets = r.execute_read(columns_sql).await?;
                            let mut by_table: BTreeMap<String, Vec<String>> = BTreeMap::new();
                            for set in sets {
                                for row in set.rows {
                                    let table = row.first().cloned().flatten().unwrap_or_default();
                                    let column = row.get(1).cloned().flatten().unwrap_or_default();
                                    if !table.is_empty() && !column.is_empty() {
                                        by_table.entry(table).or_default().push(column);
                                    }
                                }
                            }
                            let dialect = r.dialect();
                            for (table, columns) in by_table {
                                match security::data_probe(
                                    dialect.as_ref(),
                                    dialect.definition_family(),
                                    &scope,
                                    Probe {
                                        table: &table,
                                        columns: &columns,
                                        needle: &plan.needle,
                                        mode: plan.mode,
                                        case_sensitive: plan.case_sensitive,
                                    },
                                ) {
                                    Ok(sql) => {
                                        out.push_str(&table);
                                        out.push('\n');
                                        match r.execute_read(&sql).await {
                                            Ok(sets) => out.push_str(&format_sets(sets)),
                                            Err(error) => {
                                                out.push_str(&error.to_string());
                                                out.push('\n');
                                            }
                                        }
                                    }
                                    Err(error) => {
                                        out.push_str(&error);
                                        out.push('\n');
                                    }
                                }
                            }
                        }
                        Ok(out)
                    }
                    .await;
                    let _ = session.close().await;
                    result
                });
                job.await.map_err(|e| DbError::msg(e.to_string()))?
            }
            .await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(text) => {
                        this.output = if text.trim().is_empty() {
                            t!("designer.tool_no_hits").to_string()
                        } else {
                            text
                        };
                        this.error = None;
                    }
                    Err(error) => this.error = Some(error.to_string()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn maintain(&mut self, sql: String, cx: &mut Context<Self>) {
        self.output = sql.clone();
        self.preview = vec![sql];
        cx.notify();
    }

    fn dirty(&self) -> bool {
        !self.preview.is_empty()
    }

    fn ask_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || !self.dirty() {
            return;
        }
        let weak = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let weak = weak.clone();
            dialog
                .title(t!("designer.tool_discard").to_string())
                .footer(
                    h_flex().justify_end().gap_2().child(
                        Button::new("tool-discard")
                            .label(t!("designer.cancel").to_string())
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                if let Ok(id) = weak.update(cx, |this, cx| {
                                    this.close_approved = true;
                                    PanelId::from(cx.entity_id())
                                }) {
                                    window.dispatch_action(
                                        Box::new(tabs::CloseTabs {
                                            panel: Some(id),
                                            which: tabs::Close::This,
                                        }),
                                        cx,
                                    );
                                }
                            }),
                    ),
                )
        });
    }
}

impl EventEmitter<PanelEvent> for ToolPanel {}

impl Focusable for ToolPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl BasePanel for ToolPanel {
    fn panel_name(&self) -> &'static str {
        PANEL
    }
    fn closable(&self, _: &App) -> bool {
        !self.busy && (self.close_approved || !self.dirty())
    }
    fn dump(&self, _: &App) -> PanelState {
        PanelState {
            panel_name: PANEL.into(),
            children: Vec::new(),
            info: PanelInfo::panel(
                serde_json::to_value(Saved {
                    scope: self.scope.clone(),
                    tool: self.tool.key().into(),
                    target: self.target.clone(),
                })
                .unwrap_or_default(),
            ),
        }
    }
    fn on_added_to(
        &mut self,
        group: WeakEntity<TabGroup>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = PanelId::from(cx.entity_id());
        tabs::added(id, group, cx);
        let weak = cx.entity().downgrade();
        let home = window.window_handle();
        tabs::set_close_guard(
            id,
            Rc::new(move |cx| {
                let weak = weak.clone();
                let _ = home.update(cx, |_, window, cx| {
                    let _ = weak.update(cx, |this, cx| this.ask_close(window, cx));
                });
            }),
            cx,
        );
    }
    fn set_active(&mut self, active: bool, _: &mut Window, cx: &mut Context<Self>) {
        tabs::set_active(PanelId::from(cx.entity_id()), active, cx);
    }
    fn on_removed(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.auto = false;
        self.watch.take();
        self.task.take();
        if tabs::removed(PanelId::from(cx.entity_id()), cx) {
            self.close_approved = true;
        }
    }
}

impl Panel for ToolPanel {
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tabs::title(
            PanelId::from(cx.entity_id()),
            Some(lucide(match self.tool {
                Tool::Users => "user",
                Tool::Privileges => "key",
                Tool::Monitor => "server",
                Tool::Find => "search",
                Tool::Maintain => "list",
            })),
            t!(format!("designer.tool_{}", self.tool.key())).to_string(),
            None,
            self.dirty(),
        )
    }
}

impl Render for ToolPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let disabled = self.busy;
        let body = match self.tool {
            Tool::Users => self.render_users(disabled, cx),
            Tool::Privileges => self.render_privileges(disabled, cx),
            Tool::Monitor => self.render_monitor(disabled, cx),
            Tool::Find => self.render_find(disabled, cx),
            Tool::Maintain => self.render_maintain(disabled, cx),
        };
        v_flex()
            .id("tool-panel")
            .key_context(PANEL)
            .track_focus(&self.focus)
            .size_full()
            .bg(p.window_bg)
            .on_action(cx.listener(|this, _: &Save, _, cx| match this.tool {
                Tool::Users => this.show_account(cx),
                Tool::Privileges => this.show_grant(false, cx),
                _ => {}
            }))
            .child(
                h_flex()
                    .min_h(tokens::object_toolbar::HEIGHT.rems())
                    .px_2()
                    .gap_1()
                    .border_b_1()
                    .border_color(p.separator)
                    .when(!self.preview.is_empty(), |bar| {
                        bar.child(
                            Button::new("tool-execute")
                                .small()
                                .icon(lucide("play"))
                                .label(t!("designer.execute").to_string())
                                .disabled(disabled)
                                .on_click(cx.listener(|this, _, _, cx| this.execute_preview(cx))),
                        )
                    }),
            )
            .when_some(self.error.clone(), |col, error| {
                col.child(div().px_2().text_color(p.danger).child(error))
            })
            .child(body)
            .child(self.render_output())
    }
}

impl ToolPanel {
    fn render_output(&self) -> gpui_kit::AnyElement {
        v_flex()
            .id("tool-output")
            .flex_1()
            .min_h_0()
            .p_2()
            .gap_1()
            .overflow_y_scroll()
            .children(
                self.output
                    .lines()
                    .map(|line| div().child(line.to_string())),
            )
            .into_any_element()
    }

    fn render_users(&self, disabled: bool, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        if let AccountForm::Unavailable(reason) = self.dialect.account_form() {
            return v_flex()
                .id("tool-users")
                .p_3()
                .child(reason.to_string())
                .into_any_element();
        }
        let mysql = self.dialect.account_form() == AccountForm::Mysql;
        v_flex()
            .id("tool-users")
            .p_3()
            .gap_2()
            .overflow_y_scroll()
            .max_h(tokens::designer::PROPERTY_MIN_HEIGHT.rems())
            .child(FormRow::new(
                t!("designer.name").to_string(),
                Input::new(&self.name)
                    .id("tool-name")
                    .small()
                    .disabled(disabled),
            ))
            .when(mysql, |col| {
                col.child(FormRow::new(
                    t!("designer.tool_host").to_string(),
                    Input::new(&self.host)
                        .id("tool-host")
                        .small()
                        .disabled(disabled),
                ))
            })
            .child(FormRow::new(
                t!("designer.tool_password").to_string(),
                Input::new(&self.password)
                    .id("tool-password")
                    .small()
                    .disabled(disabled),
            ))
            .when(mysql, |col| {
                col.child(check(
                    "tool-role",
                    "tool_role",
                    self.role,
                    disabled,
                    cx,
                    |this, on| {
                        this.role = on;
                    },
                ))
                .child(check(
                    "tool-locked",
                    "tool_locked",
                    self.locked,
                    disabled,
                    cx,
                    |this, on| {
                        this.locked = on;
                    },
                ))
                .child(FormRow::new(
                    t!("designer.tool_expire").to_string(),
                    Input::new(&self.expire)
                        .id("tool-expire")
                        .small()
                        .disabled(disabled),
                ))
                .child(FormRow::new(
                    t!("designer.tool_max_queries").to_string(),
                    Input::new(&self.max_queries)
                        .id("tool-max-queries")
                        .small()
                        .disabled(disabled),
                ))
                .child(FormRow::new(
                    t!("designer.tool_max_updates").to_string(),
                    Input::new(&self.max_updates)
                        .id("tool-max-updates")
                        .small()
                        .disabled(disabled),
                ))
                .child(FormRow::new(
                    t!("designer.tool_max_connections").to_string(),
                    Input::new(&self.max_connections)
                        .id("tool-max-connections")
                        .small()
                        .disabled(disabled),
                ))
                .child(FormRow::new(
                    t!("designer.tool_max_user_connections").to_string(),
                    Input::new(&self.max_user_connections)
                        .id("tool-max-user-connections")
                        .small()
                        .disabled(disabled),
                ))
            })
            .when(!mysql, |col| {
                col.child(check(
                    "tool-login",
                    "tool_login",
                    self.login,
                    disabled,
                    cx,
                    |this, on| {
                        this.login = on;
                    },
                ))
                .child(check(
                    "tool-superuser",
                    "tool_superuser",
                    self.superuser,
                    disabled,
                    cx,
                    |this, on| this.superuser = on,
                ))
                .child(check(
                    "tool-createdb",
                    "tool_createdb",
                    self.createdb,
                    disabled,
                    cx,
                    |this, on| this.createdb = on,
                ))
                .child(check(
                    "tool-createrole",
                    "tool_createrole",
                    self.createrole,
                    disabled,
                    cx,
                    |this, on| this.createrole = on,
                ))
                .child(check(
                    "tool-inherit",
                    "tool_inherit",
                    self.inherit,
                    disabled,
                    cx,
                    |this, on| this.inherit = on,
                ))
                .child(check(
                    "tool-replication",
                    "tool_replication",
                    self.replication,
                    disabled,
                    cx,
                    |this, on| this.replication = on,
                ))
                .child(FormRow::new(
                    t!("designer.tool_connection_limit").to_string(),
                    Input::new(&self.connection_limit)
                        .id("tool-connection-limit")
                        .small()
                        .disabled(disabled),
                ))
                .child(FormRow::new(
                    t!("designer.tool_valid_until").to_string(),
                    Input::new(&self.valid_until)
                        .id("tool-valid-until")
                        .small()
                        .disabled(disabled),
                ))
            })
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Button::new("tool-preview-user")
                            .small()
                            .label(t!("designer.tool_preview").to_string())
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, _, cx| this.show_account(cx))),
                    )
                    .child(
                        Button::new("tool-list-users")
                            .small()
                            .label(t!("designer.tool_list").to_string())
                            .disabled(disabled || self.dialect.account_list_sql().is_none())
                            .on_click(cx.listener(|this, _, _, cx| this.list_accounts(cx))),
                    )
                    .child(
                        Button::new("tool-members")
                            .small()
                            .label(t!("designer.tool_members").to_string())
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, _, cx| this.show_members(cx))),
                    ),
            )
            .into_any_element()
    }

    fn render_privileges(&self, disabled: bool, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        if !self.dialect.manages_privileges() {
            return v_flex()
                .id("tool-privileges")
                .p_3()
                .child(t!("designer.tool_no_privileges").to_string())
                .into_any_element();
        }
        let mysql = self.dialect.account_form() == AccountForm::Mysql;
        v_flex()
            .id("tool-privileges")
            .p_3()
            .gap_2()
            .child(FormRow::new(
                t!("designer.tool_privilege").to_string(),
                Input::new(&self.privilege)
                    .id("tool-privilege")
                    .small()
                    .disabled(disabled),
            ))
            .child(FormRow::new(
                t!("designer.tool_object").to_string(),
                Input::new(&self.object_name)
                    .id("tool-object")
                    .small()
                    .disabled(disabled),
            ))
            .child(FormRow::new(
                t!("designer.tool_grantee").to_string(),
                Input::new(&self.grantee)
                    .id("tool-grantee")
                    .small()
                    .disabled(disabled),
            ))
            .when(mysql, |col| {
                col.child(FormRow::new(
                    t!("designer.tool_host").to_string(),
                    Input::new(&self.host)
                        .id("tool-grant-host")
                        .small()
                        .disabled(disabled),
                ))
            })
            .child(check(
                "tool-grant-option",
                "tool_grant_option",
                self.grant_option,
                disabled,
                cx,
                |this, on| this.grant_option = on,
            ))
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Button::new("tool-grant")
                            .small()
                            .label(t!("designer.tool_grant").to_string())
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, _, cx| this.show_grant(false, cx))),
                    )
                    .child(
                        Button::new("tool-revoke")
                            .small()
                            .label(t!("designer.tool_revoke").to_string())
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, _, cx| this.show_grant(true, cx))),
                    ),
            )
            .into_any_element()
    }

    fn render_monitor(&self, disabled: bool, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        if !self.dialect.monitors_server() {
            return v_flex()
                .id("tool-monitor")
                .p_3()
                .child(t!("designer.tool_no_monitor").to_string())
                .into_any_element();
        }
        v_flex()
            .id("tool-monitor")
            .p_3()
            .gap_2()
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Button::new("tool-processes")
                            .small()
                            .label(t!("designer.tool_processes").to_string())
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, _, cx| {
                                let sql = this.dialect.process_sql();
                                this.load_monitor(sql, cx);
                            })),
                    )
                    .child(
                        Button::new("tool-variables")
                            .small()
                            .label(t!("designer.tool_variables").to_string())
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, _, cx| {
                                let sql = this.dialect.variables_sql();
                                this.load_monitor(sql, cx);
                            })),
                    )
                    .child(
                        Button::new("tool-status")
                            .small()
                            .label(t!("designer.tool_status").to_string())
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, _, cx| {
                                let sql = this.dialect.status_sql();
                                this.load_monitor(sql, cx);
                            })),
                    ),
            )
            .child(
                Checkbox::new("tool-auto")
                    .label(t!("designer.tool_auto").to_string())
                    .checked(self.auto)
                    .disabled(disabled)
                    .on_click(cx.listener(|this, value, _, cx| this.toggle_auto(*value, cx))),
            )
            .child(FormRow::new(
                t!("designer.tool_interval").to_string(),
                Input::new(&self.interval)
                    .id("tool-interval")
                    .small()
                    .disabled(disabled),
            ))
            .child(FormRow::new(
                t!("designer.tool_process_id").to_string(),
                Input::new(&self.kill_id)
                    .id("tool-kill-id")
                    .small()
                    .disabled(disabled),
            ))
            .child(
                Button::new("tool-kill")
                    .small()
                    .label(t!("designer.tool_kill").to_string())
                    .disabled(disabled)
                    .on_click(cx.listener(|this, _, _, cx| this.kill(cx))),
            )
            .child(FormRow::new(
                t!("designer.tool_variable").to_string(),
                Input::new(&self.var_name)
                    .id("tool-var-name")
                    .small()
                    .disabled(disabled),
            ))
            .child(FormRow::new(
                t!("designer.tool_value").to_string(),
                Input::new(&self.var_value)
                    .id("tool-var-value")
                    .small()
                    .disabled(disabled),
            ))
            .child(
                Button::new("tool-set-var")
                    .small()
                    .label(t!("designer.tool_set").to_string())
                    .disabled(disabled)
                    .on_click(cx.listener(|this, _, _, cx| this.set_variable(cx))),
            )
            .into_any_element()
    }

    fn render_find(&self, disabled: bool, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        v_flex()
            .id("tool-find")
            .p_3()
            .gap_2()
            .child(FormRow::new(
                t!("designer.tool_find_text").to_string(),
                Input::new(&self.needle)
                    .id("tool-needle")
                    .small()
                    .disabled(disabled),
            ))
            .child(
                h_flex()
                    .gap_1()
                    .child(mode_button("contains", MatchMode::Contains, self.mode, cx))
                    .child(mode_button("word", MatchMode::WholeWord, self.mode, cx))
                    .child(mode_button("prefix", MatchMode::Prefix, self.mode, cx))
                    .child(mode_button("regex", MatchMode::Regex, self.mode, cx)),
            )
            .child(check(
                "tool-case",
                "tool_case",
                self.case_sensitive,
                disabled,
                cx,
                |this, on| this.case_sensitive = on,
            ))
            .child(check(
                "tool-data",
                "tool_data",
                self.search_data,
                disabled,
                cx,
                |this, on| this.search_data = on,
            ))
            .child(h_flex().gap_1().children(self.kinds.iter().enumerate().map(
                |(ix, (kind, on))| {
                    let kind_key = kind.key();
                    Checkbox::new(SharedString::from(format!("tool-kind-{kind_key}")))
                        .label(t!(format!("designer.kind_{kind_key}")).to_string())
                        .checked(*on)
                        .disabled(disabled)
                        .on_click(cx.listener(move |this, value, _, cx| {
                            if let Some(item) = this.kinds.get_mut(ix) {
                                item.1 = *value;
                            }
                            cx.notify();
                        }))
                },
            )))
            .child(
                Button::new("tool-find-run")
                    .small()
                    .icon(lucide("search"))
                    .label(t!("designer.tool_find").to_string())
                    .disabled(disabled)
                    .on_click(cx.listener(|this, _, _, cx| this.find(cx))),
            )
            .into_any_element()
    }

    fn render_maintain(&self, disabled: bool, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let jobs = security::maintenance(
            self.dialect.as_ref(),
            self.dialect.definition_family(),
            &self.scope,
            self.target.as_deref(),
        );
        v_flex()
            .id("tool-maintain")
            .p_3()
            .gap_2()
            .when(jobs.is_empty(), |col| {
                col.child(t!("designer.tool_no_maintenance").to_string())
            })
            .children(jobs.into_iter().map(|job| {
                let sql = job.sql.clone();
                let label = job_label(job.key);
                h_flex().gap_1().child(
                    Button::new(SharedString::from(format!("tool-job-{}", job.key)))
                        .small()
                        .label(label)
                        .disabled(disabled)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.maintain(sql.clone(), cx);
                        })),
                )
            }))
            .into_any_element()
    }
}

fn job_label(key: &str) -> String {
    match key {
        "analyze" => t!("designer.tool_analyze").to_string(),
        "check" => t!("designer.tool_check").to_string(),
        "optimize" => t!("designer.tool_optimize").to_string(),
        "repair" => t!("designer.tool_repair").to_string(),
        "vacuum" => t!("designer.tool_vacuum").to_string(),
        "reindex" => t!("designer.tool_reindex").to_string(),
        "integrity" => t!("designer.tool_integrity").to_string(),
        _ => key.to_string(),
    }
}

fn mode_button(
    key: &'static str,
    mode: MatchMode,
    current: MatchMode,
    cx: &mut Context<ToolPanel>,
) -> Button {
    Button::new(SharedString::from(format!("tool-mode-{key}")))
        .small()
        .selected(current == mode)
        .label(t!(format!("designer.tool_mode_{key}")).to_string())
        .on_click(cx.listener(move |this, _, _, cx| {
            this.mode = mode;
            cx.notify();
        }))
}

fn check(
    id: &'static str,
    label: &'static str,
    on: bool,
    disabled: bool,
    cx: &mut Context<ToolPanel>,
    set: impl Fn(&mut ToolPanel, bool) + 'static,
) -> gpui_kit::AnyElement {
    Checkbox::new(id)
        .label(t!(format!("designer.{label}")).to_string())
        .checked(on)
        .disabled(disabled)
        .on_click(cx.listener(move |this, value, _, cx| {
            set(this, *value);
            cx.notify();
        }))
        .into_any_element()
}

fn format_sets(sets: Vec<ResultSet>) -> String {
    let mut out = String::new();
    if sets.is_empty() {
        out.push_str("(ok)\n");
        return out;
    }
    for set in sets {
        if set.rows.is_empty() {
            if let Some(rows) = set.affected_rows {
                out.push_str(&format!("({rows})\n"));
            }
            continue;
        }
        for row in set.rows {
            for (i, cell) in row.iter().enumerate() {
                if i > 0 {
                    out.push('\t');
                }
                out.push_str(cell.as_deref().unwrap_or("NULL"));
            }
            out.push('\n');
        }
    }
    out
}

pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-s", Save, Some(PANEL)),
        KeyBinding::new("cmd-s", Save, Some("ToolPanel > Input")),
    ]);
    register_panel(cx, PANEL, |ctx, window, cx| {
        let saved = match ctx.info() {
            PanelInfo::Panel(value) => serde_json::from_value::<Saved>(value.clone()).ok(),
            _ => None,
        };
        let Some(saved) = saved else {
            return panel_handle(panel::MissingConnection::open(ctx.state().clone(), cx));
        };
        match ToolPanel::open(saved.scope, &saved.tool, saved.target, window, cx) {
            Some(panel) => panel_handle(panel),
            None => panel_handle(panel::MissingConnection::open(ctx.state().clone(), cx)),
        }
    });
}
