use super::*;

use std::sync::Arc;

use gpui_kit::{App, Context, Window};
use navidog_app_state::settings::Settings;
use navidog_db::ResultSet;
use navidog_db::dialect::Dialect;
use navidog_session::sessions::run_on;
use navidog_ui::assets::lucide;
use rust_i18n::t;

use crate::commit::{Commit, apply, error_message};
use crate::model::{Cell, GridColumn};

impl TableViewer {
    /// Projects one loaded page onto the grid. Returns whether a close that waited on this load
    /// should continue.
    pub(crate) fn apply_page(
        &mut self,
        generation: u64,
        result: Result<Vec<ResultSet>, String>,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.generation_matches(generation, cx) {
            return false;
        }
        self.set_busy(false, cx);
        match result {
            Ok(mut sets) if !sets.is_empty() => {
                let set = sets.swap_remove(0);
                let pk = |name: &str| self.columns.iter().any(|c| c.name == name && c.primary_key);
                let columns: Vec<GridColumn> = set
                    .columns
                    .iter()
                    .map(|c| {
                        let info = self.columns.iter().find(|i| i.name == c.name);
                        let type_name = info
                            .map(|i| i.type_name.clone())
                            .filter(|t| !t.is_empty())
                            .unwrap_or_else(|| c.type_name.clone());
                        let read_only = c.name == "_rowid";
                        GridColumn {
                            name: if read_only {
                                "ROWID".into()
                            } else {
                                c.name.clone().into()
                            },
                            type_name: type_name.into(),
                            primary_key: pk(&c.name),
                            comment: info.and_then(|i| i.comment.clone()).map(Into::into),
                            read_only,
                        }
                    })
                    .collect();
                let rows: Vec<Vec<Cell>> = set
                    .rows
                    .into_iter()
                    .map(|r| r.into_iter().map(|v| v.map(Into::into)).collect())
                    .collect();
                self.grid.update(cx, |g, cx| {
                    let mut drafts = None;
                    g.update_model(cx, |m| drafts = Some(m.take_blank_insert_drafts()));
                    g.set_data(columns, rows, cx);
                    g.update_model(cx, |m| {
                        m.restore_blank_insert_drafts(drafts.expect("the blank drafts were moved"));
                    });
                });
                self.reproject(cx);
                if std::mem::take(&mut self.add_after_commit) {
                    self.insert_row(cx);
                }
            }
            Ok(_) => {}
            Err(e) => {
                self.add_after_commit = false;
                self.status = e.into();
            }
        }
        cx.notify();
        self.close_after_commit && !self.busy
    }

    /// Reloads after the page, sort, filter or limit changed. Like leaving a modified row
    /// (F-DATA-003), pending edits are committed first and the page loads once that succeeds; a
    /// failed commit keeps the edits and the rows they belong to.
    pub(crate) fn reload_committed(
        &mut self,
        count: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.grid.update(cx, |g, cx| g.commit_edit(window, cx));
        if !self.grid.read(cx).model(cx).has_changes() {
            self.reload(count, cx);
            return;
        }
        // A commit is already running; its success reloads with the new state
        if self.busy {
            return;
        }
        self.reload_after_commit = true;
        self.apply_changes(false, window, cx);
    }

    pub(crate) fn commit_statements(&self, cx: &App) -> Commit {
        let d = self.dialect.unwrap_or(Dialect::MySql);
        let m = self.grid.read(cx).model(cx);
        let statements = d.render_changes(
            &self.object.scope,
            &self.object.name,
            &m.change_set(),
            m.has_primary_key(),
        );
        Commit::new(d, statements)
    }

    /// Commits the grid changes; with `end_transaction`, also commits the open transaction. On failure
    /// the database error is shown and the changes are kept.
    pub(crate) fn apply_changes(
        &mut self,
        end_transaction: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.session.clone() else {
            return;
        };
        // Statements already in flight must not be sent again. A close-prompt commit keeps
        // `close_after_commit`; the load that holds `busy` resumes it when it finishes.
        if self.busy {
            return;
        }
        // The cell being typed in belongs to this commit; once the grid locks it could not be written
        self.grid.update(cx, |g, cx| g.commit_edit(window, cx));
        self.flush_form(cx);
        let commit = self.commit_statements(cx);
        if commit.statements.is_empty() && !end_transaction {
            // Nothing to send. A hide still has to reproject; an empty new row stays as it is,
            // unless the page is being left.
            self.add_after_commit = false;
            if std::mem::take(&mut self.reload_after_commit) {
                self.hidden_after_commit = None;
                self.reload(true, cx);
            } else if let Some(next) = self.hidden_after_commit.take() {
                self.publish_hidden(next, cx);
            }
            return;
        }
        let in_tx = self.in_transaction;
        let generation = self.session_generation;
        self.status = commit.statements.join(";\n").into();
        self.set_busy(true, cx);
        let job = run_on(session, move |rel| {
            Box::pin(apply(rel, commit, in_tx, end_transaction))
        });
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = job
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r.map_err(|e| error_message(&e)));
            let _ = this.update_in(cx, |this, window, cx| {
                this.apply_commit_result(generation, result, end_transaction, window, cx);
            });
        }));
    }

    pub(crate) fn apply_commit_result(
        &mut self,
        generation: u64,
        result: Result<(), String>,
        end_transaction: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.generation_matches(generation, cx) {
            return;
        }
        self.set_busy(false, cx);
        match result {
            Ok(()) => {
                if end_transaction {
                    self.set_in_transaction(false, cx);
                }
                // Committed from the close prompt: nothing to reload, the tab goes
                if std::mem::take(&mut self.close_after_commit) {
                    self.grid.update(cx, |g, cx| {
                        g.update_model(cx, |m| m.accept_committed_preserving_drafts());
                    });
                    self.close_now(window, cx);
                    return;
                }
                let hidden = self.hidden_after_commit.take();
                let reproject = hidden.is_some();
                let leaving = std::mem::take(&mut self.reload_after_commit);
                let sync = !cx.has_global::<Settings>() || Settings::global(cx).sync_record;
                // Accept the completed write before any projection or reload. A failed
                // SELECT must not leave this batch available for a second Apply.
                let unknown = self.grid.read(cx).model(cx).commit_needs_reload();
                self.grid.update(cx, |g, cx| {
                    g.update_model(cx, |m| m.accept_committed_preserving_drafts());
                });
                if let Some(next) = hidden {
                    self.publish_hidden(next, cx);
                }
                if sync || reproject || leaving || unknown {
                    self.reload(true, cx);
                } else {
                    self.form_for = None;
                    if std::mem::take(&mut self.add_after_commit) {
                        self.insert_row(cx);
                    }
                }
            }
            Err(e) => {
                self.add_after_commit = false;
                self.reload_after_commit = false;
                self.close_after_commit = false;
                self.hidden_after_commit = None;
                self.status = e.clone().into();
                window.open_alert_dialog(cx, move |alert, _, _| {
                    alert
                        .icon(lucide("circle-x"))
                        .title(t!("viewer.commit_failed").to_string())
                        .description(e.clone())
                });
            }
        }
        cx.notify();
    }

    pub(crate) fn run_simple(
        &mut self,
        sql: &'static str,
        after: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let generation = self.session_generation;
        self.transaction_pending = true;
        let job = run_on(session.clone(), move |rel| {
            Box::pin(async move { rel.execute(sql).await })
        });
        // Not in `self.task`: replacing that drops the page load in flight (opening a table with
        // "auto begin transaction" sends BEGIN right after the first SELECT)
        cx.spawn(async move |this, cx| {
            let result = job
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r.map_err(|e| error_message(&e)));
            let _ = this.update(cx, |this, cx| {
                if !this.generation_matches(generation, cx)
                    || !this
                        .session
                        .as_ref()
                        .is_some_and(|current| Arc::ptr_eq(current, &session))
                {
                    return;
                }
                this.transaction_pending = false;
                match result {
                    Ok(_) => after(this, cx),
                    Err(e) => this.status = e.into(),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
