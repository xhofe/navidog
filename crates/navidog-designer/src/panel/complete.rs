//! Completion for the table designer's SQL fields (40-table-designer.md).

use std::ops::Range;
use std::rc::Rc;

use anyhow::Result;
use gpui_kit::component::input::{
    CompletionProvider, EditorState, Enter, InputState, Rope, RopeExt as _,
};
use gpui_kit::{App, Context, Entity, Task, WeakEntity, Window, px};
use lsp_types::{
    CompletionContext, CompletionItem, CompletionResponse, CompletionTextEdit, TextEdit,
};
use navidog_app_state::settings::Settings;
use navidog_session::outline::OutlineCache;
use navidog_sql::completion::{self, Catalog, Hit, Kind, OwnedCatalog, OwnedTable, Sources, Table};
use navidog_sql::dialect::Dialect;
use rust_i18n::t;

use super::*;

/// A form field. SQL fields are single-line editors so they can host a completion menu.
#[derive(Clone)]
pub(super) enum Line {
    Text(Entity<InputState>),
    Sql(Entity<EditorState>),
}

impl Line {
    pub(super) fn value(&self, cx: &App) -> String {
        match self {
            Line::Text(e) => e.read(cx).value().to_string(),
            Line::Sql(e) => e.read(cx).value().to_string(),
        }
    }

    /// Text written into the draft. A single-line SQL field drops pasted line breaks.
    pub(super) fn draft_value(&self, cx: &App) -> String {
        let value = self.value(cx);
        match self {
            Line::Sql(_) => completion::one_line(&value),
            Line::Text(_) => value,
        }
    }

    pub(super) fn set_value(&self, value: impl Into<String>, window: &mut Window, cx: &mut App) {
        let value = value.into();
        match self {
            Line::Text(e) => e.update(cx, |s, cx| s.set_value(value, window, cx)),
            Line::Sql(e) => e.update(cx, |s, cx| s.set_value(value, window, cx)),
        }
    }

    pub(super) fn text(&self) -> Entity<InputState> {
        match self {
            Line::Text(e) => e.clone(),
            Line::Sql(_) => panic!("a sql field has no plain input"),
        }
    }

    pub(super) fn sql(&self) -> Option<Entity<EditorState>> {
        match self {
            Line::Sql(e) => Some(e.clone()),
            Line::Text(_) => None,
        }
    }

    #[cfg(test)]
    pub(super) fn entity_id(&self) -> gpui_kit::EntityId {
        match self {
            Line::Text(e) => e.entity_id(),
            Line::Sql(e) => e.entity_id(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SqlMode {
    /// Draft columns, clause keywords (`AND`, `OR`, …) and built-in functions.
    Expression,
    /// [`SqlMode::Expression`]'s columns, plus the full keyword list and type names.
    Definition,
    /// [`SqlMode::Definition`] without type names, plus an already loaded outline.
    Catalog,
}

pub(super) fn property_is_sql(ix: usize) -> bool {
    matches!(
        structured::PROPERTIES.get(ix),
        Some(
            ColumnProperty::Default
                | ColumnProperty::GeneratedExpression
                | ColumnProperty::OnUpdate
        )
    )
}

pub(super) fn entry_is_sql(ix: usize) -> bool {
    structured::ENTRY_KEYS.get(ix) == Some(&"expression")
}

pub(super) fn line(
    sql: bool,
    value: String,
    mode: SqlMode,
    owner: WeakEntity<TableDesigner>,
    window: &mut Window,
    cx: &mut Context<TableDesigner>,
) -> Line {
    if !sql {
        return Line::Text(cx.new(|cx| InputState::new(window, cx).default_value(value)));
    }
    let editor = cx.new(|cx| {
        EditorState::new(window, cx)
            .language("sql")
            .line_number(false)
            .folding(false)
            .soft_wrap(false)
            .auto_close(false)
            .searchable(false)
            .default_value(value)
    });
    attach(&editor, owner, mode, cx);
    Line::Sql(editor)
}

pub(super) fn attach(
    editor: &Entity<EditorState>,
    designer: WeakEntity<TableDesigner>,
    mode: SqlMode,
    cx: &mut App,
) {
    editor.update(cx, |e, _| {
        e.lsp_mut().completion_provider = Some(Rc::new(DesignerProvider { designer, mode }));
        e.lsp_mut()
            .set_completion_highlighter(Rc::new(highlight_label));
        // raw-px: room for "column · type" labels, same as the query editor
        e.lsp_mut().completion_menu.max_width = px(420.);
    });
}

/// Schedule a collapse of pasted line breaks. `set_value` does not emit `Change`.
pub(super) fn collapse(
    editor: &Entity<EditorState>,
    window: &mut Window,
    cx: &mut Context<TableDesigner>,
) {
    let value = editor.read(cx).value().to_string();
    let flat = completion::one_line(&value);
    if flat == value {
        return;
    }
    let editor = editor.clone();
    cx.defer_in(window, move |this, window, cx| {
        this.syncing = true;
        editor.update(cx, |s, cx| s.set_value(flat, window, cx));
        this.syncing = false;
    });
}

fn detail(hit: &Hit) -> String {
    match hit.kind {
        Kind::Column if hit.type_name.is_empty() => t!("designer.kind_column").to_string(),
        Kind::Column => format!("{} · {}", t!("designer.kind_column"), hit.type_name),
        Kind::Table => t!("designer.kind_table").to_string(),
        Kind::View => t!("designer.kind_view").to_string(),
        Kind::Schema => t!("designer.kind_schema").to_string(),
        Kind::Database => t!("designer.kind_database").to_string(),
        Kind::Function => t!("designer.kind_function").to_string(),
        Kind::Builtin => t!("designer.kind_builtin").to_string(),
        Kind::Keyword => t!("designer.kind_keyword").to_string(),
        Kind::Type => t!("designer.kind_type").to_string(),
        Kind::Snippet => hit.label.clone(),
    }
}

fn highlight_label(query: &str, item: &CompletionItem) -> Vec<Range<usize>> {
    completion::highlight(query, &item.label)
}

fn type_hits(dialect: &dyn Dialect) -> Vec<Hit> {
    dialect
        .column_types()
        .iter()
        .map(|t| Hit {
            label: t.name.to_string(),
            insert: t.name.to_string(),
            kind: Kind::Type,
            type_name: String::new(),
        })
        .collect()
}

fn draft_columns(columns: &[navidog_schema::table_design::DesignColumn]) -> Vec<(String, String)> {
    columns
        .iter()
        .filter(|c| !c.name.is_empty())
        .map(|c| {
            let ty = c
                .definition
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string();
            (c.name.clone(), ty)
        })
        .collect()
}

struct DesignerProvider {
    designer: WeakEntity<TableDesigner>,
    mode: SqlMode,
}

impl CompletionProvider for DesignerProvider {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        _: CompletionContext,
        _: &mut Window,
        cx: &mut App,
    ) -> Task<Result<CompletionResponse>> {
        let Some(designer) = self.designer.upgrade() else {
            return Task::ready(Ok(CompletionResponse::Array(Vec::new())));
        };
        let designer = designer.read(cx);
        let dialect = designer.dialect.as_ref();
        let in_scope = draft_columns(&designer.draft.columns);
        let extras = if self.mode == SqlMode::Definition {
            type_hits(dialect)
        } else {
            Vec::new()
        };
        let scope = (self.mode == SqlMode::Catalog).then_some(designer.scope.clone());
        let cache = OutlineCache::global(cx);
        let cache = cache.read(cx);
        let outline = scope.as_ref().and_then(|s| cache.ready(s));
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
            let s = scope.as_ref()?;
            let other = if dialect.features().has_schemas {
                navidog_model::object::Scope::new(
                    s.connection.clone(),
                    s.database.clone(),
                    Some(name.into()),
                )
            } else {
                navidog_model::object::Scope::new(s.connection.clone(), name, None)
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
        let current = if dialect.features().has_schemas {
            designer
                .scope
                .schema
                .clone()
                .or_else(|| dialect.default_schema().map(str::to_string))
        } else {
            Some(designer.scope.database.clone()).filter(|s| !s.is_empty())
        };
        let raw = text.to_string();
        let (from, window_text) = completion::window(&raw, offset);
        let keywords = if self.mode == SqlMode::Expression {
            completion::Keywords::Clause
        } else {
            completion::Keywords::All
        };
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
                extras: &extras,
                keywords,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LineEnter {
    /// Menu closed. A single-line field swallows the key; a multi-line field inserts a newline.
    Closed,
    Accept,
    Dismiss,
}

pub(super) fn line_enter(menu_open: bool, accepts: bool) -> LineEnter {
    if !menu_open {
        LineEnter::Closed
    } else if accepts {
        LineEnter::Accept
    } else {
        LineEnter::Dismiss
    }
}

impl TableDesigner {
    fn editor_open(editor: &Entity<EditorState>, cx: &App) -> bool {
        editor.read(cx).completion_menu_state().open
    }

    fn focused_sql(&self, window: &Window, cx: &App) -> Option<(Entity<EditorState>, bool)> {
        let mut single = vec![self.definition.clone()];
        for line in self.properties.iter().chain(self.entry_inputs.iter()) {
            if let Some(editor) = line.sql() {
                single.push(editor);
            }
        }
        for editor in &single {
            if editor.read(cx).focus_handle(cx).is_focused(window) {
                return Some((editor.clone(), true));
            }
        }
        for editor in [&self.constraints, &self.indexes, &self.options] {
            if editor.read(cx).focus_handle(cx).is_focused(window) {
                return Some((editor.clone(), false));
            }
        }
        None
    }

    fn accepts(&self, cx: &App) -> bool {
        Settings::global(cx).auto_select_first || self.menu_moved
    }

    pub(super) fn on_sql_enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((editor, single)) = self.focused_sql(window, cx) else {
            return;
        };
        match line_enter(Self::editor_open(&editor, cx), self.accepts(cx)) {
            LineEnter::Closed if single => cx.stop_propagation(),
            LineEnter::Closed => {}
            LineEnter::Accept => {
                editor.update(cx, |e, cx| {
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
            LineEnter::Dismiss if single => {
                editor.update(cx, |e, cx| e.dismiss_completion_overlay(cx));
                cx.stop_propagation();
            }
            // A multi-line field closes the menu and then inserts the newline.
            LineEnter::Dismiss => {
                editor.update(cx, |e, cx| e.dismiss_completion_overlay(cx));
            }
        }
    }

    pub(super) fn on_sql_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((editor, single)) = self.focused_sql(window, cx) else {
            return;
        };
        if !Self::editor_open(&editor, cx) {
            return;
        }
        if self.accepts(cx) {
            editor.update(cx, |e, cx| {
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
        } else if single {
            editor.update(cx, |e, cx| e.dismiss_completion_overlay(cx));
            cx.stop_propagation();
        } else {
            editor.update(cx, |e, cx| e.dismiss_completion_overlay(cx));
        }
    }

    pub(super) fn note_sql_menu_moved(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some((editor, _)) = self.focused_sql(window, cx) else {
            return;
        };
        if Self::editor_open(&editor, cx) {
            self.menu_moved = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use navidog_schema::table_design::DesignColumn;

    #[test]
    fn a_closed_menu_lets_the_field_decide_and_an_open_one_confirms_or_dismisses() {
        assert_eq!(line_enter(false, true), LineEnter::Closed);
        assert_eq!(line_enter(true, true), LineEnter::Accept);
        assert_eq!(line_enter(true, false), LineEnter::Dismiss);
    }

    #[test]
    fn expression_completion_lists_draft_columns_and_definition_lists_types() {
        let dialect = navidog_sqlite::SqliteDialect;
        let columns = vec![DesignColumn::new(1, "display_name", "TEXT")];
        let in_scope = draft_columns(&columns);
        let (_, hits) = completion::complete(
            "dis",
            3,
            &Sources {
                dialect: &dialect,
                current: None,
                catalog: None,
                others: &|_| None,
                snippets: &[],
                in_scope: &in_scope,
                extras: &[],
                keywords: completion::Keywords::Clause,
            },
        );
        assert!(hits.iter().any(|h| h.label == "display_name"), "{hits:?}");
        let (_, hits) = completion::complete(
            "AN",
            2,
            &Sources {
                dialect: &dialect,
                current: None,
                catalog: None,
                others: &|_| None,
                snippets: &[],
                in_scope: &in_scope,
                extras: &[],
                keywords: completion::Keywords::Clause,
            },
        );
        assert!(hits.iter().any(|h| h.label == "AND"), "{hits:?}");
        let (_, hits) = completion::complete(
            "SEL",
            3,
            &Sources {
                dialect: &dialect,
                current: None,
                catalog: None,
                others: &|_| None,
                snippets: &[],
                in_scope: &[],
                extras: &[],
                keywords: completion::Keywords::Clause,
            },
        );
        assert!(!hits.iter().any(|h| h.label == "SELECT"), "{hits:?}");
        let extras = type_hits(&dialect);
        let (_, hits) = completion::complete(
            "INT",
            3,
            &Sources {
                dialect: &dialect,
                current: None,
                catalog: None,
                others: &|_| None,
                snippets: &[],
                in_scope: &in_scope,
                extras: &extras,
                keywords: completion::Keywords::All,
            },
        );
        assert!(
            hits.iter()
                .any(|h| h.kind == Kind::Type && h.insert == "INTEGER"),
            "{hits:?}"
        );
    }
}
