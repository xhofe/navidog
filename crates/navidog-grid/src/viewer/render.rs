//! The table viewer's frame: toolbar, grid delegate, form/editor overlays.
//! Separated from the session/state methods so `mod.rs` stays navigable.

use super::*;

impl Render for TableViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_form(window, cx);
        self.sync_editor(window, cx);
        let form = self.view.form;
        v_flex()
            .id("table-viewer")
            .key_context(CONTEXT)
            .track_focus(&self.view.focus_handle)
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
                if this.view.show_find {
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
                this.view.show_find = true;
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
                this.view.find_columns = false;
                this.view.replace_mode = false;
                this.open_find(false, window, cx);
            }))
            .on_action(cx.listener(|this, _: &FindReplaceMode, window, cx| {
                this.view.find_columns = false;
                this.open_find(true, window, cx);
            }))
            .on_action(cx.listener(|this, _: &FindColumnsMode, window, cx| {
                this.view.find_columns = true;
                this.view.replace_mode = false;
                this.open_find(false, window, cx);
                this.find_step(false, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleMatchCase, _, cx| {
                this.view.find_case = !this.view.find_case;
                this.sync_highlight(cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleHighlight, _, cx| {
                this.view.find_highlight = !this.view.find_highlight;
                this.sync_highlight(cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleIncremental, _, cx| {
                this.view.find_incremental = !this.view.find_incremental;
                cx.notify();
            }))
            .on_action(cx.listener(|this, a: &PickField, _, cx| {
                if let Some(rule) = this.view.rules.get_mut(a.index) {
                    rule.field = a.name.to_string();
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, a: &PickOp, _, cx| {
                let Some(op) = filter::Op::parse_key(&a.op) else {
                    return;
                };
                if let Some(rule) = this.view.rules.get_mut(a.index) {
                    rule.op = op;
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, a: &PickSort, _, cx| {
                if let Some(sort) = this.view.sorts.get_mut(a.index) {
                    sort.field = a.name.to_string();
                    cx.notify();
                }
            }))
            .child(self.render_toolbar(cx))
            .when(self.view.show_find, |v| v.child(self.render_find(cx)))
            .when(self.view.show_filter, |v| v.child(self.render_filter(cx)))
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

impl TableViewer {
    pub(crate) fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let ready = self.session.is_some() && !self.busy;
        let button = |id: &'static str, icon: &'static str, label: String| {
            let mut glyph = lucide(icon);
            if let Some(color) = tokens::icon_color(icon, cx) {
                glyph = glyph.text_color(color);
            }
            Button::new(id)
                .small()
                .custom(
                    ButtonCustomVariant::new(cx)
                        .foreground(p.label)
                        .hover(p.fill)
                        .active(p.chrome_active),
                )
                .icon(glyph)
                .label(label)
        };
        h_flex()
            .h(tokens::object_toolbar::HEIGHT.rems())
            .px_2()
            .gap_1()
            .border_b_1()
            .border_color(p.separator)
            .map(|bar| {
                if self.in_transaction {
                    bar.child(
                        button("tx-commit", "check", t!("viewer.commit").to_string())
                            .disabled(!ready)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.apply_changes(true, window, cx)
                            })),
                    )
                    .child(
                        button(
                            "tx-rollback",
                            "rotate-ccw",
                            t!("viewer.rollback").to_string(),
                        )
                        .disabled(!ready)
                        .on_click(cx.listener(|this, _, _, cx| this.rollback(cx))),
                    )
                } else {
                    bar.child(
                        button(
                            "tx-begin",
                            "git-commit-horizontal",
                            t!("viewer.begin").to_string(),
                        )
                        .disabled(!ready)
                        .on_click(cx.listener(|this, _, _, cx| this.begin_transaction(cx))),
                    )
                }
            })
            .child(
                button("toggle-filter", "funnel", t!("viewer.filter").to_string())
                    .selected(self.view.show_filter)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.toggle_filter_pane(window, cx)),
                    ),
            )
            .child(
                button("viewer-find", "search", t!("viewer.find").to_string())
                    .selected(self.view.show_find)
                    .on_click(cx.listener(|this, _, window, cx| {
                        if this.view.show_find {
                            this.close_find(window, cx);
                        } else {
                            this.open_find(false, window, cx);
                        }
                    })),
            )
            .child(button("import", "file-text", t!("viewer.import").to_string()).disabled(true))
            .child(button("export", "file-text", t!("viewer.export").to_string()).disabled(true))
            .child(div().flex_1())
            .child(
                Button::new("viewer-columns")
                    .ghost()
                    .small()
                    .icon(lucide("columns-3"))
                    .tooltip(t!("viewer.columns").to_string())
                    .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                        this.focus_for_menu(window, cx);
                        this.columns_menu(cx).show(event.position(), window, cx);
                    })),
            )
            .child(
                Button::new("viewer-raw")
                    .ghost()
                    .small()
                    .icon(lucide("square-function"))
                    .tooltip(t!("viewer.raw_data").to_string())
                    .selected(self.view.grid.read(cx).raw_mode())
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_raw(cx))),
            )
            .child(
                Button::new("viewer-form")
                    .disabled(self.busy)
                    .ghost()
                    .small()
                    .icon(lucide("list"))
                    .tooltip(t!("viewer.form").to_string())
                    .selected(self.view.form)
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.view.form {
                            this.flush_form(cx);
                        }
                        this.view.form = !this.view.form;
                        this.view.form_for = None;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("viewer-editor")
                    .ghost()
                    .small()
                    .icon(lucide("code"))
                    .tooltip(t!("viewer.editor").to_string())
                    .selected(self.view.editor.is_some())
                    .on_click(cx.listener(|this, _, window, cx| {
                        if this.view.editor.take().is_some() {
                            cx.notify();
                        } else {
                            this.open_editor(Kind::Text, window, cx);
                        }
                    })),
            )
            .child(
                Button::new("focus-mode")
                    .ghost()
                    .small()
                    .icon(lucide("scan"))
                    .tooltip(t!("viewer.focus_mode").to_string())
                    .on_click(|_, window, cx| {
                        window.dispatch_action(
                            Box::new(navidog_app_state::actions::ToggleFocusMode),
                            cx,
                        )
                    }),
            )
    }

    pub(crate) fn render_bottom_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let ready = self.session.is_some() && !self.busy;
        let dirty = self.view.grid.read(cx).model(cx).has_changes();
        let icon_button = |id: &'static str, icon: &'static str, tip: String| {
            Button::new(id)
                .ghost()
                .xsmall()
                .icon(lucide(icon))
                .tooltip(tip)
                .w(tokens::viewer::BOTTOM_BUTTON_WIDTH.rems())
                .h(tokens::viewer::BOTTOM_BUTTON_HEIGHT.rems())
        };
        let page = self.page;
        let last = self.last_page();
        let model = self.view.grid.read(cx).model(cx);
        let (rows, selected) = (model.row_count(), model.head().0 + 1);
        let mut records = t!(
            "viewer.records",
            selected = selected.min(rows),
            count = rows,
            page = page + 1
        )
        .to_string();
        if let Some(total) = self.total {
            records = format!("{records}  {}", t!("viewer.total", total = total));
        }
        h_flex()
            .h(tokens::viewer::BOTTOM_BAR_HEIGHT.rems())
            .min_w_0()
            .flex_none()
            .px_2()
            .gap_1()
            .border_t_1()
            .border_color(p.separator)
            .text_xs()
            .child(
                icon_button("row-add", "plus", t!("viewer.add_row").to_string())
                    .disabled(!ready)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.flush_form(cx);
                        let dirty = {
                            let model = this.view.grid.read(cx).model(cx);
                            model.row_dirty(model.head().0)
                        };
                        if dirty {
                            if this.after_commit == AfterCommit::None {
                                this.after_commit = AfterCommit::AddRow;
                            }
                            this.apply_changes(false, window, cx);
                        } else {
                            this.insert_row(cx);
                        }
                    })),
            )
            .child(
                icon_button("row-delete", "minus", t!("viewer.delete_row").to_string())
                    .disabled(!ready)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.view
                            .grid
                            .update(cx, |g, cx| g.update_model(cx, |m| m.delete_selected_rows()));
                    })),
            )
            .child(
                icon_button("row-apply", "check", t!("viewer.apply_changes").to_string())
                    .disabled(!ready || !dirty)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.apply_changes(false, window, cx)),
                    ),
            )
            .child(
                icon_button("row-discard", "x", t!("viewer.discard_changes").to_string())
                    .disabled(!dirty || self.busy)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.view
                            .grid
                            .update(cx, |g, cx| g.update_model(cx, |m| m.discard()));
                        this.view.form_for = None;
                    })),
            )
            .child({
                let mut reload = Button::new("reload")
                    .ghost()
                    .xsmall()
                    .tooltip(t!("viewer.refresh").to_string())
                    .w(tokens::viewer::BOTTOM_BUTTON_WIDTH.rems())
                    .h(tokens::viewer::BOTTOM_BUTTON_HEIGHT.rems())
                    .disabled(self.session.is_none());
                // Disabled would fade the icon, so a spin looks like it never started.
                if self.spinning() {
                    reload = reload.child(
                        Spinner::new()
                            .icon(lucide("refresh-cw"))
                            .xsmall()
                            .ease(|t| t),
                    );
                } else {
                    reload = reload.icon(lucide("refresh-cw"));
                }
                reload.on_click(cx.listener(|this, _, window, cx| {
                    if this.busy {
                        return;
                    }
                    this.reload_committed(true, window, cx);
                }))
            })
            .child(
                icon_button("stop", "circle-stop", t!("viewer.stop").to_string())
                    .disabled(!self.busy || self.cancel.is_none())
                    .on_click(cx.listener(|this, _, _, _| this.stop())),
            )
            .child(
                icon_button("show-sql", "code", t!("viewer.show_sql").to_string())
                    .on_click(cx.listener(|this, _, window, cx| this.show_sql(window, cx))),
            )
            .child(
                // The last SQL or error is often longer than the bar; hover shows all of it
                truncated("viewer-status", self.view.status.clone())
                    .flex_1()
                    .px_2()
                    .text_color(p.label_secondary),
            )
            .when(
                !self.columns.is_empty() && !self.columns.iter().any(|c| c.primary_key),
                |bar| {
                    bar.child(
                        div()
                            .flex_none()
                            .whitespace_nowrap()
                            .text_color(p.label_secondary)
                            .child(t!("viewer.no_primary_key").to_string()),
                    )
                },
            )
            .child(div().text_color(p.label_secondary).child(records))
            .child(
                h_flex()
                    .w(tokens::viewer::PAGER_WIDTH.rems())
                    .justify_center()
                    .child(
                        icon_button(
                            "page-first",
                            "chevron-first",
                            t!("viewer.first_page").to_string(),
                        )
                        .disabled(!ready || page == 0)
                        .on_click(cx.listener(|this, _, window, cx| this.go_page(0, window, cx))),
                    )
                    .child(
                        icon_button(
                            "page-prev",
                            "chevron-left",
                            t!("viewer.prev_page").to_string(),
                        )
                        .disabled(!ready || page == 0)
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.go_page(page.saturating_sub(1), window, cx)
                            },
                        )),
                    )
                    .child(
                        icon_button(
                            "page-next",
                            "chevron-right",
                            t!("viewer.next_page").to_string(),
                        )
                        .disabled(!ready || self.limit.is_none() || last.is_some_and(|l| page >= l))
                        .on_click(cx.listener(
                            move |this, _, window, cx| this.go_page(page + 1, window, cx),
                        )),
                    )
                    .child(
                        icon_button(
                            "page-last",
                            "chevron-last",
                            t!("viewer.last_page").to_string(),
                        )
                        .disabled(!ready || last.is_none_or(|l| page >= l))
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                if let Some(l) = last {
                                    this.go_page(l, window, cx)
                                }
                            },
                        )),
                    ),
            )
            .child(
                h_flex()
                    .w(tokens::viewer::LIMIT_WIDTH.rems())
                    .gap_1()
                    .child(
                        Checkbox::new("limit")
                            .xsmall()
                            .label(t!("viewer.limit").to_string())
                            .checked(self.limit.is_some())
                            .on_click(cx.listener(|this, checked: &bool, window, cx| {
                                this.limit = checked
                                    .then(|| {
                                        this.view
                                            .limit_input
                                            .read(cx)
                                            .value()
                                            .trim()
                                            .parse()
                                            .ok()
                                            .filter(|n| *n > 0)
                                    })
                                    .flatten()
                                    .or(checked
                                        .then_some(Settings::global(cx).records_per_page.max(1)));
                                this.page = 0;
                                this.reload_committed(false, window, cx);
                            })),
                    )
                    .child(
                        Input::new(&self.view.limit_input)
                            .xsmall()
                            .flex_1()
                            .disabled(self.limit.is_none()),
                    )
                    .child(t!("viewer.per_page").to_string()),
            )
    }

    /// Grid or form, with the cell editor as a draggable split when it is open.
    pub(crate) fn render_main(
        &self,
        form: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let body = div()
            .size_full()
            .min_h_0()
            .min_w_0()
            .overflow_hidden()
            .when(!form, |d| d.child(self.view.grid.clone()))
            .when(form, |d| d.child(self.render_form(cx)));
        if self.view.editor.is_none() {
            return body.into_any_element();
        }
        let rem = window.rem_size();
        // Two button rows. The text area is the rest of the default height.
        let chrome = tokens::viewer::FILTER_HEIGHT.to_pixels(rem) * 2.;
        // raw-px: the divider position the user dragged to, kept in pixels
        let height = self
            .view
            .cell_editor_px
            .map(px)
            .unwrap_or_else(|| chrome + tokens::viewer::CELL_EDITOR_HEIGHT.to_pixels(rem));
        let viewer = cx.entity();
        v_resizable("viewer-cell-split")
            .on_resize(move |state, _, cx| {
                let Some(size) = state.read(cx).sizes().get(1).copied() else {
                    return;
                };
                viewer.update(cx, |this, _| {
                    this.view.cell_editor_px = Some(f32::from(size))
                });
            })
            .child(resizable_panel().child(body))
            .child(
                resizable_panel()
                    .size(height)
                    .size_range(chrome..gpui_kit::Pixels::MAX)
                    .child(self.render_editor(cx)),
            )
            .into_any_element()
    }
}
