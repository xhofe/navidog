//! Filter and sort pane (F-DATA-006): the visual builder, text mode and quick filters.

use super::*;

impl TableViewer {
    pub(super) fn dialect(&self) -> Option<&dyn Dialect> {
        self.dialect.as_deref()
    }

    pub(super) fn field_names(&self, cx: &App) -> Vec<String> {
        let model = self.view.grid.read(cx).model(cx);
        if model.all_columns().next().is_none() {
            self.columns.iter().map(|c| c.name.clone()).collect()
        } else {
            model.all_columns().map(|c| c.name.to_string()).collect()
        }
    }

    pub(super) fn make_draft(rule: Rule, window: &mut Window, cx: &mut Context<Self>) -> Draft {
        let value = cx.new(|cx| InputState::new(window, cx).default_value(rule.value));
        let value2 = cx.new(|cx| InputState::new(window, cx).default_value(rule.value2));
        let on_enter = |this: &mut Self,
                        _: &Entity<InputState>,
                        e: &InputEvent,
                        window: &mut Window,
                        cx: &mut Context<Self>| {
            if matches!(e, InputEvent::PressEnter { .. }) {
                this.apply_filter(window, cx);
            }
        };
        let _subscriptions = vec![
            cx.subscribe_in(&value, window, on_enter),
            cx.subscribe_in(&value2, window, on_enter),
        ];
        Draft {
            field: rule.field,
            op: rule.op,
            value,
            value2,
            or_join: rule.or,
            negate: rule.negate,
            _subscriptions,
        }
    }

    pub(super) fn current_filter(&self, cx: &App) -> Filter {
        Filter {
            rules: self
                .view
                .rules
                .iter()
                .map(|d| Rule {
                    field: d.field.clone(),
                    op: d.op,
                    value: d.value.read(cx).value().to_string(),
                    value2: d.value2.read(cx).value().to_string(),
                    or: d.or_join,
                    negate: d.negate,
                })
                .collect(),
            grouped: self.view.grouped,
            group_negate: self.view.group_negate,
        }
    }

    pub(super) fn write_filter_sql(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialect) = self.dialect() else {
            return;
        };
        let where_text = filter::where_sql(&self.current_filter(cx), dialect);
        let order_text = filter::order_sql(&self.view.sorts, dialect);
        self.view
            .filter
            .update(cx, |s, cx| s.set_value(where_text, window, cx));
        self.view
            .order
            .update(cx, |s, cx| s.set_value(order_text, window, cx));
    }

    pub(super) fn apply_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.view.builder {
            self.write_filter_sql(window, cx);
        }
        self.page = 0;
        self.reload_committed(true, window, cx);
    }

    pub(super) fn set_builder(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        if on == self.view.builder {
            return;
        }
        let Some(dialect) = self.dialect() else {
            return;
        };
        if on {
            let where_text = self.view.filter.read(cx).value().to_string();
            let order_text = self.view.order.read(cx).value().to_string();
            let (Some(parsed), Some(sorts)) = (
                filter::parse_where(&where_text, dialect),
                filter::parse_order(&order_text, dialect),
            ) else {
                return;
            };
            let mut rules = Vec::new();
            for rule in parsed.rules {
                rules.push(Self::make_draft(rule, window, cx));
            }
            self.view.rules = rules;
            self.view.sorts = sorts;
            self.view.grouped = parsed.grouped;
            self.view.group_negate = parsed.group_negate;
        } else {
            self.write_filter_sql(window, cx);
        }
        self.view.builder = on;
        cx.notify();
    }

    pub(super) fn quick_filter(
        &mut self,
        column: &str,
        op: filter::Op,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.view.show_filter = true;
        let Some(dialect) = self.dialect() else {
            return;
        };
        let rule = Rule {
            field: column.to_string(),
            op,
            value: value.to_string(),
            value2: String::new(),
            or: false,
            negate: false,
        };
        if self.view.builder {
            self.view.rules.push(Self::make_draft(rule, window, cx));
            self.write_filter_sql(window, cx);
        } else {
            let extra = filter::where_sql(
                &Filter {
                    rules: vec![rule],
                    ..Filter::default()
                },
                dialect,
            );
            let current = self.view.filter.read(cx).value().to_string();
            let text = if current.trim().is_empty() {
                extra
            } else {
                format!("({current}) AND {extra}")
            };
            self.view
                .filter
                .update(cx, |s, cx| s.set_value(text, window, cx));
        }
        self.page = 0;
        self.reload_committed(true, window, cx);
    }

    pub(super) fn add_rule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let field = self.field_names(cx).into_iter().next().unwrap_or_default();
        self.view.rules.push(Self::make_draft(
            Rule {
                field,
                op: filter::Op::Eq,
                value: String::new(),
                value2: String::new(),
                or: false,
                negate: false,
            },
            window,
            cx,
        ));
        cx.notify();
    }

    pub(super) fn render_filter(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let mut pane = v_flex()
            .id("filter-pane")
            .w_full()
            .min_w_0()
            .max_h(tokens::filter::BUILDER_MAX_HEIGHT.rems())
            .overflow_y_scroll()
            .border_b_1()
            .border_color(p.separator)
            .child(self.render_filter_bar(cx));
        if self.view.builder {
            for i in 0..self.view.rules.len() {
                pane = pane.child(self.render_rule(i, cx));
            }
            pane = pane.child(self.render_rule_actions(cx));
            for i in 0..self.view.sorts.len() {
                pane = pane.child(self.render_sort(i, cx));
            }
            pane = pane.child(self.render_sort_actions(cx));
        } else {
            pane = pane.child(self.render_filter_text(cx));
        }
        pane
    }

    pub(super) fn render_filter_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mode = |id: &'static str, on: bool, label: String| {
            Button::new(id).xsmall().flex_1().selected(on).label(label)
        };
        h_flex()
            .h(tokens::viewer::FILTER_HEIGHT.rems())
            .px_2()
            .gap_2()
            .child(
                h_flex()
                    .w(tokens::filter::SEGMENTED_WIDTH.rems())
                    .gap_1()
                    .child(
                        mode(
                            "filter-builder",
                            self.view.builder,
                            t!("viewer.builder").to_string(),
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| this.set_builder(true, window, cx)),
                        ),
                    )
                    .child(
                        mode(
                            "filter-text",
                            !self.view.builder,
                            t!("viewer.text").to_string(),
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| this.set_builder(false, window, cx)),
                        ),
                    ),
            )
            .when(self.view.builder, |bar| {
                bar.child(
                    Button::new("filter-group")
                        .xsmall()
                        .selected(self.view.grouped)
                        .disabled(self.view.rules.len() < 2)
                        .label(t!("viewer.group").to_string())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.view.grouped = !this.view.grouped;
                            if !this.view.grouped {
                                this.view.group_negate = false;
                            }
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("filter-group-not")
                        .xsmall()
                        .selected(self.view.group_negate)
                        .disabled(!self.view.grouped)
                        .label(t!("viewer.negate").to_string())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.view.group_negate = !this.view.group_negate;
                            cx.notify();
                        })),
                )
            })
            .child(div().flex_1())
            .child(
                Button::new("apply-filter")
                    .xsmall()
                    .label(t!("viewer.apply_filter").to_string())
                    .on_click(cx.listener(|this, _, window, cx| this.apply_filter(window, cx))),
            )
    }

    pub(super) fn render_filter_text(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let label = |s: &'static str| div().text_color(p.label_secondary).child(s);
        h_flex()
            .h(tokens::viewer::FILTER_HEIGHT.rems())
            .items_center()
            .px_2()
            .gap_2()
            .overflow_hidden()
            .child(label("WHERE"))
            .child(
                navidog_ui::skin::single_line_sql(&self.view.filter)
                    .flex_1()
                    .min_w_0(),
            )
            .child(label("ORDER BY"))
            .child(
                navidog_ui::skin::single_line_sql(&self.view.order)
                    .flex_1()
                    .min_w_0(),
            )
    }

    pub(super) fn render_rule(&self, index: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let rule = &self.view.rules[index];
        let field = if rule.field.is_empty() {
            "…".to_string()
        } else {
            rule.field.clone()
        };
        h_flex()
            .h(tokens::viewer::FILTER_HEIGHT.rems())
            .items_center()
            .px_2()
            .gap_1()
            .min_w_0()
            .when(index > 0, |row| {
                let or_join = rule.or_join;
                row.child(
                    Button::new(ElementId::Name(format!("filter-join-{index}").into()))
                        .xsmall()
                        .label(if or_join {
                            t!("viewer.or").to_string()
                        } else {
                            t!("viewer.and").to_string()
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(rule) = this.view.rules.get_mut(index) {
                                rule.or_join = !rule.or_join;
                                cx.notify();
                            }
                        })),
                )
            })
            .child(
                Button::new(ElementId::Name(format!("filter-not-{index}").into()))
                    .xsmall()
                    .selected(rule.negate)
                    .label(t!("viewer.negate").to_string())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(rule) = this.view.rules.get_mut(index) {
                            rule.negate = !rule.negate;
                            cx.notify();
                        }
                    })),
            )
            .child(
                Button::new(ElementId::Name(format!("filter-field-{index}").into()))
                    .xsmall()
                    .label(field)
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        this.focus_for_menu(window, cx);
                        this.field_menu(index, cx)
                            .show(event.position(), window, cx);
                    })),
            )
            .child(
                Button::new(ElementId::Name(format!("filter-op-{index}").into()))
                    .xsmall()
                    .label(rule.op.label())
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        this.focus_for_menu(window, cx);
                        this.op_menu(index).show(event.position(), window, cx);
                    })),
            )
            .when(rule.op.needs_value(), |row| {
                row.child(Input::new(&rule.value).small().flex_1().min_w_0())
            })
            .when(rule.op.needs_second(), |row| {
                row.child(Input::new(&rule.value2).small().flex_1().min_w_0())
            })
            .child(
                Button::new(ElementId::Name(format!("filter-remove-{index}").into()))
                    .ghost()
                    .xsmall()
                    .icon(lucide("x"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if index < this.view.rules.len() {
                            this.view.rules.remove(index);
                            cx.notify();
                        }
                    })),
            )
    }

    pub(super) fn render_rule_actions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex().px_2().gap_1().child(
            Button::new("filter-add-rule")
                .ghost()
                .xsmall()
                .icon(lucide("plus"))
                .label(t!("viewer.add_rule").to_string())
                .on_click(cx.listener(|this, _, window, cx| this.add_rule(window, cx))),
        )
    }

    pub(super) fn render_sort(&self, index: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let sort = &self.view.sorts[index];
        let field = if sort.field.is_empty() {
            "…".to_string()
        } else {
            sort.field.clone()
        };
        h_flex()
            .h(tokens::viewer::FILTER_HEIGHT.rems())
            .px_2()
            .gap_1()
            .child(
                Button::new(ElementId::Name(format!("sort-field-{index}").into()))
                    .xsmall()
                    .label(field)
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        this.focus_for_menu(window, cx);
                        this.sort_menu(index, cx).show(event.position(), window, cx);
                    })),
            )
            .child(
                Button::new(ElementId::Name(format!("sort-dir-{index}").into()))
                    .xsmall()
                    .label(if sort.asc {
                        t!("viewer.asc").to_string()
                    } else {
                        t!("viewer.desc").to_string()
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(sort) = this.view.sorts.get_mut(index) {
                            sort.asc = !sort.asc;
                            cx.notify();
                        }
                    })),
            )
            .child(
                Button::new(ElementId::Name(format!("sort-up-{index}").into()))
                    .ghost()
                    .xsmall()
                    .disabled(index == 0)
                    .label(t!("viewer.sort_up").to_string())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if index > 0 && index < this.view.sorts.len() {
                            this.view.sorts.swap(index, index - 1);
                            cx.notify();
                        }
                    })),
            )
            .child(
                Button::new(ElementId::Name(format!("sort-down-{index}").into()))
                    .ghost()
                    .xsmall()
                    .disabled(index + 1 >= self.view.sorts.len())
                    .label(t!("viewer.sort_down").to_string())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if index + 1 < this.view.sorts.len() {
                            this.view.sorts.swap(index, index + 1);
                            cx.notify();
                        }
                    })),
            )
            .child(
                Button::new(ElementId::Name(format!("sort-remove-{index}").into()))
                    .ghost()
                    .xsmall()
                    .icon(lucide("x"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if index < this.view.sorts.len() {
                            this.view.sorts.remove(index);
                            cx.notify();
                        }
                    })),
            )
    }

    pub(super) fn render_sort_actions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex().px_2().pb_1().gap_1().child(
            Button::new("filter-add-sort")
                .ghost()
                .xsmall()
                .icon(lucide("plus"))
                .label(t!("viewer.add_sort").to_string())
                .on_click(cx.listener(|this, _, _, cx| {
                    let field = this.field_names(cx).into_iter().next().unwrap_or_default();
                    this.view.sorts.push(Sort { field, asc: true });
                    cx.notify();
                })),
        )
    }

    pub(super) fn field_menu(&self, index: usize, cx: &App) -> NativeMenu {
        self.field_names(cx)
            .into_iter()
            .fold(NativeMenu::new(), |menu, name| {
                menu.menu(
                    name.clone(),
                    Box::new(PickField {
                        index,
                        name: name.into(),
                    }),
                )
            })
    }

    pub(super) fn op_menu(&self, index: usize) -> NativeMenu {
        let Some(dialect) = self.dialect() else {
            return NativeMenu::new();
        };
        filter::Op::choices(dialect)
            .into_iter()
            .fold(NativeMenu::new(), |menu, op| {
                menu.menu(
                    op.label(),
                    Box::new(PickOp {
                        index,
                        op: op.key().into(),
                    }),
                )
            })
    }

    pub(super) fn sort_menu(&self, index: usize, cx: &App) -> NativeMenu {
        self.field_names(cx)
            .into_iter()
            .fold(NativeMenu::new(), |menu, name| {
                menu.menu(
                    name.clone(),
                    Box::new(PickSort {
                        index,
                        name: name.into(),
                    }),
                )
            })
    }
}
