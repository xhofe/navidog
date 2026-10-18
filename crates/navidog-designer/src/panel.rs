use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dock::{
    BasePanel, Panel, PanelEvent, PanelId, PanelInfo, PanelState, TabGroup, panel_handle,
    register_panel,
};
use gpui_kit::component::input::{Editor, EditorState, Input, InputEvent, InputState};
use gpui_kit::component::{
    Disableable as _, Selectable as _, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyBinding, SharedString,
    Subscription, Task, WeakEntity, Window, prelude::*, uniform_list,
};
use navidog_core::actions::TableStructureChanged;
use navidog_core::object::{ObjectRef, Scope};
use navidog_db::{
    DbError, DbResult,
    dialect::Dialect,
    sessions::Sessions,
    table_design::{self, DesignColumn, TableDesign, TableSnapshot},
};
use navidog_ui::{
    assets::lucide,
    palette,
    skin::{FormRow, truncated},
    tabs, tokens,
};
use rust_i18n::t;
use serde::{Deserialize, Serialize};
use std::rc::Rc;

const PANEL: &str = "TableDesigner";
gpui_kit::actions!(
    designer,
    [
        Save,
        AddField,
        DeleteField,
        DuplicateField,
        MoveFieldUp,
        MoveFieldDown
    ]
);

pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-s", Save, Some("TableDesigner")),
        KeyBinding::new("cmd-+", AddField, Some("TableDesigner")),
        KeyBinding::new("cmd--", DeleteField, Some("TableDesigner")),
        KeyBinding::new("cmd-d", DuplicateField, Some("TableDesigner")),
        KeyBinding::new("cmd-up", MoveFieldUp, Some("TableDesigner")),
        KeyBinding::new("cmd-down", MoveFieldDown, Some("TableDesigner")),
        KeyBinding::new("cmd-s", Save, Some("TableDesigner > Input")),
        KeyBinding::new("cmd-+", AddField, Some("TableDesigner > Input")),
        KeyBinding::new("cmd--", DeleteField, Some("TableDesigner > Input")),
        KeyBinding::new("cmd-d", DuplicateField, Some("TableDesigner > Input")),
        KeyBinding::new("cmd-up", MoveFieldUp, Some("TableDesigner > Input")),
        KeyBinding::new("cmd-down", MoveFieldDown, Some("TableDesigner > Input")),
    ]);
    register_panel(cx, PANEL, |ctx, window, cx| {
        let saved = match ctx.info() {
            PanelInfo::Panel(v) => serde_json::from_value::<Saved>(v.clone()).ok(),
            _ => None,
        };
        let saved = saved.unwrap_or_else(|| Saved {
            scope: Scope::new("", "", None),
            draft: TableDesign::new(),
            baseline: None,
        });
        panel_handle(TableDesigner::restore(saved, window, cx))
    });
}
#[derive(Clone, Serialize, Deserialize)]
struct Saved {
    scope: Scope,
    draft: TableDesign,
    baseline: Option<TableSnapshot>,
}
#[derive(Clone, Copy, PartialEq)]
enum Page {
    Fields,
    Constraints,
    Indexes,
    Options,
    Preview,
}

pub struct TableDesigner {
    focus: FocusHandle,
    scope: Scope,
    draft: TableDesign,
    baseline: Option<TableSnapshot>,
    table_name: Entity<InputState>,
    name: Entity<InputState>,
    definition: Entity<InputState>,
    constraints: Entity<EditorState>,
    indexes: Entity<EditorState>,
    options: Entity<EditorState>,
    sql: Entity<EditorState>,
    selected: Option<u64>,
    next_id: u64,
    page: Page,
    dialect: Dialect,
    busy: bool,
    loaded: bool,
    needs_reload: bool,
    error: Option<String>,
    preview: Option<Vec<String>>,
    close_approved: bool,
    task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}
impl EventEmitter<PanelEvent> for TableDesigner {}
impl Focusable for TableDesigner {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl BasePanel for TableDesigner {
    fn panel_name(&self) -> &'static str {
        PANEL
    }
    fn dump(&self, cx: &App) -> PanelState {
        let mut saved = Saved {
            scope: self.scope.clone(),
            draft: self.draft.clone(),
            baseline: self.baseline.clone(),
        };
        saved.draft.name = self.table_name.read(cx).value().to_string();
        PanelState {
            panel_name: PANEL.into(),
            children: Vec::new(),
            info: PanelInfo::panel(serde_json::to_value(saved).unwrap_or_default()),
        }
    }
    fn closable(&self, _: &App) -> bool {
        !self.busy && (self.close_approved || !self.dirty())
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
        let id = PanelId::from(cx.entity_id());
        tabs::set_active(id, active, cx);
        tabs::set_surface(
            id,
            active,
            tabs::MenuSurface::Other,
            tabs::TableMenu::default(),
            cx,
        );
    }
    fn on_removed(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if tabs::removed(PanelId::from(cx.entity_id()), cx) {
            return;
        }
        self.task.take();
    }
}
impl Panel for TableDesigner {
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let name = if self.draft.name.is_empty() {
            t!("designer.untitled").to_string()
        } else {
            self.draft.name.clone()
        };
        tabs::title(
            PanelId::from(cx.entity_id()),
            Some(lucide("table")),
            format!(
                "{} — {} @{}",
                name,
                t!("designer.title"),
                self.scope.database
            ),
            None,
            self.dirty(),
        )
    }
}
impl TableDesigner {
    pub fn new(scope: Scope, window: &mut Window, cx: &mut App) -> Entity<Self> {
        Self::restore(
            Saved {
                scope,
                draft: TableDesign::new(),
                baseline: None,
            },
            window,
            cx,
        )
    }
    pub fn open(object: ObjectRef, window: &mut Window, cx: &mut App) -> Entity<Self> {
        let entity = Self::new(object.scope, window, cx);
        entity.update(cx, |this, cx| {
            this.draft.name = object.name;
            this.loaded = false;
            this.load(window, cx);
        });
        entity
    }
    fn restore(saved: Saved, window: &mut Window, cx: &mut App) -> Entity<Self> {
        cx.new(|cx: &mut Context<Self>| {
            let input = |value: String, window: &mut Window, cx: &mut Context<Self>| {
                cx.new(|cx| InputState::new(window, cx).default_value(value))
            };
            let editor = |value: String, window: &mut Window, cx: &mut Context<Self>| {
                cx.new(|cx| {
                    EditorState::new(window, cx)
                        .language("sql")
                        .default_value(value)
                })
            };
            let table_name = input(saved.draft.name.clone(), window, cx);
            let name = input(
                saved
                    .draft
                    .columns
                    .first()
                    .map(|c| c.name.clone())
                    .unwrap_or_default(),
                window,
                cx,
            );
            let definition = input(
                saved
                    .draft
                    .columns
                    .first()
                    .map(|c| c.definition.clone())
                    .unwrap_or_default(),
                window,
                cx,
            );
            let constraints = editor(saved.draft.constraints.join("\n"), window, cx);
            let indexes = editor(
                saved
                    .draft
                    .indexes
                    .iter()
                    .map(|s| format!("{};", s.trim_end_matches(';')))
                    .collect::<Vec<_>>()
                    .join("\n"),
                window,
                cx,
            );
            let options = editor(saved.draft.options.clone(), window, cx);
            let sql = cx.new(|cx| EditorState::new(window, cx).language("sql"));
            sql.update(cx, |s, cx| s.set_readonly(true, cx));
            let mut subscriptions = Vec::new();
            for field in [&table_name, &name, &definition] {
                subscriptions.push(cx.subscribe_in(
                    field,
                    window,
                    |this, _, event: &InputEvent, _, cx| {
                        if matches!(event, InputEvent::Change) {
                            this.flush(cx);
                            cx.notify();
                        }
                    },
                ));
            }
            for field in [&constraints, &indexes, &options] {
                subscriptions.push(cx.subscribe_in(
                    field,
                    window,
                    |this, _, event: &InputEvent, _, cx| {
                        if matches!(event, InputEvent::Change) {
                            this.flush(cx);
                            cx.notify();
                        }
                    },
                ));
            }
            let dialect = Sessions::global(cx)
                .read(cx)
                .dialect(&saved.scope.connection)
                .unwrap_or(Dialect::Sqlite);
            Self {
                focus: cx.focus_handle().tab_stop(true),
                scope: saved.scope,
                selected: saved.draft.columns.first().map(|c| c.id),
                next_id: saved.draft.columns.iter().map(|c| c.id).max().unwrap_or(0) + 1,
                draft: saved.draft,
                baseline: saved.baseline,
                table_name,
                name,
                definition,
                constraints,
                indexes,
                options,
                sql,
                page: Page::Fields,
                dialect,
                busy: false,
                loaded: true,
                needs_reload: false,
                error: None,
                preview: None,
                close_approved: false,
                task: None,
                _subscriptions: subscriptions,
            }
        })
    }
    fn dirty(&self) -> bool {
        self.baseline
            .as_ref()
            .is_none_or(|old| old.design != self.draft)
    }
    fn flush(&mut self, cx: &App) {
        let previous = self.draft.clone();
        self.draft.name = self.table_name.read(cx).value().to_string();
        if let Some(c) = self
            .draft
            .columns
            .iter_mut()
            .find(|c| Some(c.id) == self.selected)
        {
            c.name = self.name.read(cx).value().to_string();
            c.definition = self.definition.read(cx).value().to_string();
        }
        self.draft.constraints = self
            .constraints
            .read(cx)
            .value()
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
        self.draft.indexes =
            navidog_db::sql::split_script(&self.indexes.read(cx).value(), self.dialect)
                .into_iter()
                .map(|s| s.text)
                .collect();
        self.draft.options = self.options.read(cx).value().to_string();
        if self.draft != previous {
            self.preview = None;
        }
    }
    fn select(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.flush(cx);
        self.selected = Some(id);
        self.sync_column(window, cx);
        cx.notify();
    }
    fn sync_column(&self, window: &mut Window, cx: &mut Context<Self>) {
        let c = self
            .draft
            .columns
            .iter()
            .find(|c| Some(c.id) == self.selected);
        self.name.update(cx, |s, cx| {
            s.set_value(c.map(|c| c.name.clone()).unwrap_or_default(), window, cx)
        });
        self.definition.update(cx, |s, cx| {
            s.set_value(
                c.map(|c| c.definition.clone()).unwrap_or_default(),
                window,
                cx,
            )
        });
    }
    fn add(&mut self, duplicate: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || !self.loaded {
            return;
        }
        self.flush(cx);
        let definition = if duplicate {
            self.draft
                .columns
                .iter()
                .find(|c| Some(c.id) == self.selected)
                .map(|c| c.definition.clone())
                .unwrap_or_else(|| "TEXT".into())
        } else {
            "TEXT".into()
        };
        let id = self.next_id;
        self.next_id += 1;
        self.draft
            .columns
            .push(DesignColumn::new(id, format!("field_{id}"), definition));
        self.selected = Some(id);
        self.preview = None;
        self.sync_column(window, cx);
        cx.notify();
    }
    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || !self.loaded {
            return;
        }
        self.flush(cx);
        self.draft.columns.retain(|c| Some(c.id) != self.selected);
        self.selected = self.draft.columns.first().map(|c| c.id);
        self.preview = None;
        self.sync_column(window, cx);
        cx.notify();
    }
    fn move_field(&mut self, up: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || !self.loaded {
            return;
        }
        self.flush(cx);
        if let Some(ix) = self
            .draft
            .columns
            .iter()
            .position(|c| Some(c.id) == self.selected)
        {
            let next = if up {
                ix.saturating_sub(1)
            } else {
                (ix + 1).min(self.draft.columns.len() - 1)
            };
            self.draft.columns.swap(ix, next);
            self.preview = None;
        }
        self.sync_column(window, cx);
        cx.notify();
    }
    fn show_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || !self.loaded || self.needs_reload {
            return;
        }
        self.flush(cx);
        match table_design::plan(
            self.dialect,
            &self.scope,
            self.baseline.as_ref(),
            &self.draft,
        ) {
            Ok(sql) => {
                self.sql.update(cx, |s, cx| {
                    s.set_value(
                        if sql.is_empty() {
                            format!("-- {}", t!("designer.no_changes"))
                        } else {
                            sql.iter()
                                .map(|s| format!("{};", s.trim_end_matches(';')))
                                .collect::<Vec<_>>()
                                .join("\n\n")
                        },
                        window,
                        cx,
                    )
                });
                self.preview = Some(sql);
                self.page = Page::Preview;
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }
    fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.flush(cx);
        if !self.dirty() || !self.loaded {
            self.load(window, cx);
            return;
        }
        let weak = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let weak = weak.clone();
            dialog
                .title(t!("designer.reload_title").to_string())
                .footer(
                    h_flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("designer-reload-cancel")
                                .label(t!("designer.cancel").to_string())
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("designer-reload-confirm")
                                .label(t!("designer.reload").to_string())
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let _ = weak.update(cx, |this, cx| this.load(window, cx));
                                }),
                        ),
                )
        });
    }
    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.request(false, window, cx);
    }
    fn request(&mut self, save: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy
            || (save && (self.needs_reload || self.preview.as_ref().is_none_or(Vec::is_empty)))
        {
            return;
        }
        let Some(job) = Sessions::global(cx)
            .read(cx)
            .new_session(&self.scope.connection, Some(&self.scope.database))
        else {
            self.error = Some(t!("designer.disconnected").to_string());
            cx.notify();
            return;
        };
        self.busy = true;
        self.error = None;
        let scope = self.scope.clone();
        let draft = self.draft.clone();
        let baseline = self.baseline.clone();
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result: DbResult<TableSnapshot> = async {
                let mut session = job.await.map_err(|e| DbError::msg(e.to_string()))??;
                let job = navidog_db::runtime::spawn(async move {
                    let result = async {
                        let r = session
                            .relational()
                            .ok_or_else(|| DbError::msg("Not a relational connection."))?;
                        if r.dialect() == Dialect::MySql {
                            r.execute(&format!(
                                "USE {}",
                                Dialect::MySql.quote_ident(&scope.database)
                            ))
                            .await?;
                        }
                        if save {
                            table_design::apply(r, &scope, baseline.as_ref(), &draft).await
                        } else {
                            table_design::load(r, &scope, &draft.name).await
                        }
                    }
                    .await;
                    let _ = session.close().await;
                    result
                });
                job.await.map_err(|e| DbError::msg(e.to_string()))?
            }
            .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(snapshot) => {
                        this.draft = snapshot.design.clone();
                        this.baseline = Some(snapshot);
                        this.loaded = true;
                        this.needs_reload = false;
                        this.preview = None;
                        this.selected = this.draft.columns.first().map(|c| c.id);
                        this.next_id =
                            this.draft.columns.iter().map(|c| c.id).max().unwrap_or(0) + 1;
                        this.sync_all(window, cx);
                        if save {
                            window.push_notification(t!("designer.saved").to_string(), cx);
                            window.dispatch_action(
                                Box::new(TableStructureChanged {
                                    scope: this.scope.clone(),
                                }),
                                cx,
                            );
                        }
                    }
                    Err(e) => {
                        this.error = Some(e.to_string());
                        if save && this.dialect == Dialect::MySql {
                            this.needs_reload = true;
                        }
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn sync_all(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.table_name
            .update(cx, |s, cx| s.set_value(self.draft.name.clone(), window, cx));
        self.constraints.update(cx, |s, cx| {
            s.set_value(self.draft.constraints.join("\n"), window, cx)
        });
        self.indexes.update(cx, |s, cx| {
            s.set_value(
                self.draft
                    .indexes
                    .iter()
                    .map(|s| format!("{};", s.trim_end_matches(';')))
                    .collect::<Vec<_>>()
                    .join("\n"),
                window,
                cx,
            )
        });
        self.options.update(cx, |s, cx| {
            s.set_value(self.draft.options.clone(), window, cx)
        });
        self.sync_column(window, cx);
    }
    fn ask_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let weak = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let weak = weak.clone();
            dialog
                .title(t!("designer.close_title").to_string())
                .child(t!("designer.close_body").to_string())
                .footer(
                    h_flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("designer-close-cancel")
                                .label(t!("designer.cancel").to_string())
                                .on_click(|_, w, cx| w.close_dialog(cx)),
                        )
                        .child(
                            Button::new("designer-close-discard")
                                .label(t!("designer.discard").to_string())
                                .on_click(move |_, w, cx| {
                                    w.close_dialog(cx);
                                    if let Ok(id) = weak.update(cx, |this, cx| {
                                        this.close_approved = true;
                                        PanelId::from(cx.entity_id())
                                    }) {
                                        w.dispatch_action(
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
mod view;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
