//! Completion for the WHERE and ORDER BY editors (21-table-viewer.md §6).

use std::ops::Range;
use std::rc::Rc;

use anyhow::Result;
use gpui_kit::component::input::{CompletionProvider, Rope, RopeExt as _};
use gpui_kit::{App, Task, WeakEntity, Window};
use lsp_types::{
    CompletionContext, CompletionItem, CompletionResponse, CompletionTextEdit, TextEdit,
};
use navidog_app_state::settings::Settings;
use navidog_session::outline::OutlineCache;
use navidog_sql::completion::{self, Catalog, Hit, Kind, OwnedCatalog, OwnedTable, Sources, Table};
use rust_i18n::t;

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LineEnter {
    /// The menu is closed: apply the filter and do not insert a newline.
    Apply,
    Accept,
    Dismiss,
}

pub(crate) fn line_enter(menu_open: bool, accepts: bool) -> LineEnter {
    if !menu_open {
        LineEnter::Apply
    } else if accepts {
        LineEnter::Accept
    } else {
        LineEnter::Dismiss
    }
}

fn detail(hit: &Hit) -> String {
    match hit.kind {
        Kind::Column => format!("{} · {}", t!("viewer.kind_column"), hit.type_name),
        Kind::Table => t!("viewer.kind_table").to_string(),
        Kind::View => t!("viewer.kind_view").to_string(),
        Kind::Schema => t!("viewer.kind_schema").to_string(),
        Kind::Database => t!("viewer.kind_database").to_string(),
        Kind::Function => t!("viewer.kind_function").to_string(),
        Kind::Builtin => t!("viewer.kind_builtin").to_string(),
        Kind::Keyword => t!("viewer.kind_keyword").to_string(),
        Kind::Snippet | Kind::Type => hit.label.clone(),
    }
}

pub(crate) fn highlight_label(query: &str, item: &CompletionItem) -> Vec<Range<usize>> {
    completion::highlight(query, &item.label)
}

pub(crate) fn attach(editor: &Entity<EditorState>, viewer: WeakEntity<TableViewer>, cx: &mut App) {
    let provider = Rc::new(FilterProvider { viewer });
    editor.update(cx, |e, _| {
        e.lsp_mut().completion_provider = Some(provider);
        e.lsp_mut()
            .set_completion_highlighter(Rc::new(highlight_label));
        // raw-px: room for "column · type" labels, same as the query editor
        e.lsp_mut().completion_menu.max_width = px(420.);
    });
}

struct FilterProvider {
    viewer: WeakEntity<TableViewer>,
}

impl CompletionProvider for FilterProvider {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        _: CompletionContext,
        _: &mut Window,
        cx: &mut App,
    ) -> Task<Result<CompletionResponse>> {
        let Some(viewer) = self.viewer.upgrade() else {
            return Task::ready(Ok(CompletionResponse::Array(Vec::new())));
        };
        let viewer = viewer.read(cx);
        let Some(dialect) = viewer.dialect.as_deref() else {
            return Task::ready(Ok(CompletionResponse::Array(Vec::new())));
        };
        let scope = viewer.object.scope.clone();
        let cache = OutlineCache::global(cx);
        let cache = cache.read(cx);
        let outline = cache.ready(&scope);
        let tables: Vec<Table> = outline
            .as_ref()
            .map(|o| {
                o.tables
                    .iter()
                    .map(|t| Table {
                        name: &t.name,
                        view: t.view,
                        columns: &t.columns,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let catalog = outline.as_ref().map(|o| Catalog {
            schemas: &o.schemas,
            tables: &tables,
            functions: &o.functions,
        });
        let others = |name: &str| -> Option<OwnedCatalog> {
            let other = if dialect.features().has_schemas {
                navidog_model::object::Scope::new(
                    scope.connection.clone(),
                    scope.database.clone(),
                    Some(name.into()),
                )
            } else {
                navidog_model::object::Scope::new(scope.connection.clone(), name, None)
            };
            cache.ready(&other).map(|o| OwnedCatalog {
                schemas: o.schemas.clone(),
                tables: o
                    .tables
                    .iter()
                    .map(|t| OwnedTable {
                        name: t.name.clone(),
                        view: t.view,
                        columns: t.columns.clone(),
                    })
                    .collect(),
                functions: o.functions.clone(),
            })
        };
        let types: std::collections::HashMap<&str, &str> = viewer
            .columns
            .iter()
            .map(|c| (c.name.as_str(), c.type_name.as_str()))
            .collect();
        let in_scope: Vec<(String, String)> = viewer
            .field_names(cx)
            .into_iter()
            .map(|name| {
                let ty = types.get(name.as_str()).copied().unwrap_or("").to_string();
                (name, ty)
            })
            .collect();
        let current = if dialect.features().has_schemas {
            scope.schema.clone()
        } else {
            Some(scope.database.clone()).filter(|s| !s.is_empty())
        };
        let from = offset.saturating_sub(completion::WINDOW);
        let from = (from..=offset)
            .find(|i| text.is_char_boundary(*i))
            .unwrap_or(offset);
        let to = (offset + 4096).min(text.len());
        let to = (to..=text.len())
            .find(|i| text.is_char_boundary(*i))
            .unwrap_or(text.len());
        let window_text = text.slice(from..to).to_string();
        let (range, list) = completion::complete(
            &window_text,
            offset - from,
            &Sources {
                dialect,
                current: current.as_deref(),
                catalog,
                others: &others,
                snippets: &[],
                in_scope: &in_scope,
                extras: &[],
                keywords: navidog_sql::completion::Keywords::Clause,
            },
        );
        let lsp_range = lsp_types::Range {
            start: text.offset_to_position(range.start + from),
            end: text.offset_to_position(range.end + from),
        };
        let items = list
            .into_iter()
            .take(200)
            .map(|c| {
                let detail = detail(&c);
                CompletionItem {
                    label: c.label,
                    detail: Some(detail),
                    text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                        range: lsp_range,
                        new_text: c.insert,
                    })),
                    ..CompletionItem::default()
                }
            })
            .collect();
        Task::ready(Ok(CompletionResponse::Array(items)))
    }

    fn is_completion_trigger(&self, _: usize, new_text: &str, cx: &mut App) -> bool {
        Settings::global(cx).code_completion && completion::is_trigger(new_text)
    }
}

impl TableViewer {
    fn focused_sql_line(&self, window: &Window, cx: &App) -> Option<bool> {
        if self
            .view
            .filter
            .read(cx)
            .focus_handle(cx)
            .is_focused(window)
        {
            Some(true)
        } else if self.view.order.read(cx).focus_handle(cx).is_focused(window) {
            Some(false)
        } else {
            None
        }
    }

    fn sql_line(&self, filter: bool) -> &Entity<EditorState> {
        if filter {
            &self.view.filter
        } else {
            &self.view.order
        }
    }

    fn menu_open(&self, filter: bool, cx: &App) -> bool {
        self.sql_line(filter).read(cx).completion_menu_state().open
    }

    fn accepts_menu(&self, filter: bool, cx: &App) -> bool {
        Settings::global(cx).auto_select_first
            || if filter {
                self.view.filter_menu_moved
            } else {
                self.view.order_menu_moved
            }
    }

    pub(super) fn on_sql_line_enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(filter) = self.focused_sql_line(window, cx) else {
            return;
        };
        match line_enter(self.menu_open(filter, cx), self.accepts_menu(filter, cx)) {
            LineEnter::Apply => {
                self.apply_filter(window, cx);
                cx.stop_propagation();
            }
            LineEnter::Accept => {
                self.sql_line(filter).update(cx, |e, cx| {
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
            }
            LineEnter::Dismiss => {
                self.sql_line(filter)
                    .update(cx, |e, cx| e.dismiss_completion_overlay(cx));
                cx.stop_propagation();
            }
        }
    }

    pub(super) fn on_sql_line_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(filter) = self.focused_sql_line(window, cx) else {
            return;
        };
        if !self.menu_open(filter, cx) {
            return;
        }
        if self.accepts_menu(filter, cx) {
            self.sql_line(filter).update(cx, |e, cx| {
                e.route_overlay_action(
                    Box::new(Enter {
                        secondary: false,
                        shift: false,
                    }),
                    window,
                    cx,
                );
            });
        } else {
            self.sql_line(filter)
                .update(cx, |e, cx| e.dismiss_completion_overlay(cx));
        }
        cx.stop_propagation();
    }

    pub(super) fn note_sql_menu_moved(&mut self, cx: &mut Context<Self>) {
        if self.menu_open(true, cx) {
            self.view.filter_menu_moved = true;
        }
        if self.menu_open(false, cx) {
            self.view.order_menu_moved = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LineEnter;

    #[test]
    fn a_closed_menu_applies_and_an_open_one_confirms_or_dismisses() {
        assert_eq!(super::line_enter(false, true), LineEnter::Apply);
        assert_eq!(super::line_enter(false, false), LineEnter::Apply);
        assert_eq!(super::line_enter(true, true), LineEnter::Accept);
        assert_eq!(super::line_enter(true, false), LineEnter::Dismiss);
    }
}
