//! Query editor tab (30-query-editor.md §2): toolbar, SQL editor and the results area.

mod editing;
mod render;
mod session;
mod toolbar;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use gpui_kit::component::dock::{
    BasePanel, Panel, PanelEvent, PanelId, PanelInfo, PanelState, TabGroup, panel_handle,
    register_panel,
};
use gpui_kit::component::input::{
    Copy, Cut, EditorState, Enter, Escape, IndentInline, InputEvent, MoveDown, MoveUp,
    OutdentInline, TabSize,
};
use gpui_kit::component::resizable::{h_resizable, resizable_panel, v_resizable};
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyBinding, ParentElement as _, Render, SharedString,
    Styled as _, Subscription, Task, WeakEntity, Window, actions, div, px,
};
use navidog_app_state::settings::{ResultsPosition, Settings, ShowAfterRun};
use navidog_app_state::store::ConnectionStore;
use navidog_app_state::workspace::{Selection, Workspace};
use navidog_model::object::{ObjectRef, Scope};
use navidog_session::sessions::{Sessions, SessionsEvent};
use navidog_ui::assets::lucide;
use navidog_ui::{palette, tabs, tokens};
use navidog_util::sql_format::CommentMode;
use serde::{Deserialize, Serialize};

use crate::results::{CloseAllResults, CloseResultTab, Results, TogglePinTab, UnpinAllTabs};
use crate::run::{Live, RunKind};

pub(crate) const PANEL: &str = "QueryEditor";
pub(crate) const CONTEXT: &str = "QueryEditor";

actions!(
    query,
    [
        Run,
        RunCurrent,
        RunSelected,
        Stop,
        Explain,
        ToggleContinueOnError,
        ToggleKeepResults,
        ToggleWordWrap,
        ZoomIn,
        ZoomOut,
        ActualSize,
        ResultsBelow,
        ResultsBeside,
        ResultsNewPage,
        ShowResults,
        ShowResultAfterRun,
        ShowMessagesAfterRun,
        ShowSummaryAfterRun,
        ToggleProfile,
        UpdateOutline,
    ]
);

#[derive(Clone, PartialEq, Debug, gpui_kit::Action)]
#[action(namespace = query, no_json)]
pub(crate) struct PickConnection {
    pub id: SharedString,
}

#[derive(Clone, PartialEq, Debug, gpui_kit::Action)]
#[action(namespace = query, no_json)]
pub(crate) struct PickDatabase {
    pub name: SharedString,
}

#[derive(Clone, PartialEq, Debug, gpui_kit::Action)]
#[action(namespace = query, no_json)]
pub(crate) struct PickSchema {
    pub name: SharedString,
}

/// Inputs inside a query editor: binding here overrides the text field's own keys (⌘. is "code
/// actions" and ⌘F the built-in search there).
const IN_EDITOR: &str = "QueryEditor > Input";

pub(crate) fn init(cx: &mut App) {
    use crate::find::FindPrevious;
    use crate::format::{PasteFromStack, ToggleComment};
    use navidog_grid::{FindNext, ShowFind, ShowReplace};
    cx.bind_keys([
        KeyBinding::new("cmd-r", Run, Some(CONTEXT)),
        KeyBinding::new("shift-cmd-r", RunCurrent, Some(CONTEXT)),
        KeyBinding::new("cmd-.", Stop, Some(CONTEXT)),
        KeyBinding::new("cmd-.", Stop, Some(IN_EDITOR)),
        KeyBinding::new("cmd-f", ShowFind, Some(CONTEXT)),
        KeyBinding::new("cmd-f", ShowFind, Some(IN_EDITOR)),
        KeyBinding::new("alt-cmd-f", ShowReplace, Some(IN_EDITOR)),
        KeyBinding::new("alt-cmd-f", ShowReplace, Some(CONTEXT)),
        KeyBinding::new("cmd-g", FindNext, Some(IN_EDITOR)),
        KeyBinding::new("shift-cmd-g", FindPrevious, Some(IN_EDITOR)),
        KeyBinding::new("cmd-/", ToggleComment, Some(IN_EDITOR)),
        KeyBinding::new("shift-cmd-v", PasteFromStack, Some(IN_EDITOR)),
        KeyBinding::new("cmd-s", crate::save::SaveQuery, Some(CONTEXT)),
        KeyBinding::new("shift-cmd-s", crate::save::SaveQueryAs, Some(CONTEXT)),
        KeyBinding::new("cmd-=", ZoomIn, Some(CONTEXT)),
        KeyBinding::new("cmd-+", ZoomIn, Some(CONTEXT)),
        KeyBinding::new("cmd--", ZoomOut, Some(CONTEXT)),
        KeyBinding::new("cmd-0", ActualSize, Some(CONTEXT)),
    ]);
    register_panel(cx, PANEL, |ctx, window, cx| {
        let info = match ctx.info() {
            PanelInfo::Panel(v) => v.clone(),
            _ => serde_json::Value::Null,
        };
        panel_handle(QueryEditor::restore(info, window, cx))
    });
}

/// What the editor's text belongs to.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuerySource {
    /// Not saved yet; the number tells "Untitled" tabs apart
    Untitled(usize),
    /// A saved query (`ObjectKind::Query`)
    Saved(ObjectRef),
    /// Any `.sql` file opened from disk
    External(PathBuf),
}

/// Where the editor runs, completes and saves (the toolbar pickers).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub connection: Option<String>,
    pub database: Option<String>,
    pub schema: Option<String>,
}

impl Target {
    pub fn scope(&self) -> Option<Scope> {
        Some(Scope::new(
            self.connection.clone()?,
            self.database.clone()?,
            self.schema.clone(),
        ))
    }
}

/// Panel state of untitled and external editors. Saved queries dump their `ObjectRef` instead,
/// so favorites, recent objects and the startup screen work on them like on tables.
#[derive(Serialize, Deserialize)]
struct Dumped {
    query: DumpedQuery,
}

#[derive(Serialize, Deserialize)]
struct DumpedQuery {
    id: String,
    source: QuerySource,
    target: Target,
}

static UNTITLED: AtomicUsize = AtomicUsize::new(0);

pub fn next_untitled() -> usize {
    UNTITLED.fetch_add(1, Ordering::Relaxed) + 1
}

pub struct QueryEditor {
    focus_handle: FocusHandle,
    /// Names the autosave file (30 §8)
    pub(crate) id: String,
    pub(crate) source: QuerySource,
    pub(crate) target: Target,
    pub(crate) editor: Entity<EditorState>,
    /// Changed since it was opened or saved
    pub(crate) dirty: bool,
    /// Modification time of the file when it was loaded or saved, to notice outside edits
    pub(crate) file_mtime: Option<std::time::SystemTime>,
    /// PostgreSQL schemas of the picked database
    pub(crate) schemas: Vec<String>,
    pub(crate) live: Option<Live>,
    pub(crate) running: bool,
    /// Set by Stop: the statements after the current one are skipped
    pub(crate) stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Run again once the picked connection has opened
    pub(crate) pending_run: Option<RunKind>,
    /// Last values typed for each `[$name]` parameter, and whether they were raw
    pub(crate) params: HashMap<String, (String, bool)>,
    pub(crate) results: Results,
    /// "Show results in a new page": the results page is showing instead of the editor
    pub(crate) results_page: bool,
    /// Editor preferences last applied (line numbers, folding, tab width, spaces)
    applied: Option<(bool, bool, u8, bool)>,
    /// Highlighting is SQL, so completion may run. Cached: the completion trigger
    /// runs inside the editor update and must not read that editor.
    pub(crate) sql_highlight: bool,
    /// Snippet placeholders being tabbed through
    pub(crate) tab_stops: Option<crate::snippet::TabStops>,
    /// Snippet codes the completion menu offered last
    pub(crate) offered_snippets: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
    /// An arrow key moved the completion menu since it opened (for "auto select first item")
    pub(crate) menu_moved: bool,
    pub(crate) find: crate::find::Find,
    pub(crate) stack_paste: Option<crate::format::StackPaste>,
    /// Bracket highlight frames and the caret they were computed for
    pub(crate) brackets: Option<gpui_kit::component::input::RangeDecorationCollection>,
    pub(crate) bracket_caret: Option<usize>,
    /// Zoomed font size; `None` follows the preferences
    pub(crate) zoom: Option<f32>,
    pub(crate) word_wrap: bool,
    /// The user answered the close prompt
    close_approved: bool,
    pub(crate) task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<PanelEvent> for QueryEditor {}

impl Focusable for QueryEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for QueryEditor {
    fn panel_name(&self) -> &'static str {
        PANEL
    }

    fn dump(&self, _: &App) -> PanelState {
        let info = match &self.source {
            QuerySource::Saved(o) => serde_json::to_value(o),
            source => serde_json::to_value(Dumped {
                query: DumpedQuery {
                    id: self.id.clone(),
                    source: source.clone(),
                    target: self.target.clone(),
                },
            }),
        };
        PanelState {
            panel_name: PANEL.into(),
            children: Vec::new(),
            info: PanelInfo::panel(info.unwrap_or_default()),
        }
    }

    /// Unsaved text makes the tab ask first; see `tabs::CloseGuard`.
    fn closable(&self, _: &App) -> bool {
        self.close_approved || !self.dirty
    }

    fn on_added_to(
        &mut self,
        group: WeakEntity<TabGroup>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panel = PanelId::from(cx.entity_id());
        tabs::added(panel, group, cx);
        let (editor, home) = (cx.entity().downgrade(), window.window_handle());
        let guard: tabs::CloseGuard = std::rc::Rc::new(move |cx| {
            let editor = editor.clone();
            let _ = home.update(cx, |_, window, cx| {
                let _ = editor.update(cx, |e, cx| e.ask_to_close(window, cx));
            });
        });
        tabs::set_close_guard(panel, guard, cx);
    }

    fn set_active(&mut self, active: bool, window: &mut Window, cx: &mut Context<Self>) {
        let panel = PanelId::from(cx.entity_id());
        tabs::set_active(panel, active, cx);
        tabs::set_surface(
            panel,
            active,
            tabs::MenuSurface::Query,
            tabs::TableMenu::default(),
            cx,
        );
        if active {
            set_active_editor(cx.entity().downgrade(), cx);
            self.publish_context(cx);
            if let QuerySource::Saved(o) = &self.source {
                let o = o.clone();
                Workspace::global(cx).update(cx, |w, cx| w.select(Selection::Object(o), cx));
            }
            self.check_external_change(window, cx);
        } else {
            Workspace::global(cx).update(cx, |w, cx| w.set_query_context(None, cx));
        }
    }

    fn on_removed(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if tabs::removed(PanelId::from(cx.entity_id()), cx) {
            return;
        }
        Workspace::global(cx).update(cx, |w, cx| w.set_query_context(None, cx));
        if let Some(live) = self.live.take() {
            live.close();
        }
    }
}

impl Panel for QueryEditor {
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let color = self
            .target
            .connection
            .as_ref()
            .and_then(|id| ConnectionStore::global(cx).read(cx).get(id))
            .and_then(|c| c.color.clone());
        let conn = self.connection_name(cx);
        let label = tabs::object_label(
            &self.display_name(),
            self.target.database.as_deref(),
            (!conn.is_empty()).then_some(conn.as_str()),
        );
        tabs::title(
            PanelId::from(cx.entity_id()),
            Some(lucide("file-code")),
            label,
            color.and_then(|c| tokens::connection_color(&c, cx)),
            self.dirty && !self.close_approved,
        )
    }
}

impl Render for QueryEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let position = Settings::global(cx).results_position;
        let results = self.render_results(window, cx).into_any_element();
        let split = Settings::global(cx).editor_split;
        let remember = |state: &Entity<gpui_kit::component::resizable::ResizableState>,
                        _: &mut Window,
                        cx: &mut App| {
            if let Some(size) = state.read(cx).sizes().first() {
                let size = f32::from(*size);
                Settings::update(cx, |s| s.editor_split = Some(size));
            }
        };
        let editor_panel = resizable_panel()
            // raw-px: the divider position the user dragged to, saved in pixels
            .when_some(split, |p, size| p.size(px(size)))
            .child(self.render_editor(cx));
        let body = match position {
            ResultsPosition::Below => v_resizable("query-split")
                .on_resize(remember)
                .child(editor_panel)
                .child(resizable_panel().child(results))
                .into_any_element(),
            ResultsPosition::Beside => h_resizable("query-split-beside")
                .on_resize(remember)
                .child(editor_panel)
                .child(resizable_panel().child(results))
                .into_any_element(),
            ResultsPosition::NewPage if self.results_page => results,
            ResultsPosition::NewPage => self.render_editor(cx).into_any_element(),
        };
        v_flex()
            .id("query-editor")
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .size_full()
            .min_w_0()
            .overflow_hidden()
            .bg(p.control_bg)
            .on_action(cx.listener(|this, _: &Run, window, cx| this.run(RunKind::All, window, cx)))
            .on_action(cx.listener(|this, _: &RunCurrent, window, cx| {
                this.run(RunKind::Current, window, cx)
            }))
            .on_action(cx.listener(|this, _: &RunSelected, window, cx| {
                this.run(RunKind::Selected, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &Explain, window, cx| this.run(RunKind::Explain, window, cx)),
            )
            .on_action(cx.listener(|this, _: &Stop, _, cx| this.stop(cx)))
            .on_action(|_: &ToggleContinueOnError, _, cx| {
                Settings::update(cx, |s| s.continue_on_error = !s.continue_on_error)
            })
            .on_action(|_: &ToggleKeepResults, _, cx| {
                Settings::update(cx, |s| s.keep_results = !s.keep_results)
            })
            // Result-tab menus dispatch along the focused element (usually the SQL editor)
            // and bubble to this root. Handlers on the results pane never see them.
            .on_action(
                cx.listener(|this, a: &TogglePinTab, _, cx| this.results.toggle_pin(a.id, cx)),
            )
            .on_action(
                cx.listener(|this, a: &CloseResultTab, _, cx| this.results.close_tab(a.id, cx)),
            )
            .on_action(cx.listener(|this, _: &UnpinAllTabs, _, cx| {
                let ids: Vec<u64> = this
                    .results
                    .tabs
                    .iter()
                    .filter(|t| t.pinned)
                    .map(|t| t.id)
                    .collect();
                for id in ids.into_iter().rev() {
                    this.results.toggle_pin(id, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &CloseAllResults, _, cx| {
                let ids: Vec<u64> = this.results.tabs.iter().map(|t| t.id).collect();
                for id in ids {
                    this.results.close_tab(id, cx);
                }
            }))
            .on_action(cx.listener(|this, a: &PickConnection, window, cx| {
                this.pick_connection(a.id.to_string(), window, cx)
            }))
            .on_action(cx.listener(|this, a: &PickDatabase, _, cx| {
                this.pick_database(a.name.to_string(), cx)
            }))
            .on_action(
                cx.listener(|this, a: &PickSchema, _, cx| this.pick_schema(a.name.to_string(), cx)),
            )
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| this.zoom_by(1., cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, _, cx| this.zoom_by(-1., cx)))
            .on_action(cx.listener(|this, _: &ActualSize, _, cx| {
                this.zoom = None;
                cx.notify();
            }))
            .on_action(
                cx.listener(|this, _: &ToggleWordWrap, window, cx| {
                    this.toggle_word_wrap(window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &ResultsBelow, window, cx| {
                this.place_results(ResultsPosition::Below, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ResultsBeside, window, cx| {
                this.place_results(ResultsPosition::Beside, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ResultsNewPage, window, cx| {
                this.place_results(ResultsPosition::NewPage, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &crate::results::BackToEditor, window, cx| {
                    this.back_to_editor(window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &ShowResults, _, cx| {
                if this.results.has_run() {
                    this.results_page = true;
                    cx.notify();
                }
            }))
            .on_action(|_: &ShowResultAfterRun, _, cx| {
                Settings::update(cx, |s| s.show_after_run = ShowAfterRun::Result)
            })
            .on_action(|_: &ShowMessagesAfterRun, _, cx| {
                Settings::update(cx, |s| s.show_after_run = ShowAfterRun::Messages)
            })
            .on_action(|_: &ShowSummaryAfterRun, _, cx| {
                Settings::update(cx, |s| s.show_after_run = ShowAfterRun::Summary)
            })
            .on_action(cx.listener(|this, _: &ToggleProfile, _, cx| {
                if this.dialect(cx).is_some_and(|d| d.supports_profile()) {
                    Settings::update(cx, |s| s.show_profile = !s.show_profile);
                }
            }))
            // Snippets and identifiers dragged from the info pane land at the caret (a recorded
            // deviation: the editor does not map a drop point to a text position)
            .on_drop(cx.listener(
                |this, a: &navidog_app_state::actions::InsertText, window, cx| {
                    this.insert_text(&a.text, a.snippet, window, cx)
                },
            ))
            .on_action(cx.listener(|this, _: &crate::save::SaveQuery, window, cx| {
                this.save_query(false, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &crate::save::SaveQueryAs, window, cx| {
                    this.save_query_as(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::save::SaveAsExternal, window, cx| {
                    let rebind = matches!(this.source, QuerySource::Untitled(_));
                    this.save_as_external(rebind, false, window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::save::SaveAsNavicat, window, cx| {
                    if let Some(scope) = this.target.scope() {
                        this.save_as_navicat(scope, false, window, cx);
                    }
                }),
            )
            .on_action(cx.listener(|this, _: &navidog_grid::ShowFind, window, cx| {
                this.open_find(false, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &navidog_grid::ShowReplace, window, cx| {
                    this.open_find(true, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &navidog_grid::FindNext, window, cx| {
                this.find_step(true, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &crate::find::FindPrevious, window, cx| {
                    this.find_step(false, window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::find::ToggleFindRegex, window, cx| {
                    this.toggle_find_option(|o| o.regex = !o.regex, window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::find::ToggleFindCase, window, cx| {
                    this.toggle_find_option(|o| o.case = !o.case, window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::find::ToggleFindWholeWord, window, cx| {
                    this.toggle_find_option(|o| o.whole_word = !o.whole_word, window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::find::FindLiteral, window, cx| {
                    this.open_find(false, window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::find::FindReplace, window, cx| {
                    this.open_find(true, window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::format::CommentLines, window, cx| {
                    this.comment(CommentMode::Comment, window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::format::UncommentLines, window, cx| {
                    this.comment(CommentMode::Uncomment, window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::format::ToggleComment, window, cx| {
                    this.comment(CommentMode::Toggle, window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::format::UpperCase, window, cx| {
                    this.change_case(true, window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::format::LowerCase, window, cx| {
                    this.change_case(false, window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::format::Beautify, window, cx| {
                    this.beautify(window, cx)
                }),
            )
            .on_action(
                cx.listener(|_, _: &crate::format::BeautifyOptionsDialog, window, cx| {
                    crate::format::open_beautify_options(cx.entity().downgrade(), window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::format::Minify, window, cx| this.minify(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &crate::format::PasteFromStack, window, cx| {
                    this.paste_from_stack(window, cx)
                }),
            )
            // Capture phase: these run before the text field handles the key
            .capture_action(cx.listener(|this, _: &Copy, window, cx| {
                if this.editor_focused(window, cx) {
                    this.remember_copy(cx);
                }
            }))
            .capture_action(cx.listener(|this, _: &Cut, window, cx| {
                if this.editor_focused(window, cx) {
                    this.remember_copy(cx);
                }
            }))
            .capture_action(cx.listener(|this, _: &IndentInline, window, cx| {
                if !this.editor_focused(window, cx) {
                    return;
                }
                // Tab accepts the completion like Enter (30 §4)
                if this.completion_open(cx) {
                    if this.accepts_completion(cx) {
                        this.editor.update(cx, |e, cx| {
                            e.route_overlay_action(
                                Box::new(Enter {
                                    secondary: false,
                                    shift: false,
                                }),
                                window,
                                cx,
                            );
                        });
                        cx.stop_propagation();
                    } else {
                        this.editor
                            .update(cx, |e, cx| e.dismiss_completion_overlay(cx));
                    }
                    return;
                }
                if this.step_tab_stop(true, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &Enter, window, cx| {
                if this.editor_focused(window, cx)
                    && this.completion_open(cx)
                    && !this.accepts_completion(cx)
                {
                    this.editor
                        .update(cx, |e, cx| e.dismiss_completion_overlay(cx));
                }
            }))
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
                if this.completion_open(cx) {
                    this.menu_moved = true;
                }
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
                if this.completion_open(cx) {
                    this.menu_moved = true;
                }
            }))
            .on_action(cx.listener(|this, _: &UpdateOutline, _, cx| this.ensure_outline(true, cx)))
            .capture_action(cx.listener(|this, _: &OutdentInline, window, cx| {
                if this.editor_focused(window, cx) && this.step_tab_stop(false, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &Escape, window, cx| {
                if this.tab_stops.is_some() {
                    this.end_tab_stops(cx);
                    cx.stop_propagation();
                } else if this.find.open && !this.completion_open(cx) {
                    this.close_find(window, cx);
                    cx.stop_propagation();
                } else if this.editor_focused(window, cx)
                    && !this.completion_open(cx)
                    && crate::completion::completion_on(this, cx)
                {
                    // Esc with the menu closed opens it (30 §4)
                    this.editor
                        .update(cx, |e, cx| e.show_completions(window, cx));
                    cx.stop_propagation();
                }
            }))
            .child(self.render_toolbar(cx))
            .when(self.find.open, |v| v.child(self.render_find(cx)))
            .child(div().flex_1().min_h_0().min_w_0().child(body))
    }
}

/// The query editor tab last activated, for commands from outside it (inserting identifiers).
struct ActiveEditor(WeakEntity<QueryEditor>);
impl gpui_kit::Global for ActiveEditor {}

fn set_active_editor(editor: WeakEntity<QueryEditor>, cx: &mut App) {
    cx.set_global(ActiveEditor(editor));
}

pub fn active_editor(cx: &App) -> Option<Entity<QueryEditor>> {
    cx.try_global::<ActiveEditor>()?.0.upgrade()
}

impl QueryEditor {
    pub fn new(
        source: QuerySource,
        target: Target,
        text: String,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        Self::with_id(
            uuid::Uuid::new_v4().simple().to_string(),
            source,
            target,
            text,
            window,
            cx,
        )
    }

    fn with_id(
        id: String,
        source: QuerySource,
        target: Target,
        text: String,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let s = Settings::global(cx).clone();
        let sql_highlight = language_for(&s, text.len()) == "sql";
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(language_for(&s, text.len()))
                .line_number(s.show_line_numbers)
                .folding(s.code_folding)
                .soft_wrap(s.word_wrap)
                .auto_close(false)
                .searchable(false)
                .tab_size(TabSize {
                    tab_size: s.tab_width.clamp(1, 16) as usize,
                    hard_tabs: !s.insert_spaces,
                })
        });
        editor.update(cx, |e, cx| e.set_value(text, window, cx));
        let this = cx.new(|cx: &mut Context<Self>| {
            let subs = vec![
                cx.subscribe_in(&editor, window, |this, _, e: &InputEvent, window, cx| {
                    if matches!(e, InputEvent::Change) {
                        this.on_text_changed(cx);
                        this.expand_inserted_snippet(window, cx);
                    }
                }),
                cx.subscribe_in(
                    &Sessions::global(cx),
                    window,
                    |this, _, e: &SessionsEvent, window, cx| this.on_sessions_event(e, window, cx),
                ),
                cx.observe(&ConnectionStore::global(cx), |_, _, cx| cx.notify()),
                // Caret moves only notify the editor; bracket frames follow them from here
                cx.observe(&editor, |this, _, cx| this.sync_brackets(cx)),
                // Editor preferences apply to open editors right away (30 §9)
                cx.observe_global_in::<Settings>(window, |this, window, cx| {
                    this.apply_settings(window, cx)
                }),
            ];
            let results = Results::new(window, cx);
            let find = crate::find::Find::new(window, cx);
            let offered: std::rc::Rc<std::cell::RefCell<Vec<String>>> = Default::default();
            let provider = crate::completion::Provider {
                editor: cx.entity().downgrade(),
                offered: offered.clone(),
            };
            editor.update(cx, |e, _| {
                e.lsp_mut().completion_provider = Some(std::rc::Rc::new(provider));
                e.lsp_mut().set_completion_highlighter(std::rc::Rc::new(
                    crate::completion::highlight_label,
                ));
                // raw-px: room for "column · type" labels
                e.lsp_mut().completion_menu.max_width = px(420.);
            });
            Self {
                focus_handle: cx.focus_handle(),
                id,
                source,
                target,
                file_mtime: None,
                editor,
                dirty: false,
                schemas: Vec::new(),
                live: None,
                running: false,
                stop_flag: Default::default(),
                pending_run: None,
                params: HashMap::new(),
                results,
                results_page: false,
                applied: Some((
                    s.show_line_numbers,
                    s.code_folding,
                    s.tab_width,
                    s.insert_spaces,
                )),
                sql_highlight,
                tab_stops: None,
                offered_snippets: offered,
                menu_moved: false,
                find,
                stack_paste: None,
                brackets: None,
                bracket_caret: None,
                zoom: None,
                word_wrap: s.word_wrap,
                close_approved: false,
                task: None,
                _subscriptions: subs,
            }
        });
        this.update(cx, |this, cx| {
            this.file_mtime = this
                .file_path()
                .and_then(|p| navidog_model::queries::modified(&p));
            this.load_schemas(cx);
            this.ensure_outline(false, cx);
        });
        crate::save::register(this.downgrade(), cx);
        this
    }

    /// An editor brought back from an autosave: its text is unsaved.
    pub(crate) fn restored(
        id: String,
        source: QuerySource,
        target: Target,
        text: String,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let editor = Self::with_id(id, source, target, text, window, cx);
        editor.update(cx, |e, _| e.dirty = true);
        editor
    }

    /// File > Open External File > Query…: any `.sql` file, saved back in place.
    pub fn open_external(
        path: std::path::PathBuf,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let selection = Workspace::global(cx).read(cx).selection().clone();
        let scope = selection.scope();
        let target = Target {
            connection: selection.connection_id().cloned(),
            database: scope.as_ref().map(|s| s.database.clone()),
            schema: scope.and_then(|s| s.schema),
        };
        Self::new(QuerySource::External(path), target, text, window, cx)
    }

    pub fn from_sql(
        target: Target,
        sql: String,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        Self::new(
            QuerySource::Untitled(next_untitled()),
            target,
            sql,
            window,
            cx,
        )
    }

    /// A new untitled query at the workspace selection (⌘Y, toolbar "New Query").
    pub fn untitled(window: &mut Window, cx: &mut App) -> Entity<Self> {
        let selection = Workspace::global(cx).read(cx).selection().clone();
        let scope = selection.scope();
        let target = Target {
            connection: selection.connection_id().cloned(),
            database: scope.as_ref().map(|s| s.database.clone()),
            schema: scope.and_then(|s| s.schema),
        };
        Self::new(
            QuerySource::Untitled(next_untitled()),
            target,
            String::new(),
            window,
            cx,
        )
    }

    /// Opens a saved query (double-click in the navigator, favorites, recent objects).
    pub fn open_saved(object: ObjectRef, window: &mut Window, cx: &mut App) -> Entity<Self> {
        let text = navidog_model::queries::read(&object.scope, &object.name).unwrap_or_default();
        let target = Target {
            connection: Some(object.scope.connection.clone()),
            database: Some(object.scope.database.clone()),
            schema: object.scope.schema.clone(),
        };
        Self::new(QuerySource::Saved(object), target, text, window, cx)
    }

    /// Recreates a tab from the dock layout.
    fn restore(info: serde_json::Value, window: &mut Window, cx: &mut App) -> Entity<Self> {
        if let Ok(o) = serde_json::from_value::<ObjectRef>(info.clone()) {
            return Self::open_saved(o, window, cx);
        }
        match serde_json::from_value::<Dumped>(info) {
            Ok(Dumped { query }) => {
                let text = match &query.source {
                    QuerySource::External(path) => {
                        std::fs::read_to_string(path).unwrap_or_default()
                    }
                    _ => String::new(),
                };
                if let QuerySource::Untitled(n) = query.source {
                    UNTITLED.fetch_max(n, Ordering::Relaxed);
                }
                Self::with_id(query.id, query.source, query.target, text, window, cx)
            }
            Err(_) => Self::new(
                QuerySource::Untitled(next_untitled()),
                Target::default(),
                String::new(),
                window,
                cx,
            ),
        }
    }
}

/// Plain text above the size limit or with syntax highlighting turned off (F-PREF-004).
pub(crate) fn language_for(s: &Settings, len: usize) -> &'static str {
    if !s.syntax_highlight || len > s.highlight_limit_mb as usize * 1024 * 1024 {
        "text"
    } else {
        "sql"
    }
}
