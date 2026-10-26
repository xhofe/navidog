//! Rendering of one result: the form view and the bottom bar.

use super::*;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Disableable as _, Selectable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, InteractiveElement as _, IntoElement, StatefulInteractiveElement as _,
    Styled as _, div,
};
use navidog_ui::assets::lucide;
use navidog_ui::skin::truncated;
use navidog_ui::{palette, tokens};
use rust_i18n::t;

impl ResultView {
    pub(crate) fn render_form(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let model = self.grid.read(cx).model(cx);
        let rows = model.row_count();
        let row = model.head().0.min(rows.saturating_sub(1));
        let fields: Vec<(SharedString, String)> = model
            .columns
            .iter()
            .enumerate()
            .map(|(c, col)| {
                let v = model
                    .value(row, c)
                    .cloned()
                    .flatten()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "(NULL)".into());
                (col.name.clone(), v)
            })
            .collect();
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .h(tokens::result::BOTTOM_BAR_HEIGHT.rems())
                    .px_2()
                    .gap_1()
                    .border_b_1()
                    .border_color(p.separator)
                    .child(
                        Button::new("form-prev")
                            .ghost()
                            .xsmall()
                            .icon(lucide("chevron-left"))
                            .disabled(row == 0)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.grid
                                    .update(cx, |g, cx| g.move_to_row(row.saturating_sub(1), cx));
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("form-next")
                            .ghost()
                            .xsmall()
                            .icon(lucide("chevron-right"))
                            .disabled(row + 1 >= rows)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.grid.update(cx, |g, cx| g.move_to_row(row + 1, cx));
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(p.label_secondary)
                            .child(format!("{} / {rows}", (row + 1).min(rows))),
                    ),
            )
            .child(
                v_flex()
                    .id("result-form")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_2()
                    .gap_1()
                    .children(fields.into_iter().enumerate().map(|(ix, (name, value))| {
                        h_flex()
                            .gap_2()
                            .min_w_0()
                            .child(
                                truncated(("form-field", ix), name)
                                    .w(tokens::list::DETAIL_COLUMN_WIDTH.rems())
                                    .flex_none()
                                    .text_color(p.label_secondary),
                            )
                            .child(truncated(("form-value", ix), value).flex_1())
                    })),
            )
            .into_any_element()
    }

    pub(crate) fn render_bottom_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let editable = self.session.edit.is_some() && !self.session.busy;
        let dirty = self.grid.read(cx).model(cx).has_changes();
        let icon_button = |id: &'static str, icon: &'static str, tip: String| {
            Button::new(id)
                .ghost()
                .xsmall()
                .icon(lucide(icon))
                .tooltip(tip)
                .w(tokens::result::BOTTOM_BUTTON.rems())
                .h(tokens::result::BOTTOM_BUTTON.rems())
        };
        let sql: SharedString = if self.session.status.is_empty() {
            self.session.sql.clone().into()
        } else {
            self.session.status.clone()
        };
        h_flex()
            .h(tokens::result::BOTTOM_BAR_HEIGHT.rems())
            .min_w_0()
            .flex_none()
            .px_2()
            .gap_1()
            .border_t_1()
            .border_color(p.separator)
            .text_xs()
            .child(
                icon_button(
                    "result-cell-editor",
                    "code",
                    t!("query.cell_editor").to_string(),
                )
                .disabled(true),
            )
            .child(
                icon_button(
                    "result-analysis",
                    "chart-pie",
                    t!("query.data_analysis").to_string(),
                )
                .disabled(true),
            )
            .child(
                icon_button("result-export", "file-text", t!("query.export").to_string())
                    .disabled(true),
            )
            .child(
                icon_button("result-pin", "pin", t!("query.pin").to_string())
                    .selected(self.pinned)
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ResultEvent::TogglePin))),
            )
            .child(
                truncated("result-sql", sql)
                    .flex_1()
                    .px_2()
                    .text_color(p.label_secondary),
            )
            .when(self.session.edit.is_none(), |bar| {
                bar.child(
                    div()
                        .flex_none()
                        .text_color(p.label_secondary)
                        .child(t!("query.read_only").to_string()),
                )
            })
            .child(
                icon_button("result-add", "plus", t!("query.add_row").to_string())
                    .disabled(!editable)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.grid.update(cx, |g, cx| {
                            let mut row = 0;
                            g.update_model(cx, |m| row = m.insert_row());
                            g.scroll_to_row(row, cx);
                            g.settle(cx);
                        });
                    })),
            )
            .child(
                icon_button("result-delete", "minus", t!("query.delete_row").to_string())
                    .disabled(!editable)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.grid
                            .update(cx, |g, cx| g.update_model(cx, |m| m.delete_selected_rows()));
                    })),
            )
            .child(
                icon_button("result-apply", "check", t!("query.apply").to_string())
                    .disabled(!editable || !dirty)
                    .on_click(cx.listener(|this, _, window, cx| this.commit(window, cx))),
            )
            .child(
                icon_button("result-discard", "x", t!("query.discard").to_string())
                    .disabled(!dirty || self.session.busy)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.grid
                            .update(cx, |g, cx| g.update_model(cx, |m| m.discard()));
                    })),
            )
    }
}
