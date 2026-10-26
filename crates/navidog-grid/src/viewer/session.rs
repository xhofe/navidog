use super::*;

use std::sync::Arc;
use std::time::Instant;

use gpui_kit::{App, Context, SharedString};
use navidog_app_state::settings::Settings;
use navidog_db::dialect::Dialect;
use navidog_db::{CancelHandle, ColumnInfo};
use navidog_session::sessions::{ConnState, Sessions, SharedSession, run_on};

use crate::commit::{close_session, error_message};
use crate::grid::GridTarget;
use crate::model::Cell;

impl TableViewer {
    pub(crate) fn connect(&mut self, cx: &mut Context<Self>) {
        let sessions = Sessions::global(cx).read(cx);
        if !matches!(
            sessions.state(&self.object.scope.connection),
            ConnState::Open { .. }
        ) {
            return;
        }
        let Some(job) = sessions.new_session(
            &self.object.scope.connection,
            Some(&self.object.scope.database),
        ) else {
            return;
        };
        let generation = sessions.generation(&self.object.scope.connection);
        let (scope, table) = (self.object.scope.clone(), self.object.name.clone());
        self.set_busy(true, cx);
        self.task = Some(cx.spawn(async move |this, cx| {
            let session = match job.await {
                Ok(Ok(s)) => s,
                Ok(Err(e)) => {
                    return this
                        .update(cx, |t, cx| {
                            if t.generation_matches(generation, cx) {
                                t.fail(error_message(&e), cx);
                            }
                        })
                        .unwrap_or(());
                }
                Err(e) => {
                    return this
                        .update(cx, |t, cx| {
                            if t.generation_matches(generation, cx) {
                                t.fail(e.to_string(), cx);
                            }
                        })
                        .unwrap_or(());
                }
            };
            let cancel = session.cancel_handle();
            let shared: SharedSession = std::sync::Arc::new(tokio::sync::Mutex::new(Some(session)));
            if !this
                .update(cx, |this, cx| this.generation_matches(generation, cx))
                .unwrap_or(false)
            {
                close_session(shared);
                return;
            }
            let columns = run_on(shared.clone(), move |rel| {
                Box::pin(async move {
                    let d = rel.dialect();
                    Ok((d, rel.table_columns(&scope, &table).await?))
                })
            })
            .await;
            let pending_close = this
                .update(cx, |this, cx| {
                    this.apply_connected(generation, columns, shared, cancel, cx)
                })
                .unwrap_or(false);
            if pending_close {
                cx.update(|cx| TableViewer::resume_close(this.clone(), cx));
            }
        }));
    }

    /// Catalog columns and the viewer's own session, then the first page. Returns whether a
    /// close that waited on this load should continue.
    pub(crate) fn apply_connected(
        &mut self,
        generation: u64,
        columns: Result<navidog_db::DbResult<(Dialect, Vec<ColumnInfo>)>, tokio::task::JoinError>,
        shared: SharedSession,
        cancel: Option<CancelHandle>,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.generation_matches(generation, cx) {
            close_session(shared);
            return false;
        }
        self.session_generation = generation;
        match columns {
            Ok(Ok((dialect, columns))) => {
                self.dialect = Some(dialect);
                self.columns = columns;
                self.grid.update(cx, |g, cx| {
                    g.set_target(
                        Some(GridTarget {
                            dialect,
                            scope: self.object.scope.clone(),
                            table: self.object.name.clone(),
                        }),
                        cx,
                    )
                });
            }
            Ok(Err(e)) => self.status = error_message(&e).into(),
            Err(e) => self.status = e.to_string().into(),
        }
        self.session = Some(shared);
        self.cancel = cancel;
        self.load_foreign_keys(cx);
        // Back after a reopen with edits pending: keep the rows they belong to; they
        // commit on the new session. A reload would discard them.
        if self.grid.read(cx).model(cx).has_changes() {
            self.set_busy(false, cx);
            self.status = SharedString::default();
        } else {
            self.reload(true, cx);
        }
        if Settings::global(cx).auto_begin_transaction {
            self.begin_transaction(cx);
        }
        self.close_after_commit && !self.busy
    }

    pub(crate) fn load_foreign_keys(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let generation = self.session_generation;
        let (scope, table) = (self.object.scope.clone(), self.object.name.clone());
        let revision = self.catalog_revision;
        let job = run_on(session.clone(), move |rel| {
            Box::pin(async move { rel.foreign_keys(&scope, &table).await })
        });
        cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                if !this.generation_matches(generation, cx) {
                    return;
                }
                let Ok(Ok(keys)) = result else {
                    return;
                };
                if this.catalog_revision != revision
                    || !this
                        .session
                        .as_ref()
                        .is_some_and(|current| Arc::ptr_eq(current, &session))
                {
                    return;
                }
                let names = keys.iter().map(|key| key.column.clone()).collect();
                this.foreign_keys = keys;
                this.grid
                    .update(cx, |grid, cx| grid.set_foreign_keys(names, cx));
            });
        })
        .detach();
    }

    pub(crate) fn write_cell(
        &mut self,
        row: usize,
        col: usize,
        value: Cell,
        cx: &mut Context<Self>,
    ) {
        self.grid.update(cx, |grid, cx| {
            grid.update_model(cx, |model| model.set_value(row, col, value));
        });
    }

    /// `busy` also locks the grid: an edit made while a commit or a load runs would be lost
    /// when it finishes (see `GridModel::locked`).
    pub(crate) fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        self.busy = busy;
        self.grid.update(cx, |g, cx| g.set_locked(busy, cx));
        if busy {
            self.spin_until = Some(Instant::now() + REFRESH_SPIN);
            return;
        }
        if self.external_refresh_pending {
            let viewer = cx.entity().downgrade();
            cx.defer(move |cx| {
                let _ = viewer.update(cx, |this, cx| {
                    if this.external_refresh_pending && !this.busy {
                        this.refresh_after_external_sql(cx);
                    }
                });
            });
        }
        let Some(until) = self.spin_until else {
            return;
        };
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            self.spin_until = None;
            return;
        }
        // The load can finish before half a turn; wake the view when that half ends.
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(left).await;
            let _ = this.update(cx, |this, cx| {
                if this.busy || this.spin_until.is_some_and(|t| Instant::now() < t) {
                    return;
                }
                this.spin_until = None;
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn spinning(&self) -> bool {
        refresh_spinning(self.busy, self.spin_until, Instant::now())
    }

    pub(crate) fn generation_matches(&self, generation: u64, cx: &App) -> bool {
        Sessions::global(cx)
            .read(cx)
            .generation(&self.object.scope.connection)
            == generation
    }

    pub(crate) fn fail(&mut self, message: String, cx: &mut Context<Self>) {
        self.set_busy(false, cx);
        self.status = message.into();
        cx.notify();
    }

    /// Loads the current page; with `count`, also queries the total row count (when the filter changes).
    pub(crate) fn reload(&mut self, count: bool, cx: &mut Context<Self>) {
        let (Some(session), Some(d)) = (self.session.clone(), self.dialect) else {
            return;
        };
        let generation = self.session_generation;
        let (scope, table) = (&self.object.scope, &self.object.name);
        let filter = self.filter.read(cx).value().to_string();
        let order = self.order.read(cx).value().to_string();
        let offset = self.page * self.limit.unwrap_or(0);
        let sql = d.select_page(
            scope,
            table,
            &filter,
            &order,
            self.limit,
            offset,
            self.grid.read(cx).show_rowid,
        );
        let count_sql = count.then(|| d.count(scope, table, &filter));
        let metadata = self.metadata_stale;
        let (scope, table) = (scope.clone(), table.clone());
        self.status = sql.clone().into();
        self.set_busy(true, cx);
        cx.notify();
        let page = run_on(session.clone(), move |rel| {
            Box::pin(async move {
                let catalog = if metadata {
                    Some((
                        rel.table_columns(&scope, &table).await?,
                        rel.foreign_keys(&scope, &table).await?,
                    ))
                } else {
                    None
                };
                Ok((catalog, rel.execute(&sql).await?))
            })
        });
        let total = count_sql.map(|sql| {
            run_on(session, move |rel| {
                Box::pin(async move { rel.execute(&sql).await })
            })
        });
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = page
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r.map_err(|e| error_message(&e)));
            let total = if let Some(total) = total {
                Some(
                    total
                        .await
                        .ok()
                        .and_then(|r| r.ok())
                        .and_then(|sets| first_cell(&sets)),
                )
            } else {
                None
            };
            let pending_close = this
                .update(cx, |this, cx| {
                    if !this.generation_matches(generation, cx) {
                        return false;
                    }
                    if let Some(total) = total {
                        this.total = total;
                    }
                    let result = result.map(|(catalog, page)| {
                        if let Some((columns, foreign_keys)) = catalog {
                            this.accept_catalog(columns, foreign_keys, cx);
                        }
                        page
                    });
                    this.apply_page(generation, result, cx)
                })
                .unwrap_or(false);
            if pending_close {
                cx.update(|cx| TableViewer::resume_close(this.clone(), cx));
            }
        }));
    }
}
