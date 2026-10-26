//! Results area of a query editor (31-query-results.md §4): result and explain tabs, messages,
//! summary, pinning, and editing single-table results.

mod editing;
mod page;
mod render;
mod tabs;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashSet};
use std::ops::Range;
use std::time::Duration;

use gpui_kit::component::input::{EditorState, InputEvent, Textarea, TextareaState};
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, EventEmitter, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, WeakEntity, Window, div,
};
use navidog_db::dialect::Dialect;
use navidog_db::{Column, ResultSet};
use navidog_grid::{GridColumn, ResultGrid};
use navidog_model::object::Scope;
use navidog_ui::{palette, tokens};
use rust_i18n::t;

use crate::editor::QueryEditor;
use crate::explain::ExplainView;

/// At most this many result sets get a tab per run; the rest only reach the messages (31 §4.1).
pub(crate) const MAX_RESULTS: usize = 20;

/// When and where a result came from, shown on hover over a pinned tab and on explain tabs.
#[derive(Clone, Debug)]
pub(crate) struct RunInfo {
    pub at: chrono::DateTime<chrono::Local>,
    pub query_time: Duration,
    pub connection: String,
    pub connection_id: String,
    pub database: Option<String>,
    pub schema: Option<String>,
    /// Exact PostgreSQL path after a SQL context command, including fallback schemas.
    pub search_path: Option<String>,
    pub context_id: Option<u64>,
    pub context_known: bool,
    pub dialect: Option<Dialect>,
}

impl RunInfo {
    /// Where a result column's table lives: MySQL and SQLite name the database in the origin,
    /// PostgreSQL the schema of the database this editor runs in.
    pub fn scope_for(&self, origin_schema: &str) -> Option<Scope> {
        let conn = self.connection_id.clone();
        Some(match self.dialect? {
            Dialect::Postgres => Scope::new(
                conn,
                self.database.clone()?,
                Some(origin_schema.to_string()),
            ),
            Dialect::MySql | Dialect::Sqlite => Scope::new(conn, origin_schema, None),
        })
    }

    pub fn lines(&self, sql: &str) -> Vec<(String, String)> {
        let mut out = vec![
            (
                t!("query.info_time").to_string(),
                self.at.format("%Y-%m-%d %H:%M:%S").to_string(),
            ),
            (
                t!("query.info_query_time").to_string(),
                secs(self.query_time),
            ),
            (
                t!("query.info_connection").to_string(),
                self.connection.clone(),
            ),
        ];
        if let Some(db) = &self.database {
            out.push((t!("query.database").to_string(), db.clone()));
        }
        if let Some(s) = &self.schema {
            out.push((t!("query.schema").to_string(), s.clone()));
        }
        out.push(("SQL".into(), sql.to_string()));
        out
    }
}

pub(crate) fn secs(d: Duration) -> String {
    format!("{:.3}s", d.as_secs_f64())
}

pub(crate) enum Body {
    Grid(Entity<ResultView>),
    Explain(Entity<ExplainView>),
}

pub(crate) struct Tab {
    pub id: u64,
    pub name: String,
    pub pinned: bool,
    pub body: Body,
    pub info: RunInfo,
    pub sql: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Page {
    Messages,
    Summary,
    Tab(u64),
    Profile,
    Status,
}

pub(crate) struct SummaryRow {
    pub sql: String,
    pub message: String,
    pub elapsed: Duration,
    pub error: bool,
    pub range: Range<usize>,
}

pub(crate) struct Results {
    pub tabs: Vec<Tab>,
    pub page: Page,
    messages: Entity<EditorState>,
    messages_text: String,
    pub rows: Vec<SummaryRow>,
    started: Option<chrono::DateTime<chrono::Local>>,
    ended: Option<chrono::DateTime<chrono::Local>>,
    pub only_errors: bool,
    /// Result and explain tabs made by the current run, for naming and the cap
    results_this_run: usize,
    /// `next_id` at the start of the current run. Tabs above this were created by it.
    run_floor: u64,
    explains_this_run: usize,
    /// MySQL "Show Profile and Status" tabs of the last run
    profile: Option<Entity<ResultGrid>>,
    status: Option<Entity<ResultGrid>>,
    next_id: u64,
    subscriptions: BTreeMap<u64, Subscription>,
}

/// Commands from a result tab's context menu.
#[derive(Clone, PartialEq, Debug, gpui_kit::Action)]
#[action(namespace = query, no_json)]
pub(crate) struct TogglePinTab {
    pub id: u64,
}

#[derive(Clone, PartialEq, Debug, gpui_kit::Action)]
#[action(namespace = query, no_json)]
pub(crate) struct CloseResultTab {
    pub id: u64,
}

gpui_kit::actions!(query, [UnpinAllTabs, CloseAllResults, BackToEditor]);

pub(crate) enum ResultEvent {
    TogglePin,
}

impl Results {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        let messages = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("text")
                .line_number(false)
                .soft_wrap(true)
        });
        messages.update(cx, |e, cx| e.set_readonly(true, cx));
        Self {
            tabs: Vec::new(),
            page: Page::Messages,
            messages,
            messages_text: String::new(),
            rows: Vec::new(),
            started: None,
            ended: None,
            only_errors: false,
            results_this_run: 0,
            run_floor: 0,
            explains_this_run: 0,
            profile: None,
            status: None,
            next_id: 0,
            subscriptions: BTreeMap::new(),
        }
    }
}

/// Where an editable result writes.
struct Editing {
    dialect: Dialect,
    scope: Scope,
    table: String,
    /// Result column name → table column name
    names: BTreeMap<String, String>,
    /// Primary key columns of the table
    pk: HashSet<String>,
}

impl Editing {
    fn is_pk(&self, result_column: &str) -> bool {
        self.names
            .get(result_column)
            .is_some_and(|c| self.pk.contains(c))
    }
}

/// One result tab: the grid, the bottom bar and the edit and commit flow.
pub(crate) struct ResultView {
    pub grid: Entity<ResultGrid>,
    pub sql: String,
    columns: Vec<Column>,
    edit: Option<Editing>,
    info: RunInfo,
    editor: WeakEntity<QueryEditor>,
    pub pinned: bool,
    busy: bool,
    status: SharedString,
    /// The last sort accepted by this result, including DataTable's header indicator.
    accepted_sort: Option<(SharedString, bool)>,
    /// The cell editor under the grid (F-DATA-011 style), and the cell it shows
    cell_editor: bool,
    cell_input: Entity<TextareaState>,
    cell_at: Option<(usize, usize)>,
    /// Form view of the current row instead of the grid
    form: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ResultEvent> for ResultView {}

impl ResultView {
    pub fn new(
        set: ResultSet,
        sql: String,
        info: RunInfo,
        editor: WeakEntity<QueryEditor>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let grid = cx.new(|cx| {
            let mut grid = ResultGrid::new(window, cx);
            grid.set_quick_filter(false);
            let columns = set
                .columns
                .iter()
                .map(|c| GridColumn {
                    name: c.name.clone().into(),
                    type_name: c.type_name.clone().into(),
                    ..GridColumn::default()
                })
                .collect();
            let rows = set
                .rows
                .into_iter()
                .map(|r| r.into_iter().map(|v| v.map(Into::into)).collect())
                .collect();
            grid.set_data(columns, rows, cx);
            grid
        });
        let cell_input = cx.new(|cx| TextareaState::new(window, cx));
        cx.new(|cx: &mut Context<Self>| {
            let subs = vec![
                cx.subscribe_in(&grid, window, Self::on_grid_event),
                cx.subscribe(&cell_input, |this, input, e: &InputEvent, cx| {
                    if matches!(e, InputEvent::Change) {
                        let text = input.read(cx).value().to_string();
                        this.write_cell(text, cx);
                    }
                }),
            ];
            let mut this = Self {
                grid,
                sql,
                columns: set.columns,
                edit: None,
                info,
                editor,
                pinned: false,
                busy: false,
                status: SharedString::default(),
                accepted_sort: None,
                cell_editor: false,
                cell_input,
                cell_at: None,
                form: false,
                _subscriptions: subs,
            };
            this.grid.update(cx, |g, cx| g.set_locked(true, cx));
            this.find_table(cx);
            this
        })
    }
}

impl Render for ResultView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_cell(window, cx);
        let p = palette(cx);
        let body: AnyElement = if self.form {
            self.render_form(cx)
        } else {
            self.grid.clone().into_any_element()
        };
        let read_only = self.edit.is_none();
        v_flex()
            .size_full()
            .min_h_0()
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_hidden()
                    .child(body),
            )
            .when(self.cell_editor, |v| {
                v.child(
                    div()
                        .h(tokens::viewer::CELL_EDITOR_HEIGHT.rems())
                        .border_t_1()
                        .border_color(p.separator)
                        .child(
                            Textarea::new(&self.cell_input)
                                .disabled(read_only)
                                .size_full(),
                        ),
                )
            })
            .child(self.render_bottom_bar(cx))
    }
}
