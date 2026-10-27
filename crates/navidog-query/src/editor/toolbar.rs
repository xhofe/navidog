//! The editor toolbar: connection, database and schema pickers, run menu and buttons.

use super::*;

use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::component::{Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, ClickEvent, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, div,
};
use navidog_app_state::settings::{ResultsPosition, Settings};
use navidog_app_state::store::ConnectionStore;
use navidog_session::sessions::{ConnState, Sessions};
use navidog_ui::assets::lucide;
use navidog_ui::skin::truncated;
use navidog_ui::{palette, tokens};
use rust_i18n::t;

impl QueryEditor {
    fn connection_menu(&self, cx: &App) -> NativeMenu {
        let store = ConnectionStore::global(cx);
        store
            .read(cx)
            .connections()
            .iter()
            .fold(NativeMenu::new(), |menu, c| {
                menu.menu_with_check(
                    c.name.clone(),
                    self.target.connection.as_deref() == Some(c.id.as_str()),
                    Box::new(PickConnection {
                        id: c.id.clone().into(),
                    }),
                )
            })
    }

    fn database_menu(&self, cx: &App) -> NativeMenu {
        let Some(conn) = self.target.connection.as_deref() else {
            return NativeMenu::new();
        };
        let ConnState::Open { databases, .. } = Sessions::global(cx).read(cx).state(conn) else {
            return NativeMenu::new();
        };
        databases.iter().fold(NativeMenu::new(), |menu, db| {
            menu.menu_with_check(
                db.clone(),
                self.target.database.as_deref() == Some(db.as_str()),
                Box::new(PickDatabase {
                    name: db.clone().into(),
                }),
            )
        })
    }

    fn schema_menu(&self) -> NativeMenu {
        self.schemas.iter().fold(NativeMenu::new(), |menu, s| {
            menu.menu_with_check(
                s.clone(),
                self.target.schema.as_deref() == Some(s.as_str()),
                Box::new(PickSchema {
                    name: s.clone().into(),
                }),
            )
        })
    }

    fn run_menu(&self, cx: &App) -> NativeMenu {
        NativeMenu::new()
            .menu(t!("query.run").to_string(), Box::new(Run))
            .menu(t!("query.run_current").to_string(), Box::new(RunCurrent))
            .menu(t!("query.run_selected").to_string(), Box::new(RunSelected))
            .separator()
            .menu_with_check(
                t!("query.continue_on_error").to_string(),
                Settings::global(cx).continue_on_error,
                Box::new(ToggleContinueOnError),
            )
            .menu_with_check(
                t!("query.keep_results").to_string(),
                Settings::global(cx).keep_results,
                Box::new(ToggleKeepResults),
            )
    }

    pub(crate) fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
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
        let picker = |id: &'static str, icon: &'static str, label: String, empty: bool| {
            let mut glyph = lucide(icon);
            if let Some(color) = tokens::icon_color(icon, cx) {
                glyph = glyph.text_color(color);
            }
            h_flex()
                .id(id)
                .flex_none()
                .w(tokens::query::PICKER_WIDTH.rems())
                .h(tokens::info::SEARCH_FIELD_HEIGHT.rems())
                .px_1()
                .gap_1()
                .items_center()
                .rounded_md()
                .border_1()
                .border_color(p.separator)
                .bg(p.fill_tertiary)
                .cursor_pointer()
                .child(glyph.size(tokens::navigator::ROW_ICON.rems()))
                .child(
                    truncated(SharedString::from(format!("{id}-label")), label)
                        .flex_1()
                        .text_color(if empty { p.label_secondary } else { p.label }),
                )
                .child(
                    lucide("chevron-down")
                        .size(tokens::navigator::ROW_ICON.rems())
                        .text_color(p.label_secondary),
                )
        };
        let store = ConnectionStore::global(cx);
        let conn_name = self
            .target
            .connection
            .as_ref()
            .and_then(|id| store.read(cx).get(id))
            .map(|c| c.name.clone());
        let pg = self.dialect(cx).is_some_and(|d| d.features().has_schemas);
        let selected = self.has_selection(cx);
        let show_results = Settings::global(cx).results_position == ResultsPosition::NewPage
            && !self.results_page
            && self.results.has_run();
        v_flex()
            .child(
                h_flex()
                    .h(tokens::query::TOOLBAR_HEIGHT.rems())
                    .px_2()
                    .gap_1()
                    .min_w_0()
                    .items_center()
                    .overflow_hidden()
                    .border_b_1()
                    .border_color(p.separator)
                    .child(
                        button("query-save", "save", t!("query.save").to_string()).on_click(
                            cx.listener(|this, _, window, cx| this.save_query(false, window, cx)),
                        ),
                    )
                    .child(separator(cx))
                    .child(
                        button(
                            "query-beautify",
                            "wand-sparkles",
                            t!("query.beautify").to_string(),
                        )
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(crate::format::Beautify), cx)
                        }),
                    )
                    .child(
                        button(
                            "query-snippets",
                            "scroll-text",
                            t!("query.snippets").to_string(),
                        )
                        .on_click(|_, window, cx| {
                            window.dispatch_action(
                                Box::new(navidog_app_state::actions::ShowSnippets),
                                cx,
                            )
                        }),
                    )
                    .child(
                        button("query-builder", "blocks", t!("query.builder").to_string())
                            .disabled(true),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("query-focus-mode")
                            .ghost()
                            .small()
                            .icon(lucide("scan"))
                            .tooltip(t!("query.focus_mode").to_string())
                            .on_click(|_, window, cx| {
                                window.dispatch_action(
                                    Box::new(navidog_app_state::actions::ToggleFocusMode),
                                    cx,
                                )
                            }),
                    ),
            )
            .child(
                h_flex()
                    .h(tokens::query::TOOLBAR_HEIGHT.rems())
                    .px_2()
                    .gap_1()
                    .min_w_0()
                    .items_center()
                    .overflow_hidden()
                    .border_b_1()
                    .border_color(p.separator)
                    .child(
                        picker(
                            "query-connection",
                            "plug",
                            conn_name
                                .clone()
                                .unwrap_or_else(|| t!("query.connection").to_string()),
                            conn_name.is_none(),
                        )
                        .on_click(cx.listener(
                            |this, e: &ClickEvent, window, cx| {
                                this.focus_for_menu(window, cx);
                                this.connection_menu(cx).show(e.position(), window, cx);
                            },
                        )),
                    )
                    .child(
                        picker(
                            "query-database",
                            "database",
                            self.target
                                .database
                                .clone()
                                .unwrap_or_else(|| t!("query.database").to_string()),
                            self.target.database.is_none(),
                        )
                        .when(self.target.connection.is_none(), |d| d.opacity(0.4))
                        .on_click(cx.listener(
                            |this, e: &ClickEvent, window, cx| {
                                if this.target.connection.is_none() {
                                    return;
                                }
                                this.focus_for_menu(window, cx);
                                this.database_menu(cx).show(e.position(), window, cx);
                            },
                        )),
                    )
                    .when(pg, |bar| {
                        bar.child(
                            picker(
                                "query-schema",
                                "layers",
                                self.target
                                    .schema
                                    .clone()
                                    .unwrap_or_else(|| t!("query.schema").to_string()),
                                self.target.schema.is_none(),
                            )
                            .when(self.schemas.is_empty(), |d| d.opacity(0.4))
                            .on_click(cx.listener(
                                |this, e: &ClickEvent, window, cx| {
                                    if this.schemas.is_empty() {
                                        return;
                                    }
                                    this.focus_for_menu(window, cx);
                                    this.schema_menu().show(e.position(), window, cx);
                                },
                            )),
                        )
                    })
                    .child(
                        button("query-run", "play", t!("query.run").to_string())
                            .disabled(self.running)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.run(RunKind::All, window, cx)
                            })),
                    )
                    .child(
                        Button::new("query-run-menu")
                            .ghost()
                            .xsmall()
                            .icon(lucide("chevron-down"))
                            .disabled(self.running)
                            .on_click(cx.listener(|this, e: &ClickEvent, window, cx| {
                                this.focus_for_menu(window, cx);
                                this.run_menu(cx).show(e.position(), window, cx);
                            })),
                    )
                    .child(
                        Checkbox::new("query-continue")
                            .label(t!("query.continue_on_error").to_string())
                            .checked(Settings::global(cx).continue_on_error)
                            .on_click(|checked: &bool, _, cx| {
                                let on = *checked;
                                Settings::update(cx, |s| s.continue_on_error = on);
                            }),
                    )
                    .child(
                        button("query-stop", "circle-stop", t!("query.stop").to_string())
                            .disabled(!self.running)
                            .on_click(cx.listener(|this, _, _, cx| this.stop(cx))),
                    )
                    .child(
                        button(
                            "query-explain",
                            "list-tree",
                            if selected {
                                t!("query.explain_selected").to_string()
                            } else {
                                t!("query.explain").to_string()
                            },
                        )
                        .disabled(self.running)
                        .on_click(
                            cx.listener(|this, _, window, cx| {
                                this.run(RunKind::Explain, window, cx)
                            }),
                        ),
                    )
                    .when(show_results, |bar| {
                        bar.child(
                            button(
                                "query-show-results",
                                "square",
                                t!("query.show_results").to_string(),
                            )
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(ShowResults), cx)
                            }),
                        )
                    }),
            )
    }
}

fn separator(cx: &App) -> impl IntoElement {
    div()
        .w(tokens::pane::DIVIDER.rems())
        .h(tokens::navigator::ROW_ICON.rems())
        .mx_1()
        .bg(palette(cx).separator)
}
