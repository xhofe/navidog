//! The results area of the query editor: the tab strip, the pages and the position buttons.

use super::*;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::Editor;
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::component::{Selectable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, div,
};
use navidog_app_state::settings::{ResultsPosition, Settings};
use navidog_ui::assets::lucide;
use navidog_ui::skin::truncated;
use navidog_ui::{palette, tokens};
use rust_i18n::t;

impl QueryEditor {
    fn tab_button(
        &self,
        id: SharedString,
        label: String,
        page: Page,
        pinned: bool,
        close: Option<u64>,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        let p = palette(cx);
        let active = self.results.page == page;
        div()
            .id(id.clone())
            .flex_none()
            .h_full()
            .px_3()
            .flex()
            .items_center()
            .gap_1()
            .cursor_pointer()
            .border_b_2()
            .border_color(if active {
                p.accent
            } else {
                gpui_kit::transparent_black()
            })
            .text_color(if active { p.label } else { p.label_secondary })
            .when(pinned, |d| {
                d.child(lucide("pin").size(tokens::navigator::ROW_ICON.rems()))
            })
            .child(
                truncated(SharedString::from(format!("{id}-label")), label)
                    .max_w(tokens::editor_tab::TITLE_MAX_WIDTH.rems()),
            )
            .when_some(close, |d, tab_id| {
                d.child(
                    Button::new(SharedString::from(format!("result-close-{tab_id}")))
                        .ghost()
                        .xsmall()
                        .icon(lucide("x"))
                        .tooltip(t!("query.close").to_string())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.results.close_tab(tab_id, cx);
                        })),
                )
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.results.page = page;
                cx.notify();
            }))
    }

    pub(crate) fn render_results(
        &mut self,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let p = palette(cx);
        if !self.results.page_exists() {
            self.results.page = Page::Messages;
        }
        let mut strip: Vec<AnyElement> = Vec::new();
        let tab_el = |this: &Self, t: &Tab, cx: &mut Context<Self>| {
            let id = t.id;
            let lines = t.info.lines(&t.sql);
            let pinned = t.pinned;
            let close = Settings::global(cx).keep_results.then_some(id);
            this.tab_button(
                SharedString::from(format!("result-tab-{id}")),
                t.name.clone(),
                Page::Tab(id),
                pinned,
                close,
                cx,
            )
            .when(pinned, |d| {
                d.tooltip(move |window, cx| {
                    let text = lines
                        .iter()
                        .map(|(k, v)| format!("{k}: {v}"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    gpui_kit::component::tooltip::Tooltip::new(text).build(window, cx)
                })
            })
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, e: &gpui_kit::MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.focus_for_menu(window, cx);
                    let pinned = this.results.tabs.iter().any(|t| t.id == id && t.pinned);
                    NativeMenu::new()
                        .menu(
                            if pinned {
                                t!("query.unpin").to_string()
                            } else {
                                t!("query.pin").to_string()
                            },
                            Box::new(TogglePinTab { id }),
                        )
                        .menu(
                            t!("query.close").to_string(),
                            Box::new(CloseResultTab { id }),
                        )
                        .separator()
                        .menu(t!("query.unpin_all").to_string(), Box::new(UnpinAllTabs))
                        .menu(
                            t!("query.close_all_results").to_string(),
                            Box::new(CloseAllResults),
                        )
                        .show(e.position, window, cx);
                }),
            )
            .into_any_element()
        };
        let tabs: Vec<(bool, usize)> = self
            .results
            .tabs
            .iter()
            .enumerate()
            .map(|(i, t)| (t.pinned, i))
            .collect();
        for (pinned, i) in &tabs {
            if *pinned {
                let t = &self.results.tabs[*i];
                strip.push(tab_el(self, t, cx));
            }
        }
        strip.push(
            self.tab_button(
                "result-messages".into(),
                t!("query.messages").to_string(),
                Page::Messages,
                false,
                None,
                cx,
            )
            .into_any_element(),
        );
        strip.push(
            self.tab_button(
                "result-summary".into(),
                t!("query.summary").to_string(),
                Page::Summary,
                false,
                None,
                cx,
            )
            .into_any_element(),
        );
        for (pinned, i) in &tabs {
            if !*pinned {
                let t = &self.results.tabs[*i];
                strip.push(tab_el(self, t, cx));
            }
        }
        if self.results.profile.is_some() {
            strip.push(
                self.tab_button(
                    "result-profile".into(),
                    t!("query.profile").to_string(),
                    Page::Profile,
                    false,
                    None,
                    cx,
                )
                .into_any_element(),
            );
            strip.push(
                self.tab_button(
                    "result-status".into(),
                    t!("query.status").to_string(),
                    Page::Status,
                    false,
                    None,
                    cx,
                )
                .into_any_element(),
            );
        }
        let position = Settings::global(cx).results_position;
        let pos_button = |id: &'static str, icon: &'static str, tip: String, on: bool| {
            Button::new(id)
                .ghost()
                .xsmall()
                .icon(lucide(icon))
                .tooltip(tip)
                .selected(on)
        };
        let body: AnyElement = match self.results.page {
            Page::Messages => div()
                .size_full()
                .child(
                    Editor::new(&self.results.messages)
                        .bordered(false)
                        .size_full(),
                )
                .into_any_element(),
            Page::Summary => self.render_summary(cx).into_any_element(),
            Page::Tab(id) => match self
                .results
                .tabs
                .iter()
                .find(|t| t.id == id)
                .map(|t| &t.body)
            {
                Some(Body::Grid(view)) => view.clone().into_any_element(),
                Some(Body::Explain(view)) => view.clone().into_any_element(),
                None => div().into_any_element(),
            },
            Page::Profile => self
                .results
                .profile
                .clone()
                .map(|g| g.into_any_element())
                .unwrap_or_else(|| div().into_any_element()),
            Page::Status => self
                .results
                .status
                .clone()
                .map(|g| g.into_any_element())
                .unwrap_or_else(|| div().into_any_element()),
        };
        v_flex()
            .id("query-results")
            .size_full()
            .min_h_0()
            .child(
                h_flex()
                    .h(tokens::result::TAB_HEIGHT.rems())
                    .border_b_1()
                    .border_t_1()
                    .border_color(p.separator)
                    .child(
                        div()
                            .id("result-strip")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .overflow_x_scroll()
                            .children(strip),
                    )
                    .child(
                        h_flex()
                            .flex_none()
                            .px_1()
                            .gap_0p5()
                            .when(position == ResultsPosition::NewPage, |r| {
                                r.child(
                                    pos_button(
                                        "result-back",
                                        "pencil",
                                        t!("query.back_to_editor").to_string(),
                                        false,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, window, cx| this.back_to_editor(window, cx),
                                    )),
                                )
                            })
                            .child(
                                pos_button(
                                    "result-below",
                                    "panel-bottom",
                                    t!("query.results_below").to_string(),
                                    position == ResultsPosition::Below,
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.place_results(ResultsPosition::Below, window, cx)
                                    },
                                )),
                            )
                            .child(
                                pos_button(
                                    "result-beside",
                                    "panel-right",
                                    t!("query.results_beside").to_string(),
                                    position == ResultsPosition::Beside,
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.place_results(ResultsPosition::Beside, window, cx)
                                    },
                                )),
                            )
                            .child(
                                pos_button(
                                    "result-new-page",
                                    "square",
                                    t!("query.results_new_page").to_string(),
                                    position == ResultsPosition::NewPage,
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.place_results(ResultsPosition::NewPage, window, cx)
                                    },
                                )),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_hidden()
                    .child(body),
            )
    }

    fn render_summary(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let r = &self.results;
        let none = || t!("query.none").to_string();
        let errors = r.rows.iter().filter(|x| x.error).count();
        let fmt = |t: &Option<chrono::DateTime<chrono::Local>>| {
            t.map(|t| t.format("%H:%M:%S%.3f").to_string())
                .unwrap_or_else(none)
        };
        let ran = r.started.is_some();
        let elapsed = match (r.started, r.ended) {
            (Some(a), Some(b)) => secs((b - a).to_std().unwrap_or_default()),
            _ => none(),
        };
        let stat = |label: String, value: String| {
            h_flex()
                .gap_2()
                .child(
                    div()
                        .text_color(p.label_secondary)
                        .child(format!("{label}:")),
                )
                .child(div().child(value))
        };
        let count = |n: usize| if ran { n.to_string() } else { none() };
        let rows: Vec<(usize, &SummaryRow)> = r
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| !r.only_errors || row.error)
            .collect();
        let header_cell = |text: String| {
            div()
                .px_2()
                .truncate()
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .child(text)
        };
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .h(tokens::result::SUMMARY_HEIGHT.rems())
                    .px_4()
                    .gap_8()
                    .border_b_1()
                    .border_color(p.separator)
                    .child(
                        v_flex()
                            .gap_1()
                            .child(stat(t!("query.processed").to_string(), count(r.rows.len())))
                            .child(stat(
                                t!("query.succeeded").to_string(),
                                count(r.rows.len() - errors),
                            ))
                            .child(stat(t!("query.errors").to_string(), count(errors))),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(stat(t!("query.start_time").to_string(), fmt(&r.started)))
                            .child(stat(t!("query.end_time").to_string(), fmt(&r.ended)))
                            .child(stat(t!("query.elapsed").to_string(), elapsed)),
                    )
                    .child(div().flex_1())
                    .child(
                        Checkbox::new("summary-only-errors")
                            .label(t!("query.only_errors").to_string())
                            .checked(r.only_errors)
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                this.results.only_errors = *checked;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                h_flex()
                    .h(tokens::list::HEADER_HEIGHT.rems())
                    .border_b_1()
                    .border_color(p.separator)
                    .child(header_cell(t!("query.query").to_string()).flex_1())
                    .child(header_cell(t!("query.message").to_string()).flex_1())
                    .child(
                        header_cell(t!("query.elapsed").to_string())
                            .w(tokens::list::DETAIL_COLUMN_WIDTH.rems()),
                    ),
            )
            .child(
                v_flex()
                    .id("summary-rows")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows.into_iter().map(|(ix, row)| {
                        let range = row.range.clone();
                        h_flex()
                            .id(("summary-row", ix))
                            .h(tokens::list::ROW_HEIGHT.rems())
                            .when(ix % 2 == 1, |d| d.bg(p.row_alt))
                            .when(row.error, |d| d.text_color(p.danger))
                            .child(
                                truncated(("summary-sql", ix), row.sql.replace('\n', " "))
                                    .flex_1()
                                    .px_2(),
                            )
                            .child(
                                truncated(("summary-msg", ix), row.message.clone())
                                    .flex_1()
                                    .px_2(),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .w(tokens::list::DETAIL_COLUMN_WIDTH.rems())
                                    .child(secs(row.elapsed)),
                            )
                            .on_click(cx.listener(
                                move |this, e: &gpui_kit::ClickEvent, window, cx| {
                                    if e.click_count() == 2 {
                                        this.select_range(range.clone(), window, cx);
                                    }
                                },
                            ))
                    })),
            )
    }
}
