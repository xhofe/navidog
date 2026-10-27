//! Table viewer: paging, row count, WHERE / ORDER BY text, editing, commits and transactions
//! (21-table-viewer.md §3–6).

use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dock::{
    BasePanel, Panel, PanelEvent, PanelId, PanelInfo, PanelState, TabGroup, panel_handle,
    register_panel,
};
use gpui_kit::component::input::{
    Editor, EditorState, Escape, Input, InputEvent, InputState, Textarea, TextareaState,
};
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::component::resizable::{resizable_panel, v_resizable};
use gpui_kit::component::{
    Disableable as _, Selectable as _, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, ClickEvent, Context, ElementId, Entity, EventEmitter,
    FocusHandle, Focusable, Image, InteractiveElement as _, IntoElement, KeyBinding,
    ParentElement as _, PathPromptOptions, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, WeakEntity, Window, actions, div, img, px,
};
use navidog_app_state::settings::Settings;
use navidog_app_state::workspace::{Selection, Workspace, WorkspaceEvent};
use navidog_db::{CancelHandle, ColumnInfo, ResultSet};
use navidog_model::object::{ObjectKind, ObjectRef, Scope};
use navidog_session::sessions::{Sessions, SessionsEvent, SharedSession, run_on};
use navidog_sql::dialect::Dialect;
use navidog_ui::assets::lucide;
use navidog_ui::skin::{FormRow, confirm_with, truncated};
use navidog_ui::{palette, tabs, tokens};
use navidog_util::format::{decode_hex, encode_hex, pretty_json};
use navidog_util::sql_type::{Choices, choices};
use rust_i18n::t;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use crate::commit::{apply, close_session, error_message};
use crate::editor::{self, Kind};
use crate::filter::{self, Filter, Rule, Sort};
use crate::grid::{GridEvent, HideColumn, ResultGrid, ToggleRawData, ToggleRowId};

mod cell_editor;
mod external_refresh;
mod filter_pane;
mod find;
mod fk_picker;
mod form;

const PANEL: &str = "TableViewer";
const CONTEXT: &str = "TableViewer";
actions!(
    viewer,
    [
        ApplyChanges,
        ShowFind,
        FindNext,
        ShowReplace,
        ToggleFilter,
        BeginTransaction,
        CommitTransaction,
        RollbackTransaction,
        QueryTable,
        ApplyFilterSort,
        ClearFilterSort,
        RefreshTable,
        StopLoad,
        SortAscending,
        SortDescending,
        SortNone,
        FindData,
        FindReplaceMode,
        FindColumnsMode,
        ToggleMatchCase,
        ToggleHighlight,
        ToggleIncremental,
    ]
);

#[derive(Clone, PartialEq, Debug, gpui_kit::Action)]
#[action(namespace = viewer, no_json)]
struct PickField {
    index: usize,
    name: SharedString,
}

#[derive(Clone, PartialEq, Debug, gpui_kit::Action)]
#[action(namespace = viewer, no_json)]
struct PickOp {
    index: usize,
    op: SharedString,
}

#[derive(Clone, PartialEq, Debug, gpui_kit::Action)]
#[action(namespace = viewer, no_json)]
struct PickSort {
    index: usize,
    name: SharedString,
}

pub(crate) struct Draft {
    field: String,
    op: filter::Op,
    value: Entity<InputState>,
    value2: Entity<InputState>,
    or_join: bool,
    negate: bool,
    _subscriptions: Vec<Subscription>,
}

pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-s", ApplyChanges, Some(CONTEXT)),
        KeyBinding::new("cmd-f", ShowFind, Some(CONTEXT)),
        KeyBinding::new("cmd-g", FindNext, Some(CONTEXT)),
        KeyBinding::new("alt-cmd-f", ShowReplace, Some(CONTEXT)),
        KeyBinding::new("cmd-shift-f", ToggleFilter, Some(CONTEXT)),
        KeyBinding::new("cmd-r", RefreshTable, Some(CONTEXT)),
        KeyBinding::new("cmd-.", StopLoad, Some(CONTEXT)),
        KeyBinding::new("ctrl-r", ApplyFilterSort, Some(CONTEXT)),
        KeyBinding::new("shift-cmd-y", QueryTable, Some(CONTEXT)),
        KeyBinding::new(
            "secondary-d",
            navidog_app_state::actions::DesignTable { object: None },
            Some(CONTEXT),
        ),
    ]);
    register_panel(cx, PANEL, |ctx, window, cx| {
        let object = match ctx.info() {
            PanelInfo::Panel(v) => serde_json::from_value::<ObjectRef>(v.clone()).ok(),
            _ => None,
        };
        let object = object.unwrap_or_else(|| ObjectRef {
            scope: Scope::new("", "", None),
            kind: ObjectKind::Table,
            name: String::new(),
        });
        panel_handle(TableViewer::new(object, window, cx))
    });
}

/// What the next load or commit should do once it finishes (A-07). Only one action is pending at a
/// time: every setter runs while the viewer is idle, and the action is consumed by the load it
/// belongs to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum AfterCommit {
    #[default]
    None,
    /// The close prompt chose "commit and close": the tab goes once the commit succeeds.
    Close,
    /// Republish the hidden-column projection after the commit; `reload` also loads the page.
    Hidden {
        hidden: HashSet<String>,
        reload: bool,
    },
    /// Load the page once the commit started by paging, sorting or filtering succeeds.
    Reload,
    /// Start a blank row once the commit that made room for it lands, or after the page loads.
    AddRow,
}

/// Connection, paging, transaction and commit state of a table tab (A-07). `TableViewer` derefs
/// here, so existing code keeps writing `viewer.page`; the render half is [`TableView`].
pub struct TableSession {
    pub(crate) object: ObjectRef,
    pub(crate) limit: Option<u64>,
    pub(crate) page: u64,
    pub(crate) total: Option<u64>,
    pub(crate) session: Option<SharedSession>,
    pub(crate) session_generation: u64,
    pub(crate) cancel: Option<CancelHandle>,
    pub(crate) dialect: Option<Arc<dyn Dialect>>,
    pub(crate) columns: Vec<ColumnInfo>,
    pub(crate) foreign_keys: Vec<navidog_db::ForeignKey>,
    /// The user answered the close prompt; the tab may close now (pending edits are committed or dropped)
    pub(crate) close_approved: bool,
    /// What the next load or commit should do once it finishes.
    pub(crate) after_commit: AfterCommit,
    pub(crate) in_transaction: bool,
    pub(crate) transaction_pending: bool,
    pub(crate) busy: bool,
    /// Database notifications received during an operation collapse into one follow-up load.
    pub(crate) external_refresh_pending: bool,
    /// A blocked notification must still refresh catalog data on the next explicit reload.
    pub(crate) metadata_stale: bool,
    pub(crate) catalog_revision: u64,
    pub(crate) task: Option<Task<()>>,
}

/// Render and interaction state of a table tab (A-07): toolbar inputs, the find bar, the cell
/// editor, the form view and the status line. `TableViewer` derefs to [`TableSession`], so this
/// state is reached as `viewer.view`.
pub(crate) struct TableView {
    pub(crate) focus_handle: FocusHandle,
    pub(crate) grid: Entity<ResultGrid>,
    pub(crate) filter: Entity<InputState>,
    pub(crate) order: Entity<InputState>,
    pub(crate) limit_input: Entity<InputState>,
    pub(crate) show_filter: bool,
    /// Visual builder. Text mode keeps editing `filter` and `order` directly.
    pub(crate) builder: bool,
    pub(crate) rules: Vec<Draft>,
    pub(crate) sorts: Vec<Sort>,
    pub(crate) grouped: bool,
    pub(crate) group_negate: bool,
    /// Cell editor under the grid. `None` hides it (F-DATA-011).
    pub(crate) editor: Option<Kind>,
    /// Dragged cell-editor height in pixels. `None` uses the default until the user drags.
    pub(crate) cell_editor_px: Option<f32>,
    pub(crate) editor_input: Entity<TextareaState>,
    /// JSON tab. Separate from `editor_input` so it can use the JSON highlighter.
    pub(crate) json_input: Entity<EditorState>,
    /// Row and column the editor text was loaded from.
    pub(crate) editor_cell: Option<(usize, usize)>,
    /// Exact text loaded into the field editor, before a user applies it.
    pub(crate) editor_baseline: String,
    pub(crate) editor_note: SharedString,
    /// Preview of a loaded image. Binary files are not written into the cell.
    pub(crate) image: Option<Arc<Image>>,
    pub(crate) find_input: Entity<InputState>,
    pub(crate) replace_input: Entity<InputState>,
    pub(crate) show_find: bool,
    pub(crate) replace_mode: bool,
    pub(crate) find_columns: bool,
    pub(crate) find_case: bool,
    pub(crate) find_highlight: bool,
    pub(crate) find_incremental: bool,
    pub(crate) hidden: HashSet<String>,
    /// Window this tab was last added to. A popped-out tab updates it in `on_added_to`.
    pub(crate) home: Option<gpui_kit::AnyWindowHandle>,
    pub(crate) form: bool,
    pub(crate) form_inputs: Vec<Entity<InputState>>,
    pub(crate) form_for: Option<usize>,
    /// Refresh icon keeps turning through at least half a turn after a load or commit.
    pub(crate) spin: navidog_ui::refresh_spin::HalfSpin,
    /// The most recently executed SQL or error
    pub(crate) status: SharedString,
    pub(crate) _subscriptions: Vec<Subscription>,
}

/// One table tab: the session half in `state`, the render half in `view`.
pub struct TableViewer {
    state: TableSession,
    view: TableView,
}

impl std::ops::Deref for TableViewer {
    type Target = TableSession;

    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

impl std::ops::DerefMut for TableViewer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.state
    }
}

impl EventEmitter<PanelEvent> for TableViewer {}

impl Focusable for TableViewer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.view.focus_handle.clone()
    }
}

impl BasePanel for TableViewer {
    fn panel_name(&self) -> &'static str {
        PANEL
    }
    fn dump(&self, _: &App) -> PanelState {
        PanelState {
            panel_name: PANEL.into(),
            children: Vec::new(),
            info: PanelInfo::panel(serde_json::to_value(&self.object).unwrap_or_default()),
        }
    }

    /// With uncommitted changes or an open transaction the tab refuses to close; the tab close
    /// commands run `ask_to_close` instead (see `tabs::CloseGuard`).
    fn closable(&self, cx: &App) -> bool {
        !self.needs_close_prompt(cx)
    }

    fn on_added_to(
        &mut self,
        group: WeakEntity<TabGroup>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panel = PanelId::from(cx.entity_id());
        tabs::added(panel, group, cx);
        // Runs again when the tab moves to another window, so the prompt follows it
        self.view.home = Some(window.window_handle());
        let (viewer, home) = (cx.entity().downgrade(), window.window_handle());
        let guard: tabs::CloseGuard = std::rc::Rc::new(move |cx| {
            let viewer = viewer.clone();
            let _ = home.update(cx, |_, window, cx| {
                let _ = viewer.update(cx, |v, cx| v.ask_to_close(window, cx));
            });
        });
        tabs::set_close_guard(panel, guard, cx);
    }
    fn set_active(&mut self, active: bool, _: &mut Window, cx: &mut Context<Self>) {
        let panel = PanelId::from(cx.entity_id());
        tabs::set_active(panel, active, cx);
        tabs::set_surface(
            panel,
            active,
            tabs::MenuSurface::Table,
            tabs::TableMenu {
                in_transaction: self.in_transaction,
                filter_open: self.view.show_filter,
            },
            cx,
        );
        let workspace = Workspace::global(cx);
        if active {
            // The info pane and the navigator follow the tab, not the last tree click.
            let object = self.object.clone();
            workspace.update(cx, |w, cx| w.select(Selection::Object(object), cx));
        } else {
            workspace.update(cx, |w, cx| w.set_focused_column(None, cx));
        }
    }

    fn on_removed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if tabs::removed(PanelId::from(cx.entity_id()), cx) {
            return;
        }
        let Some(session) = self.session.take() else {
            return;
        };
        // Closing the session rolls back an open transaction, which is what "discard" asked for
        let dirty = self.view.grid.read(cx).model(cx).has_changes();
        if self.close_approved || (!dirty && !self.in_transaction) {
            close_session(session);
            return;
        }
        // Only reached when something closed the tab without asking first (the dock refusing
        // via `closable` is the normal path): ask now, with the tab already gone.
        let Some(commit) = self.commit_statements(cx) else {
            close_session(session);
            return;
        };
        let in_tx = self.in_transaction;
        let (ok_session, cancel_session) = (session.clone(), session);
        let name = self.object.name.clone();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (ok_session, cancel_session, commit) =
                (ok_session.clone(), cancel_session.clone(), commit.clone());
            let name = name.clone();
            alert
                .icon(lucide("info"))
                .title(t!("viewer.close_pending", name = name).to_string())
                .show_cancel(true)
                .ok_text(t!("viewer.commit").to_string())
                .cancel_text(t!("viewer.discard").to_string())
                // The tab is already gone: Esc must not quietly pick "discard"
                .keyboard(false)
                .on_ok(move |_, window, cx| {
                    let (session, commit) = (ok_session.clone(), commit.clone());
                    let job = run_on(session.clone(), move |rel| {
                        Box::pin(apply(rel, commit, in_tx, true))
                    });
                    let name = name.clone();
                    window
                        .spawn(cx, async move |cx| {
                            let result = job
                                .await
                                .map_err(|e| e.to_string())
                                .and_then(|r| r.map_err(|e| error_message(&e)));
                            close_session(session);
                            let Err(e) = result else { return };
                            tracing::warn!("commit on tab close failed: {e}");
                            let _ = cx.update(|window, cx| {
                                window.open_alert_dialog(cx, move |alert, _, _| {
                                    alert
                                        .icon(lucide("circle-x"))
                                        .title(
                                            t!("viewer.close_commit_failed", name = name)
                                                .to_string(),
                                        )
                                        .description(e.clone())
                                });
                            });
                        })
                        .detach();
                    true
                })
                .on_cancel(move |_, _, _| {
                    let session = cancel_session.clone();
                    navidog_db::runtime::spawn(async move {
                        if in_tx {
                            let _ = run_on(session.clone(), |rel| {
                                Box::pin(async move { rel.execute("ROLLBACK").await })
                            })
                            .await;
                        }
                        close_session(session);
                    });
                    true
                })
        });
    }
}

impl Panel for TableViewer {
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = navidog_app_state::store::ConnectionStore::global(cx);
        let (color, connection) = match store.read(cx).get(&self.object.scope.connection) {
            Some(c) => (c.color.clone(), Some(c.name.clone())),
            None => (None, None),
        };
        let pending = self.needs_close_prompt(cx);
        let label = tabs::object_label(
            &self.object.name,
            Some(&self.object.scope.database),
            connection.as_deref(),
        );
        tabs::title(
            PanelId::from(cx.entity_id()),
            Some(lucide("table")),
            label,
            color.and_then(|c| tokens::connection_color(&c, cx)),
            pending,
        )
    }
}

impl TableViewer {
    fn needs_close_prompt(&self, cx: &App) -> bool {
        !self.close_approved
            && self.session.is_some()
            && (self.in_transaction || self.view.grid.read(cx).model(cx).has_changes())
    }

    /// The tab close commands land here while there are uncommitted changes: commit them and
    /// close, drop them and close, or keep the tab. A failed commit keeps the tab and the changes.
    fn ask_to_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.view.grid.update(cx, |g, cx| g.commit_edit(window, cx));
        self.flush_form(cx);
        if !self.needs_close_prompt(cx) {
            self.close_now(window, cx);
            return;
        }
        let viewer = cx.entity();
        let name = self.object.name.clone();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (commit, discard) = (viewer.clone(), viewer.clone());
            let enter = viewer.clone();
            alert
                .icon(lucide("info"))
                .title(t!("viewer.close_pending", name = name).to_string())
                // Enter commits, like the primary button; Esc keeps the tab
                .on_ok(move |_, window, cx| {
                    enter.update(cx, |v, cx| v.commit_and_close(window, cx));
                    true
                })
                .on_cancel(|_, _, cx| {
                    tabs::abort_pending_dirty_closes(cx);
                    true
                })
                .footer(navidog_ui::skin::close_prompt_footer(
                    "close",
                    t!("grid.cancel").to_string(),
                    t!("viewer.discard").to_string(),
                    t!("viewer.commit").to_string(),
                    |_, window, cx| {
                        tabs::abort_pending_dirty_closes(cx);
                        window.close_dialog(cx);
                    },
                    move |_, window, cx| {
                        window.close_dialog(cx);
                        discard.update(cx, |v, cx| {
                            v.view
                                .grid
                                .update(cx, |g, cx| g.update_model(cx, |m| m.discard()));
                            v.close_now(window, cx);
                        });
                    },
                    move |_, window, cx| {
                        window.close_dialog(cx);
                        commit.update(cx, |v, cx| v.commit_and_close(window, cx));
                    },
                ))
        });
    }

    fn commit_and_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.after_commit = AfterCommit::Close;
        self.apply_changes(true, window, cx);
    }

    /// Sends a close-prompt commit that was skipped because a load still held `busy`.
    /// The commit completion path closes the tab itself and must not call this.
    fn resume_close(viewer: WeakEntity<Self>, cx: &mut App) {
        let Ok(Some(home)) = viewer.update(cx, |this, _| this.view.home) else {
            return;
        };
        let _ = home.update(cx, |_, window, cx| {
            let _ = viewer.update(cx, |this, cx| {
                if this.after_commit == AfterCommit::Close && !this.busy {
                    this.apply_changes(true, window, cx);
                }
            });
        });
    }

    /// Closes this tab past its own prompt.
    fn close_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_approved = true;
        let close = tabs::CloseTabs {
            panel: Some(PanelId::from(cx.entity_id())),
            which: tabs::Close::This,
        };
        window.dispatch_action(Box::new(close), cx);
    }
}

fn first_cell(sets: &[ResultSet]) -> Option<u64> {
    sets.first()?
        .rows
        .first()?
        .first()?
        .as_deref()?
        .parse()
        .ok()
}

impl TableViewer {
    pub fn new(object: ObjectRef, window: &mut Window, cx: &mut App) -> Entity<Self> {
        cx.new(|cx: &mut Context<Self>| {
            let grid = cx.new(|cx| ResultGrid::new(window, cx));
            let filter = cx.new(|cx| {
                InputState::new(window, cx).placeholder(t!("viewer.where_hint").to_string())
            });
            let order = cx.new(|cx| {
                InputState::new(window, cx).placeholder(t!("viewer.order_hint").to_string())
            });
            let limit_input = cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(Settings::global(cx).records_per_page.to_string())
            });
            let find_input = cx.new(|cx| InputState::new(window, cx));
            let replace_input = cx.new(|cx| InputState::new(window, cx));
            let editor_input = cx.new(|cx| TextareaState::new(window, cx));
            let json_input = cx.new(|cx| {
                EditorState::new(window, cx)
                    .language("json")
                    .line_number(false)
            });
            let apply_on_enter = |this: &mut Self,
                                  _: &Entity<InputState>,
                                  e: &InputEvent,
                                  window: &mut Window,
                                  cx: &mut Context<Self>| {
                if matches!(e, InputEvent::PressEnter { .. }) {
                    this.apply_filter(window, cx);
                }
            };
            let subs = vec![
                cx.subscribe_in(
                    &find_input,
                    window,
                    |this, _, e: &InputEvent, _, cx| match e {
                        InputEvent::PressEnter { .. } => this.find_step(true, cx),
                        InputEvent::Change if this.view.find_incremental => {
                            this.find_step(false, cx)
                        }
                        InputEvent::Change => this.sync_highlight(cx),
                        _ => {}
                    },
                ),
                cx.subscribe_in(&replace_input, window, |this, _, e: &InputEvent, _, cx| {
                    if matches!(e, InputEvent::PressEnter { .. }) {
                        this.replace_current(cx);
                    }
                }),
                cx.subscribe_in(&filter, window, apply_on_enter),
                cx.subscribe_in(&order, window, apply_on_enter),
                cx.subscribe_in(
                    &limit_input,
                    window,
                    |this, input, e: &InputEvent, window, cx| {
                        if matches!(e, InputEvent::PressEnter { .. }) {
                            this.limit = input
                                .read(cx)
                                .value()
                                .trim()
                                .parse()
                                .ok()
                                .filter(|n| *n > 0)
                                .or(this.limit);
                            this.page = 0;
                            this.reload_committed(false, window, cx);
                        }
                    },
                ),
                cx.subscribe_in(
                    &grid,
                    window,
                    |this, _, e: &GridEvent, window, cx| match e {
                        GridEvent::SortChanged(sort) => {
                            if this.view.builder {
                                this.view.sorts = sort
                                    .as_ref()
                                    .map(|(col, asc)| Sort {
                                        field: col.to_string(),
                                        asc: *asc,
                                    })
                                    .into_iter()
                                    .collect();
                                this.write_filter_sql(window, cx);
                            } else {
                                let text = sort
                                    .as_ref()
                                    .zip(this.dialect.clone())
                                    .map(|((col, asc), d)| {
                                        format!(
                                            "{} {}",
                                            d.quote_ident(col),
                                            if *asc { "ASC" } else { "DESC" }
                                        )
                                    })
                                    .unwrap_or_default();
                                this.view
                                    .order
                                    .update(cx, |s, cx| s.set_value(text, window, cx));
                            }
                            this.page = 0;
                            this.reload_committed(false, window, cx);
                        }
                        GridEvent::PickForeignKey => this.open_fk_picker(window, cx),
                        GridEvent::QuickFilter { column, op, value } => {
                            if let Some(op) = filter::Op::parse_key(op) {
                                this.quick_filter(column, op, value, window, cx);
                            }
                        }
                        GridEvent::Changed => cx.notify(),
                        // Leaving a modified row commits it (F-DATA-003)
                        GridEvent::CommitRequested => {
                            if !this.busy {
                                this.apply_changes(false, window, cx);
                            }
                        }
                        GridEvent::ColumnFocused(name) => {
                            let name = name.to_string();
                            Workspace::global(cx)
                                .update(cx, |w, cx| w.set_focused_column(Some(name), cx));
                            this.sync_editor(window, cx);
                        }
                    },
                ),
                cx.subscribe(&Workspace::global(cx), |this, _, event, cx| {
                    if let WorkspaceEvent::DatabaseChanged(connection) = event
                        && connection == &this.object.scope.connection
                    {
                        this.catalog_revision = this.catalog_revision.wrapping_add(1);
                        this.refresh_after_external_sql(cx);
                    }
                }),
                cx.subscribe(
                    &Sessions::global(cx),
                    |this, _, e: &SessionsEvent, cx| match e {
                        SessionsEvent::Opened(id)
                            if *id == this.object.scope.connection && this.session.is_none() =>
                        {
                            this.connect(cx)
                        }
                        SessionsEvent::Closed(id) | SessionsEvent::Reopening(id)
                            if *id == this.object.scope.connection =>
                        {
                            if let Some(s) = this.session.take() {
                                close_session(s);
                            }
                            this.task = None;
                            this.cancel = None;
                            this.external_refresh_pending = false;
                            this.transaction_pending = false;
                            this.set_busy(false, cx);
                            this.after_commit = AfterCommit::None;
                            this.set_in_transaction(false, cx);
                            this.view.status = t!("viewer.closed").to_string().into();
                            cx.notify();
                        }
                        _ => {}
                    },
                ),
            ];
            let mut this = Self {
                state: TableSession {
                    object,
                    session: None,
                    session_generation: 0,
                    cancel: None,
                    dialect: None,
                    columns: Vec::new(),
                    foreign_keys: Vec::new(),
                    limit: {
                        let s = Settings::global(cx);
                        s.limit_records.then_some(s.records_per_page.max(1))
                    },
                    page: 0,
                    total: None,
                    close_approved: false,
                    after_commit: AfterCommit::None,
                    in_transaction: false,
                    transaction_pending: false,
                    busy: false,
                    external_refresh_pending: false,
                    metadata_stale: false,
                    catalog_revision: 0,
                    task: None,
                },
                view: TableView {
                    focus_handle: cx.focus_handle(),
                    grid,
                    filter,
                    order,
                    limit_input,
                    find_input,
                    replace_input,
                    editor_input,
                    json_input,
                    show_filter: false,
                    builder: false,
                    rules: Vec::new(),
                    sorts: Vec::new(),
                    grouped: false,
                    group_negate: false,
                    editor: None,
                    cell_editor_px: None,
                    editor_cell: None,
                    editor_baseline: String::new(),
                    editor_note: SharedString::default(),
                    image: None,
                    show_find: false,
                    replace_mode: false,
                    find_columns: false,
                    find_case: false,
                    find_highlight: true,
                    find_incremental: false,
                    hidden: HashSet::new(),
                    home: None,
                    form: false,
                    form_inputs: Vec::new(),
                    form_for: None,
                    spin: navidog_ui::refresh_spin::HalfSpin::default(),
                    status: SharedString::default(),
                    _subscriptions: subs,
                },
            };
            this.connect(cx);
            this
        })
    }
}

mod actions;
mod editing;
mod session;

mod render;

#[cfg(test)]
mod tests;
