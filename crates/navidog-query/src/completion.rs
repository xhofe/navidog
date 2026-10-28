//! Code completion (30-query-editor.md §4). Ranking lives in `navidog-sql`; this module feeds the
//! query editor's menu and turns a chosen snippet into tab stops.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use anyhow::Result;
use gpui_kit::component::input::{CompletionProvider, Rope, RopeExt as _};
use gpui_kit::{App, Context, Task, WeakEntity, Window};
use lsp_types::{
    CompletionContext, CompletionItem, CompletionResponse, CompletionTextEdit, TextEdit,
};
use navidog_app_state::settings::Settings;
use navidog_app_state::snippets::Snippets;
use navidog_app_state::store::ConnectionStore;
use navidog_model::object::Scope;
use navidog_session::outline::OutlineCache;
use navidog_sql::completion::{self, Catalog, Hit, Kind, OwnedCatalog, OwnedTable, Sources, Table};
use navidog_sql::dialect::Dialect;
use rust_i18n::t;

use crate::editor::QueryEditor;

fn detail(hit: &Hit) -> String {
    match hit.kind {
        Kind::Column => format!("{} · {}", t!("query.kind_column"), hit.type_name),
        Kind::Table => t!("query.kind_table").to_string(),
        Kind::View => t!("query.kind_view").to_string(),
        Kind::Schema => t!("query.schema").to_string(),
        Kind::Database => t!("query.database").to_string(),
        Kind::Function => t!("query.kind_function").to_string(),
        Kind::Builtin => t!("query.kind_builtin").to_string(),
        Kind::Keyword => t!("query.kind_keyword").to_string(),
        Kind::Snippet => t!("query.snippets").to_string(),
        Kind::Type => hit.label.clone(),
    }
}

pub(crate) fn highlight_label(query: &str, item: &CompletionItem) -> Vec<Range<usize>> {
    completion::highlight(query, &item.label)
}

/// Feeds the editor's completion menu.
pub(crate) struct Provider {
    pub editor: WeakEntity<QueryEditor>,
    /// Snippet codes offered last, to spot one being inserted and start its tab stops
    pub offered: Rc<RefCell<Vec<String>>>,
}

/// What one caret asks the shared ranker for.
#[derive(Copy, Clone)]
pub(crate) struct Request<'a> {
    pub text: &'a Rope,
    pub offset: usize,
    pub dialect: &'a dyn Dialect,
    /// Database (PostgreSQL: schema) whose outline is ready.
    pub scope: Option<&'a Scope>,
    pub current: Option<&'a str>,
    pub snippets: &'a [(String, String)],
    pub in_scope: &'a [(String, String)],
    pub extras: &'a [Hit],
}

/// Menu rows for one caret.
pub(crate) fn response(req: &Request<'_>, cx: &App) -> CompletionResponse {
    let Request {
        text,
        offset,
        dialect,
        scope,
        current,
        snippets,
        in_scope,
        extras,
    } = *req;
    let cache = OutlineCache::global(cx);
    let cache = cache.read(cx);
    let outline = scope.and_then(|s| cache.ready(s));
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
        let s = scope?;
        let other = if dialect.features().has_schemas {
            Scope::new(s.connection.clone(), s.database.clone(), Some(name.into()))
        } else {
            Scope::new(s.connection.clone(), name, None)
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
            current,
            catalog,
            others: &others,
            snippets,
            in_scope,
            extras,
            keywords: navidog_sql::completion::Keywords::All,
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
    CompletionResponse::Array(items)
}

impl CompletionProvider for Provider {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        _: CompletionContext,
        _: &mut Window,
        cx: &mut App,
    ) -> Task<Result<CompletionResponse>> {
        let Some(editor) = self.editor.upgrade() else {
            return Task::ready(Ok(CompletionResponse::Array(Vec::new())));
        };
        let e = editor.read(cx);
        let Some(dialect) = e.dialect(cx) else {
            return Task::ready(Ok(CompletionResponse::Array(Vec::new())));
        };
        let driver = e
            .target
            .connection
            .as_ref()
            .and_then(|id| ConnectionStore::global(cx).read(cx).get(id))
            .map(|c| c.driver.clone())
            .unwrap_or_default();
        let snippets: Vec<(String, String)> = Snippets::global(cx)
            .read(cx)
            .all()
            .filter(|s| s.applies_to(&driver))
            .map(|s| (s.name.clone(), s.code.clone()))
            .collect();
        *self.offered.borrow_mut() = snippets.iter().map(|(_, code)| code.clone()).collect();
        let current = if dialect.features().has_schemas {
            e.target.schema.clone()
        } else {
            e.target.database.clone()
        };
        let scope = e.target.scope();
        Task::ready(Ok(response(
            &Request {
                text,
                offset,
                dialect: dialect.as_ref(),
                scope: scope.as_ref(),
                current: current.as_deref(),
                snippets: &snippets,
                in_scope: &[],
                extras: &[],
            },
            cx,
        )))
    }

    fn is_completion_trigger(&self, _: usize, new_text: &str, cx: &mut App) -> bool {
        let Some(editor) = self.editor.upgrade() else {
            return false;
        };
        // Runs inside the editor update (each keystroke). `completion_on` must
        // not read that editor, or the app aborts.
        completion_on(editor.read(cx), cx) && completion::is_trigger(new_text)
    }
}

/// Off in the preferences, or above the size where highlighting and completion stop (F-PREF-004).
pub(crate) fn completion_on(e: &QueryEditor, cx: &App) -> bool {
    Settings::global(cx).code_completion && e.sql_highlight
}

impl QueryEditor {
    /// Loads the completion info for the picked database or schema; with "auto update" it is
    /// fetched again each time one is picked.
    pub(crate) fn ensure_outline(&mut self, refresh: bool, cx: &mut Context<Self>) {
        let Some(scope) = self.target.scope() else {
            return;
        };
        if !navidog_session::sessions::Sessions::global(cx)
            .read(cx)
            .state(&scope.connection)
            .is_open()
        {
            return;
        }
        let auto = Settings::global(cx).auto_update_outline;
        OutlineCache::global(cx).update(cx, |c, cx| c.ensure(&scope, refresh || auto, cx));
    }

    /// A snippet chosen from the completion menu arrives as its raw code; turn its `${…}` into
    /// tab stops.
    pub(crate) fn expand_inserted_snippet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let codes = self.offered_snippets.borrow().clone();
        if codes.is_empty() {
            return;
        }
        let (caret, text) = {
            let e = self.editor.read(cx);
            (e.cursor(), e.value().to_string())
        };
        let hit = codes.into_iter().find(|code| {
            code.contains("${")
                && caret >= code.len()
                && text.get(caret - code.len()..caret) == Some(code.as_str())
        });
        let Some(code) = hit else { return };
        self.offered_snippets.borrow_mut().clear();
        let start = caret - code.len();
        self.editor
            .update(cx, |e, cx| e.set_selected_range(start..caret, cx));
        self.insert_snippet(&code, window, cx);
    }
}
