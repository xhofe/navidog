use super::*;

use std::collections::HashSet;

use gpui_kit::component::dock::PanelId;
use gpui_kit::component::input::{Editor, EditorState};
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{App, Context, Focusable, Window, div};
use navidog_db::dialect::Dialect;
use navidog_ui::{tabs, tokens};
use rust_i18n::t;

use crate::grid::HideColumn;

impl TableViewer {
    pub(crate) fn note_menu(&self, cx: &mut Context<Self>) {
        tabs::set_table_menu(
            PanelId::from(cx.entity_id()),
            tabs::TableMenu {
                in_transaction: self.in_transaction,
                filter_open: self.view.show_filter,
            },
            cx,
        );
    }

    pub(crate) fn set_in_transaction(&mut self, on: bool, cx: &mut Context<Self>) {
        self.in_transaction = on;
        self.note_menu(cx);
    }

    pub(crate) fn toggle_filter_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.view.show_filter = !self.view.show_filter;
        if self.view.show_filter {
            self.focus_filter_input(window, cx);
        } else if self.view.focus_handle.contains_focused(window, cx) {
            // The focused field is in this pane and unmounts with it. Leave focus on the
            // grid, or the shortcut has nowhere to land and cannot open the pane again.
            self.focus_grid(window, cx);
        }
        self.note_menu(cx);
        cx.notify();
    }

    /// Where keyboard input goes once a pane that held it has closed.
    pub(crate) fn focus_grid(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.view.form {
            window.focus(&self.view.focus_handle, cx);
        } else {
            let handle = self.view.grid.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        }
    }

    /// Text mode focuses the WHERE field. The builder has no WHERE field, so the first
    /// condition's value takes its place; an empty builder leaves focus where it was.
    pub(crate) fn focus_filter_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.view.builder {
            if let Some(rule) = self.view.rules.first() {
                rule.value.update(cx, |s, cx| s.focus(window, cx));
            }
        } else {
            self.view.filter.update(cx, |s, cx| s.focus(window, cx));
        }
    }

    /// "Query Table": `SELECT * FROM` this table, in a new query, then run.
    pub(crate) fn query_table(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dialect = self.dialect.unwrap_or(Dialect::Sqlite);
        let sql = format!(
            "SELECT * FROM {}",
            dialect.qualified(&self.object.scope, &self.object.name)
        );
        window.dispatch_action(
            Box::new(navidog_app_state::actions::NewQueryText {
                connection: self.object.scope.connection.clone().into(),
                database: self.object.scope.database.clone().into(),
                schema: self.object.scope.schema.clone().map(Into::into),
                sql: sql.into(),
            }),
            cx,
        );
    }

    pub(crate) fn sort_focused(&mut self, dir: Option<bool>, cx: &mut Context<Self>) {
        let col = self.view.grid.read(cx).model(cx).head().1;
        if self.view.grid.read(cx).model(cx).columns.get(col).is_none() {
            return;
        }
        self.view.grid.update(cx, |g, cx| {
            g.request_sort(dir.map(|_| col), dir.unwrap_or(true), cx)
        });
    }

    pub(crate) fn clear_filter_sort(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.view.rules.clear();
        self.view.sorts.clear();
        self.view.grouped = false;
        self.view.group_negate = false;
        self.view
            .filter
            .update(cx, |s, cx| s.set_value(String::new(), window, cx));
        self.view
            .order
            .update(cx, |s, cx| s.set_value(String::new(), window, cx));
        self.page = 0;
        self.reload_committed(true, window, cx);
    }

    pub(crate) fn begin_transaction(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.dialect else { return };
        self.run_simple(
            d.begin_transaction(),
            |this, cx| this.set_in_transaction(true, cx),
            cx,
        );
    }

    pub(crate) fn rollback(&mut self, cx: &mut Context<Self>) {
        self.run_simple(
            "ROLLBACK",
            |this, cx| {
                this.set_in_transaction(false, cx);
                this.reload(true, cx);
            },
            cx,
        );
    }

    pub(crate) fn stop(&mut self) {
        if let Some(cancel) = self.cancel.clone() {
            navidog_db::runtime::spawn(async move {
                if let Err(e) = cancel().await {
                    tracing::warn!("cancel query failed: {e}");
                }
            });
        }
    }

    pub(crate) fn last_page(&self) -> Option<u64> {
        let (total, limit) = (self.total?, self.limit?);
        Some(total.saturating_sub(1) / limit.max(1))
    }

    pub(crate) fn go_page(&mut self, page: u64, window: &mut Window, cx: &mut Context<Self>) {
        let page = self.last_page().map_or(page, |last| page.min(last));
        if page != self.page {
            self.page = page;
            self.reload_committed(false, window, cx);
        }
    }

    pub(crate) fn reproject(&mut self, cx: &mut Context<Self>) {
        if !self
            .view
            .grid
            .update(cx, |g, cx| g.set_hidden_columns(&self.view.hidden, cx))
        {
            return;
        }
        self.view.form_for = None;
        // The focused cell may keep its coordinates on new rows; the next `render` reloads the
        // cell editor through `sync_editor`
        self.view.editor_cell = None;
    }

    /// The menu and display change together only after the grid accepts the projection.
    pub(crate) fn publish_hidden(&mut self, next: HashSet<String>, cx: &mut Context<Self>) {
        if self
            .view
            .grid
            .update(cx, |g, cx| g.set_hidden_columns(&next, cx))
        {
            self.view.hidden = next;
            self.view.form_for = None;
            self.view.editor_cell = None;
            cx.notify();
        }
    }

    pub(crate) fn insert_row(&mut self, cx: &mut Context<Self>) {
        self.view.grid.update(cx, |g, cx| {
            let mut row = 0;
            g.update_model(cx, |m| row = m.insert_row());
            g.scroll_to_row(row, cx);
            g.settle(cx);
        });
        self.view.form_for = None;
    }

    pub(crate) fn toggle_column(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            return;
        }
        self.view.grid.update(cx, |g, cx| g.commit_edit(window, cx));
        self.flush_form(cx);
        let visible = self
            .view
            .grid
            .read(cx)
            .model(cx)
            .all_columns()
            .filter(|c| !self.view.hidden.contains(c.name.as_ref()))
            .count();
        let mut next = self.view.hidden.clone();
        if next.contains(&name) {
            next.remove(&name);
        } else if visible <= 1 {
            return;
        } else {
            next.insert(name);
        }
        if self.view.grid.read(cx).model(cx).has_changes() {
            self.after_commit = match std::mem::take(&mut self.after_commit) {
                // The close prompt owns the next commit; the projection goes first.
                AfterCommit::Close => AfterCommit::Close,
                AfterCommit::Hidden { reload, .. } => AfterCommit::Hidden {
                    hidden: next,
                    reload,
                },
                AfterCommit::Reload => AfterCommit::Hidden {
                    hidden: next,
                    reload: true,
                },
                AfterCommit::AddRow | AfterCommit::None => AfterCommit::Hidden {
                    hidden: next,
                    reload: false,
                },
            };
            self.apply_changes(false, window, cx);
        } else {
            self.publish_hidden(next, cx);
        }
    }

    pub(crate) fn columns_menu(&self, cx: &App) -> NativeMenu {
        let model = self.view.grid.read(cx).model(cx);
        let names: Vec<String> = if model.all_columns().next().is_none() {
            self.columns.iter().map(|c| c.name.clone()).collect()
        } else {
            model.all_columns().map(|c| c.name.to_string()).collect()
        };
        names.into_iter().fold(NativeMenu::new(), |menu, name| {
            let shown = !self.view.hidden.contains(&name);
            menu.menu_with_check(
                name.clone(),
                shown,
                Box::new(HideColumn { name: name.into() }),
            )
        })
    }

    pub(crate) fn toggle_raw(&mut self, cx: &mut Context<Self>) {
        self.view.form_for = None;
        let raw = !self.view.grid.read(cx).raw_mode();
        self.view.grid.update(cx, |g, cx| g.set_raw_mode(raw, cx));
        cx.notify();
    }

    pub(crate) fn show_sql(&self, window: &mut Window, cx: &mut Context<Self>) {
        let pending = self.commit_statements(cx).statements.join(";\n");
        let text = if pending.is_empty() {
            self.view.status.to_string()
        } else {
            format!("{}\n\n{pending}", self.view.status)
        };
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("sql")
                .line_number(false)
        });
        editor.update(cx, |s, cx| {
            s.set_readonly(true, cx);
            s.set_value(text, window, cx);
        });
        window.open_dialog(cx, move |dialog, window, _| {
            dialog
                .w(tokens::conn_form::WIDTH.to_pixels(window.rem_size()))
                .title(t!("viewer.show_sql").to_string())
                .child(
                    div()
                        .id("sql-preview")
                        .h(tokens::viewer::CELL_EDITOR_HEIGHT.rems())
                        .w_full()
                        .child(
                            Editor::new(&editor)
                                .readonly(true)
                                .bordered(false)
                                .size_full(),
                        ),
                )
        });
    }

    pub(crate) fn toggle_rowid(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialect != Some(Dialect::Sqlite) {
            return;
        }
        let on = !self.view.grid.read(cx).show_rowid;
        self.view.grid.update(cx, |g, cx| g.set_show_rowid(on, cx));
        self.reload_committed(false, window, cx);
    }

    /// Menu items dispatch along the focused element. Stay on the grid when it
    /// already has focus; otherwise focus this viewer so the item is handled.
    pub(crate) fn focus_for_menu(&self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.view.focus_handle.contains_focused(window, cx) {
            window.focus(&self.view.focus_handle, cx);
        }
    }
}
