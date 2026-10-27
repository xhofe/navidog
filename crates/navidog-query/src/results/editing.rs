//! Editable detection, cell editing, commit, reload and the guarded busy state of a result.

use super::*;

use std::collections::{BTreeMap, HashSet};

use gpui_kit::component::WindowExt as _;
#[cfg(test)]
use gpui_kit::{App, SharedString};
use gpui_kit::{Context, Entity, Window};
use navidog_app_state::settings::Settings;
use navidog_db::Column;
use navidog_db::commit::{Commit, apply};
use navidog_grid::{GridEvent, GridTarget, ResultGrid};
use navidog_session::sessions::{Sessions, prepare_database, run_on};
use navidog_sql::dialect::{ChangeSet, RowKey, Values, Writes};
use navidog_ui::assets::lucide;
use navidog_util::sql_text;
use rust_i18n::t;

use crate::errors::error_message;

use super::tabs::grid_data;

impl ResultView {
    /// Keeps the cell editor on the focused cell (needs `window`, so called from render).
    pub(crate) fn sync_cell(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.cell_editor {
            return;
        }
        let model = self.grid.read(cx).model(cx);
        if model.row_count() == 0 {
            return;
        }
        let at = model.head();
        if self.cell_at == Some(at) {
            return;
        }
        let text = model
            .value(at.0, at.1)
            .cloned()
            .flatten()
            .map(|v| v.to_string())
            .unwrap_or_default();
        self.cell_at = Some(at);
        self.cell_input
            .update(cx, |s, cx| s.set_value(text, window, cx));
    }

    /// Typing in the cell editor edits the cell, when the result is editable.
    pub(crate) fn write_cell(&mut self, text: String, cx: &mut Context<Self>) {
        let (Some((r, c)), true) = (
            self.cell_at,
            self.session.edit.is_some() && !self.session.busy,
        ) else {
            return;
        };
        let current = self.grid.read(cx).model(cx).value(r, c).cloned().flatten();
        if current.as_deref() == Some(text.as_str()) {
            return;
        }
        self.grid.update(cx, |g, cx| {
            g.update_model(cx, |m| m.set_value(r, c, Some(text.into())))
        });
    }

    #[cfg(test)]
    pub(crate) fn editable(&self) -> bool {
        self.session.edit.is_some()
    }

    #[cfg(test)]
    pub(crate) fn commit_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit(window, cx);
    }

    #[cfg(test)]
    pub(crate) fn refresh_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh(window, cx);
    }

    #[cfg(test)]
    pub(crate) fn sort_now(
        &mut self,
        sort: Option<(SharedString, bool)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.on_grid_event(
            &self.grid.clone(),
            &GridEvent::SortChanged(sort),
            window,
            cx,
        );
    }

    #[cfg(test)]
    pub(crate) fn status_text(&self) -> &str {
        &self.session.status
    }

    #[cfg(test)]
    pub(crate) fn status(&self) -> &str {
        &self.session.status
    }

    #[cfg(test)]
    pub(crate) fn is_busy(&self) -> bool {
        self.session.busy
    }

    #[cfg(test)]
    pub(crate) fn rows(&self, cx: &App) -> Vec<Vec<Option<String>>> {
        let model = self.grid.read(cx).model(cx);
        (0..model.row_count())
            .map(|r| {
                (0..model.columns.len())
                    .map(|c| model.value(r, c).cloned().flatten().map(|v| v.to_string()))
                    .collect()
            })
            .collect()
    }

    /// Looks up the primary key of the result's table; with all of it in the result, the result
    /// becomes editable.
    pub(crate) fn find_table(&mut self, cx: &mut Context<Self>) {
        let Some(dialect) = self.session.info.dialect.clone() else {
            return;
        };
        let Some((schema, table)) = single_table(&self.session.sql, &self.session.columns) else {
            return;
        };
        let Some(scope) = self.session.info.scope_for(&schema) else {
            return;
        };
        let generation = Sessions::global(cx).read(cx).generation(&scope.connection);
        let (s, t) = (scope.clone(), table.clone());
        let Some(job) = Sessions::global(cx).read(cx).run_rel(
            &scope.connection,
            Some(&scope.database),
            move |rel| Box::pin(async move { rel.table_columns(&s, &t).await }),
        ) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let Ok(Ok(columns)) = job.await else { return };
            let _ = this.update(cx, |this, cx| {
                if Sessions::global(cx).read(cx).generation(&scope.connection) != generation {
                    return;
                }
                let pk: Vec<&str> = columns
                    .iter()
                    .filter(|c| c.primary_key)
                    .map(|c| c.name.as_str())
                    .collect();
                let names: BTreeMap<String, String> = this
                    .session
                    .columns
                    .iter()
                    .filter_map(|c| Some((c.name.clone(), c.origin.as_ref()?.column.clone())))
                    .collect();
                let all_pk_present =
                    !pk.is_empty() && pk.iter().all(|k| names.values().any(|v| v == k));
                if !all_pk_present {
                    return;
                }
                this.session.edit = Some(Editing {
                    dialect: dialect.clone(),
                    scope: scope.clone(),
                    table: table.clone(),
                    names,
                    pk: pk.iter().map(|k| k.to_string()).collect(),
                });
                this.grid.update(cx, |g, cx| {
                    g.set_target(
                        Some(GridTarget {
                            dialect,
                            scope,
                            table,
                        }),
                        cx,
                    );
                    g.update_model(cx, |model| {
                        for c in &mut model.columns {
                            c.primary_key =
                                this.session.edit.as_ref().is_some_and(|e| e.is_pk(&c.name));
                        }
                    });
                    g.set_sort(this.session.accepted_sort.clone(), cx);
                    g.set_locked(false, cx);
                });
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn on_grid_event(
        &mut self,
        _: &Entity<ResultGrid>,
        e: &GridEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match e {
            GridEvent::SortChanged(sort) => {
                self.grid.update(cx, |g, cx| g.commit_edit(window, cx));
                if self.grid.read(cx).model(cx).has_changes() {
                    self.grid.update(cx, |g, cx| {
                        g.set_sort(self.session.accepted_sort.clone(), cx)
                    });
                    self.session.status = t!("query.sort_blocked").to_string().into();
                    cx.notify();
                    return;
                }
                self.session.accepted_sort = sort.clone();
                if let Some((name, asc)) = sort
                    && let Some(col) = self
                        .grid
                        .read(cx)
                        .model(cx)
                        .columns
                        .iter()
                        .position(|c| c.name == *name)
                {
                    self.grid
                        .update(cx, |g, cx| g.update_model(cx, |m| m.sort_rows(col, *asc)));
                }
                self.grid.update(cx, |g, cx| g.accept_header_sort(cx));
            }
            GridEvent::CommitRequested => self.commit(window, cx),
            GridEvent::Changed => cx.notify(),
            _ => {}
        }
    }

    pub(crate) fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.busy {
            return;
        }
        let Some(edit) = &self.session.edit else {
            return;
        };
        self.grid.update(cx, |g, cx| g.commit_edit(window, cx));
        let model = self.grid.read(cx).model(cx);
        if !model.has_changes() {
            return;
        }
        let changes = rename_changes(model.change_set(), &edit.names);
        let statements = edit
            .dialect
            .render_changes(&edit.scope, &edit.table, &changes, true);
        if statements.is_empty() {
            return;
        }
        self.session.status = statements.join(";\n").into();
        let commit = Commit::new(edit.dialect.clone(), statements);
        let generation = Sessions::global(cx)
            .read(cx)
            .generation(&self.session.info.connection_id);
        let database = edit.scope.database.clone();
        let dialect = edit.dialect.clone();
        // A fresh operation session cannot inherit a metadata caller's transaction or locks.
        let Some(job) = Sessions::global(cx).read(cx).run_operation_rel(
            &edit.scope.connection,
            Some(&edit.scope.database),
            move |rel| {
                Box::pin(async move {
                    prepare_database(rel, dialect.as_ref(), &database).await?;
                    apply(rel, commit, false, false).await
                })
            },
        ) else {
            return;
        };
        self.set_busy(true, cx);
        cx.spawn_in(window, async move |this, cx| {
            let result = job
                .await
                .map_err(|e| navidog_db::DbError::msg(e.to_string()))
                .and_then(|r| r);
            let _ = this.update_in(cx, |this, window, cx| {
                if !this.accepts_generation(generation, cx) {
                    return;
                }
                this.set_busy(false, cx);
                match result {
                    Ok(()) => {
                        let sync = Settings::global(cx).sync_record;
                        let unknown = this.grid.read(cx).model(cx).commit_needs_reload();
                        // The write is complete even if the following SELECT fails. Leaving its
                        // changes pending would allow a later Apply to replay the same batch.
                        this.grid.update(cx, |g, cx| {
                            g.update_model(cx, |m| m.accept_committed_preserving_drafts())
                        });
                        if sync || unknown {
                            this.reload(cx);
                        }
                    }
                    Err(e) => {
                        let e = error_message(&e);
                        this.session.status = e.clone().into();
                        window.open_alert_dialog(cx, move |alert, _, _| {
                            alert
                                .icon(lucide("circle-x"))
                                .title(t!("query.commit_failed").to_string())
                                .description(e.clone())
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Guard reserved for a future explicit refresh action; no user entry point exists yet.
    #[allow(dead_code)] // Production currently reloads only after accepting a successful write.
    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.grid.update(cx, |g, cx| g.commit_edit(window, cx));
        if self.grid.read(cx).model(cx).has_changes() {
            self.session.status = t!("query.pending_result_changes").to_string().into();
            cx.notify();
            return;
        }
        if self.session.busy {
            return;
        }
        self.reload(cx);
    }

    /// Reloads accepted rows after a successful commit, or the internal explicit-refresh guard.
    /// Reuses the editor session only while it still matches the result's original source.
    fn reload(&mut self, cx: &mut Context<Self>) {
        let session = self.session.editor.upgrade().and_then(|e| {
            e.read(cx)
                .live
                .as_ref()
                .filter(|live| live.matches_source(&self.session.info))
                .map(|live| live.session.clone())
        });
        let open = if session.is_none() {
            if !self.session.info.context_known {
                self.session.status = t!("query.source_context_unknown").to_string().into();
                cx.notify();
                return;
            }
            Sessions::global(cx).read(cx).new_session(
                &self.session.info.connection_id,
                self.session.info.database.as_deref(),
            )
        } else {
            None
        };
        if session.is_none() && open.is_none() {
            self.session.status = t!("query.no_connection").to_string().into();
            cx.notify();
            return;
        }
        let generation = Sessions::global(cx)
            .read(cx)
            .generation(&self.session.info.connection_id);
        let sql = self.session.sql.clone();
        let info = self.session.info.clone();
        self.set_busy(true, cx);
        cx.spawn(async move |this, cx| {
            let result = async {
                let owned = session.is_none();
                let session = match session {
                    Some(session) => session,
                    None => {
                        let session = open
                            .expect("a source session is opening")
                            .await
                            .map_err(|e| navidog_db::DbError::msg(e.to_string()))??;
                        std::sync::Arc::new(tokio::sync::Mutex::new(Some(session)))
                    }
                };
                let result = run_on(session.clone(), move |rel| {
                    Box::pin(async move {
                        if owned && let Some(dialect) = info.dialect.as_ref() {
                            for statement in crate::run::owned_context_sql(
                                dialect.as_ref(),
                                info.database.as_deref(),
                                info.search_path.as_deref(),
                                info.schema.as_deref(),
                            ) {
                                rel.execute(&statement).await?;
                            }
                        }
                        rel.execute_statement(&sql).await
                    })
                })
                .await;
                if owned {
                    navidog_grid::commit::close_session(session);
                }
                result.map_err(|e| navidog_db::DbError::msg(e.to_string()))?
            }
            .await;
            let _ = this.update(cx, |this, cx| {
                if !this.accepts_generation(generation, cx) {
                    return;
                }
                this.set_busy(false, cx);
                match result {
                    Ok(sets) => {
                        if let Some(set) = sets.into_iter().find(|s| !s.columns.is_empty()) {
                            let (mut columns, rows) = grid_data(&set);
                            if let Some(edit) = &this.session.edit {
                                for column in &mut columns {
                                    column.primary_key = edit.is_pk(&column.name);
                                }
                            }
                            this.session.columns = set.columns;
                            // Preserve a valid accepted sort across synchronization. Otherwise
                            // the SQL's original order would disagree with the cached arrow.
                            this.session.accepted_sort =
                                this.session.accepted_sort.take().filter(|(name, _)| {
                                    columns.iter().any(|column| column.name == *name)
                                });
                            this.grid.update(cx, |g, cx| {
                                let mut drafts = None;
                                g.update_model(cx, |model| {
                                    drafts = Some(model.take_blank_insert_drafts())
                                });
                                g.set_data(columns, rows, cx);
                                if let Some((name, asc)) = &this.session.accepted_sort
                                    && let Some(col) =
                                        g.model(cx).columns.iter().position(|c| c.name == *name)
                                {
                                    g.update_model(cx, |model| model.sort_rows(col, *asc));
                                }
                                if let Some(drafts) = drafts {
                                    g.update_model(cx, |model| {
                                        model.restore_blank_insert_drafts(drafts)
                                    });
                                }
                                g.set_sort(this.session.accepted_sort.clone(), cx);
                            });
                        }
                    }
                    Err(error) => this.session.status = error_message(&error).into(),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// A retired operation must not replace rows, accept edits, or surface an old modal.
    fn accepts_generation(&mut self, generation: u64, cx: &mut Context<Self>) -> bool {
        if Sessions::global(cx)
            .read(cx)
            .generation(&self.session.info.connection_id)
            == generation
        {
            return true;
        }
        // This view serializes its work through `busy`; reopening does not start a newer load.
        self.set_busy(false, cx);
        self.session.status = t!("query.connection_changed_during_operation")
            .to_string()
            .into();
        cx.notify();
        false
    }

    fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        self.session.busy = busy;
        let lock = busy || self.session.edit.is_none();
        self.grid.update(cx, |g, cx| g.set_locked(lock, cx));
    }
}

/// The table a result can be edited in: a SELECT whose columns all come from one table, each
/// column at most once (31 §4.2). The primary key check needs the server and comes after.
pub(super) fn single_table(sql: &str, columns: &[Column]) -> Option<(String, String)> {
    if !sql_text::first_keyword(sql).eq_ignore_ascii_case("SELECT") {
        return None;
    }
    let first = columns.first()?.origin.as_ref()?;
    let mut seen = HashSet::new();
    for c in columns {
        let o = c.origin.as_ref()?;
        if o.schema != first.schema || o.table != first.table || !seen.insert(o.column.as_str()) {
            return None;
        }
    }
    Some((first.schema.clone(), first.table.clone()))
}

/// The grid writes changes under the result's column names, which may be aliases; the table
/// knows the source names.
pub(super) fn rename_changes(changes: ChangeSet, names: &BTreeMap<String, String>) -> ChangeSet {
    let values = |v: Values| -> Values {
        v.into_iter()
            .map(|(c, x)| (names.get(&c).cloned().unwrap_or(c), x))
            .collect()
    };
    let writes = |w: Writes| -> Writes {
        w.into_iter()
            .map(|(c, x)| (names.get(&c).cloned().unwrap_or(c), x))
            .collect()
    };
    ChangeSet {
        updates: changes
            .updates
            .into_iter()
            .map(|u| navidog_sql::dialect::RowUpdate {
                key: RowKey(values(u.key.0)),
                values: writes(u.values),
            })
            .collect(),
        inserts: changes.inserts.into_iter().map(writes).collect(),
        deletes: changes
            .deletes
            .into_iter()
            .map(|k| RowKey(values(k.0)))
            .collect(),
    }
}
