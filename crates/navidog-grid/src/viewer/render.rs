//! The table viewer's frame: toolbar, grid delegate, form/editor overlays.
//! Separated from the session/state methods so `mod.rs` stays navigable.

use super::*;

impl Render for TableViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_form(window, cx);
        self.sync_editor(window, cx);
        let form = self.form;
        v_flex()
            .id("table-viewer")
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .size_full()
            .min_w_0()
            .overflow_hidden()
            .bg(palette(cx).control_bg)
            .on_action(cx.listener(|this, _: &ApplyChanges, window, cx| {
                this.apply_changes(false, window, cx)
            }))
            .on_action(cx.listener(|this, action: &HideColumn, window, cx| {
                this.toggle_column(action.name.to_string(), window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleRawData, _, cx| this.toggle_raw(cx)))
            .on_action(
                cx.listener(|this, _: &ToggleRowId, window, cx| this.toggle_rowid(window, cx)),
            )
            .on_action(cx.listener(|this, _: &Escape, window, cx| {
                if this.show_find {
                    this.close_find(window, cx);
                } else {
                    cx.propagate();
                }
            }))
            .on_action(
                cx.listener(|this, _: &ShowFind, window, cx| this.open_find(false, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ShowReplace, window, cx| this.open_find(true, window, cx)),
            )
            .on_action(cx.listener(|this, _: &FindNext, _, cx| {
                this.show_find = true;
                this.find_step(true, cx);
            }))
            .on_action(
                cx.listener(|this, _: &ToggleFilter, window, cx| {
                    this.toggle_filter_pane(window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &BeginTransaction, _, cx| this.begin_transaction(cx)))
            .on_action(cx.listener(|this, _: &CommitTransaction, window, cx| {
                this.apply_changes(true, window, cx)
            }))
            .on_action(cx.listener(|this, _: &RollbackTransaction, _, cx| this.rollback(cx)))
            .on_action(cx.listener(|this, _: &QueryTable, window, cx| this.query_table(window, cx)))
            .on_action(cx.listener(
                |this, action: &navidog_app_state::actions::DesignTable, window, cx| {
                    if action.object.is_none() {
                        window.dispatch_action(
                            Box::new(navidog_app_state::actions::DesignTable {
                                object: Some(this.object.clone()),
                            }),
                            cx,
                        );
                    } else {
                        cx.propagate();
                    }
                },
            ))
            .on_action(
                cx.listener(|this, _: &ApplyFilterSort, window, cx| this.apply_filter(window, cx)),
            )
            .on_action(cx.listener(|this, _: &ClearFilterSort, window, cx| {
                this.clear_filter_sort(window, cx)
            }))
            .on_action(cx.listener(|this, _: &RefreshTable, window, cx| {
                this.reload_committed(true, window, cx)
            }))
            .on_action(cx.listener(|this, _: &StopLoad, _, _| this.stop()))
            .on_action(
                cx.listener(|this, _: &SortAscending, _, cx| this.sort_focused(Some(true), cx)),
            )
            .on_action(
                cx.listener(|this, _: &SortDescending, _, cx| this.sort_focused(Some(false), cx)),
            )
            .on_action(cx.listener(|this, _: &SortNone, _, cx| this.sort_focused(None, cx)))
            .on_action(cx.listener(|this, _: &FindData, window, cx| {
                this.find_columns = false;
                this.replace_mode = false;
                this.open_find(false, window, cx);
            }))
            .on_action(cx.listener(|this, _: &FindReplaceMode, window, cx| {
                this.find_columns = false;
                this.open_find(true, window, cx);
            }))
            .on_action(cx.listener(|this, _: &FindColumnsMode, window, cx| {
                this.find_columns = true;
                this.replace_mode = false;
                this.open_find(false, window, cx);
                this.find_step(false, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleMatchCase, _, cx| {
                this.find_case = !this.find_case;
                this.sync_highlight(cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleHighlight, _, cx| {
                this.find_highlight = !this.find_highlight;
                this.sync_highlight(cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleIncremental, _, cx| {
                this.find_incremental = !this.find_incremental;
                cx.notify();
            }))
            .on_action(cx.listener(|this, a: &PickField, _, cx| {
                if let Some(rule) = this.rules.get_mut(a.index) {
                    rule.field = a.name.to_string();
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, a: &PickOp, _, cx| {
                let Some(op) = filter::Op::parse_key(&a.op) else {
                    return;
                };
                if let Some(rule) = this.rules.get_mut(a.index) {
                    rule.op = op;
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, a: &PickSort, _, cx| {
                if let Some(sort) = this.sorts.get_mut(a.index) {
                    sort.field = a.name.to_string();
                    cx.notify();
                }
            }))
            .child(self.render_toolbar(cx))
            .when(self.show_find, |v| v.child(self.render_find(cx)))
            .when(self.show_filter, |v| v.child(self.render_filter(cx)))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_hidden()
                    .child(self.render_main(form, window, cx)),
            )
            .child(self.render_bottom_bar(cx))
    }
}

pub(crate) fn refresh_spinning(busy: bool, until: Option<Instant>, now: Instant) -> bool {
    busy || until.is_some_and(|t| now < t)
}

impl Drop for TableViewer {
    fn drop(&mut self) {
        if let Some(s) = self.session.take() {
            close_session(s);
        }
    }
}
