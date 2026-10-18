use super::*;

impl Render for TableDesigner {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let disabled = self.busy || !self.loaded;
        let selected = self.selected;
        let rows = self.draft.columns.clone();
        let fields = uniform_list(
            "designer-columns",
            rows.len(),
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|ix| {
                        let c = &rows[ix];
                        let id = c.id;
                        h_flex()
                            .id(("designer-column", id))
                            .h(tokens::list::ROW_HEIGHT.rems())
                            .w_full()
                            .px_2()
                            .gap_2()
                            .min_w_0()
                            .when(selected == Some(id), |d| {
                                d.bg(palette(cx).selection)
                                    .text_color(palette(cx).selection_text)
                            })
                            .child(truncated(("designer-column-name", id), c.name.clone()).flex_1())
                            .child(
                                truncated(("designer-column-type", id), c.definition.clone())
                                    .flex_1(),
                            )
                            .on_mouse_down(
                                gpui_kit::MouseButton::Left,
                                cx.listener(move |this, _, window, cx| this.select(id, window, cx)),
                            )
                    })
                    .collect()
            }),
        )
        .flex_1()
        .min_h_0();
        v_flex()
            .id("table-designer")
            .key_context("TableDesigner")
            .track_focus(&self.focus)
            .size_full()
            .min_w_0()
            .bg(p.control_bg)
            .text_color(p.label)
            .on_action(cx.listener(|this, _: &Save, w, cx| this.show_preview(w, cx)))
            .on_action(cx.listener(|this, _: &AddField, w, cx| this.add(false, w, cx)))
            .on_action(cx.listener(|this, _: &DuplicateField, w, cx| this.add(true, w, cx)))
            .on_action(cx.listener(|this, _: &DeleteField, w, cx| this.delete(w, cx)))
            .on_action(cx.listener(|this, _: &MoveFieldUp, w, cx| this.move_field(true, w, cx)))
            .on_action(cx.listener(|this, _: &MoveFieldDown, w, cx| this.move_field(false, w, cx)))
            .child(
                h_flex()
                    .min_h(tokens::object_toolbar::HEIGHT.rems())
                    .flex_wrap()
                    .px_2()
                    .gap_1()
                    .border_b_1()
                    .border_color(p.separator)
                    .child(
                        Button::new("designer-save")
                            .ghost()
                            .small()
                            .label(t!("designer.save").to_string())
                            .disabled(disabled || self.needs_reload)
                            .on_click(cx.listener(|this, _, w, cx| this.show_preview(w, cx))),
                    )
                    .child(
                        Button::new("designer-execute")
                            .small()
                            .label(t!("designer.execute").to_string())
                            .disabled(
                                disabled
                                    || self.needs_reload
                                    || self.preview.as_ref().is_none_or(Vec::is_empty),
                            )
                            .on_click(cx.listener(|this, _, w, cx| this.request(true, w, cx))),
                    )
                    .when(self.baseline.is_some() || self.needs_reload, |bar| {
                        bar.child(
                            Button::new("designer-reload-toolbar")
                                .ghost()
                                .small()
                                .label(t!("designer.reload").to_string())
                                .disabled(self.busy)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.reload(window, cx)),
                                ),
                        )
                    })
                    .child(
                        Button::new("designer-add")
                            .ghost()
                            .small()
                            .icon(lucide("plus"))
                            .tooltip(t!("designer.add").to_string())
                            .accessibility_label(t!("designer.add").to_string())
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, w, cx| this.add(false, w, cx))),
                    )
                    .child(
                        Button::new("designer-delete")
                            .ghost()
                            .small()
                            .icon(lucide("minus"))
                            .tooltip(t!("designer.delete").to_string())
                            .accessibility_label(t!("designer.delete").to_string())
                            .disabled(disabled || selected.is_none())
                            .on_click(cx.listener(|this, _, w, cx| this.delete(w, cx))),
                    )
                    .child(
                        Button::new("designer-duplicate")
                            .ghost()
                            .small()
                            .icon(lucide("copy"))
                            .tooltip(t!("designer.duplicate").to_string())
                            .accessibility_label(t!("designer.duplicate").to_string())
                            .disabled(disabled || selected.is_none())
                            .on_click(cx.listener(|this, _, w, cx| this.add(true, w, cx))),
                    )
                    .child(
                        Button::new("designer-up")
                            .ghost()
                            .small()
                            .icon(lucide("arrow-up"))
                            .tooltip(t!("designer.up").to_string())
                            .accessibility_label(t!("designer.up").to_string())
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, w, cx| this.move_field(true, w, cx))),
                    )
                    .child(
                        Button::new("designer-down")
                            .ghost()
                            .small()
                            .icon(lucide("arrow-down"))
                            .tooltip(t!("designer.down").to_string())
                            .accessibility_label(t!("designer.down").to_string())
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, w, cx| this.move_field(false, w, cx))),
                    ),
            )
            .child(
                h_flex().px_2().py_1().child(
                    FormRow::new(
                        t!("designer.table_name").to_string(),
                        Input::new(&self.table_name)
                            .id("designer-table-name")
                            .disabled(disabled || self.baseline.is_some()),
                    )
                    .hug_label(),
                ),
            )
            .child(
                h_flex().px_2().gap_1().flex_wrap().children(
                    [
                        (Page::Fields, "fields"),
                        (Page::Constraints, "constraints"),
                        (Page::Indexes, "indexes"),
                        (Page::Options, "options"),
                        (Page::Preview, "preview"),
                    ]
                    .into_iter()
                    .map(|(page, key)| {
                        Button::new(SharedString::from(format!("designer-page-{key}")))
                            .ghost()
                            .small()
                            .label(t!(format!("designer.{key}")).to_string())
                            .selected(self.page == page)
                            .on_click(cx.listener(move |this, _, w, cx| {
                                if page == Page::Preview {
                                    this.show_preview(w, cx);
                                } else {
                                    this.page = page;
                                    cx.notify();
                                }
                            }))
                    }),
                ),
            )
            .when(self.busy, |v| {
                v.child(
                    gpui_kit::div()
                        .px_2()
                        .child(t!("designer.loading").to_string()),
                )
            })
            .when_some(self.error.clone(), |v, error| {
                v.child(gpui_kit::div().px_2().text_color(p.danger).child(error))
            })
            .when(self.needs_reload, |v| {
                v.child(
                    gpui_kit::div()
                        .px_2()
                        .text_color(p.danger)
                        .child(t!("designer.reload_required").to_string()),
                )
            })
            .when(self.needs_reload || !self.loaded, |v| {
                v.child(
                    Button::new("designer-reload")
                        .label(t!("designer.reload").to_string())
                        .disabled(self.busy)
                        .on_click(cx.listener(|this, _, w, cx| this.load(w, cx))),
                )
            })
            .child(match self.page {
                Page::Fields => h_flex()
                    .items_stretch()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(
                        v_flex()
                            .w(tokens::list::NAME_COLUMN_WIDTH.rems())
                            .min_h_0()
                            .border_r_1()
                            .border_color(p.separator)
                            .child(fields),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .p_3()
                            .gap_2()
                            .child(FormRow::new(
                                t!("designer.name").to_string(),
                                Input::new(&self.name)
                                    .id("designer-field-name")
                                    .disabled(disabled || selected.is_none()),
                            ))
                            .child(FormRow::new(
                                t!("designer.definition").to_string(),
                                Input::new(&self.definition)
                                    .id("designer-field-definition")
                                    .disabled(disabled || selected.is_none()),
                            ))
                            .child(
                                gpui_kit::div()
                                    .text_color(p.label_secondary)
                                    .child(t!("designer.definition_help").to_string()),
                            ),
                    )
                    .into_any_element(),
                Page::Constraints | Page::Indexes | Page::Options => {
                    let (field, key) = match self.page {
                        Page::Constraints => (&self.constraints, "constraints_help"),
                        Page::Indexes => (&self.indexes, "indexes_help"),
                        _ => (&self.options, "options_help"),
                    };
                    v_flex()
                        .flex_1()
                        .min_h_0()
                        .child(
                            gpui_kit::div()
                                .px_2()
                                .py_1()
                                .text_color(p.label_secondary)
                                .child(t!(format!("designer.{key}")).to_string()),
                        )
                        .child(Editor::new(field).disabled(disabled).size_full())
                        .into_any_element()
                }
                Page::Preview => Editor::new(&self.sql).size_full().into_any_element(),
            })
    }
}
