//! Run state, messages, summary rows, the result tab list and the profile/status grids.

use super::*;

use std::time::Duration;

use gpui_kit::{App, Context, Entity, Window};
use navidog_app_state::settings::{Settings, ShowAfterRun};
use navidog_db::{Column, ResultSet};
use navidog_grid::{Cell, GridColumn, ResultGrid};
use navidog_util::sql_text;
use rust_i18n::t;

use crate::errors::error_message;
use crate::run::Outcome;

impl Results {
    pub(crate) fn has_run(&self) -> bool {
        self.started.is_some()
    }

    /// Each run drops the tabs that are not pinned, unless `keep` is set (31 §2).
    pub(crate) fn begin_run(&mut self, keep: bool, cx: &mut Context<QueryEditor>) {
        if !keep {
            let dropped: Vec<u64> = self
                .tabs
                .iter()
                .filter(|t| !t.pinned)
                .map(|t| t.id)
                .collect();
            for id in dropped {
                self.subscriptions.remove(&id);
            }
            self.tabs.retain(|t| t.pinned);
        }
        self.run_floor = self.next_id;
        self.rows.clear();
        self.messages_text.clear();
        self.results_this_run = 0;
        self.explains_this_run = 0;
        self.profile = None;
        self.status = None;
        self.started = Some(chrono::Local::now());
        self.ended = None;
        if !matches!(self.page, Page::Tab(id) if self.tabs.iter().any(|t| t.id == id)) {
            self.page = Page::Messages;
        }
        cx.notify();
    }

    pub(crate) fn failed_to_start(
        &mut self,
        error: &str,
        window: &mut Window,
        cx: &mut Context<QueryEditor>,
    ) {
        self.messages_text.push_str(&format!("> {error}\n"));
        self.sync_messages(window, cx);
        self.page = Page::Messages;
    }

    fn sync_messages(&mut self, window: &mut Window, cx: &mut Context<QueryEditor>) {
        let text = self.messages_text.clone();
        self.messages
            .update(cx, |e, cx| e.set_value(text, window, cx));
    }

    /// Writes a statement to the messages and summary pages.
    pub(crate) fn note(
        &mut self,
        outcome: Outcome,
        window: &mut Window,
        cx: &mut Context<QueryEditor>,
    ) {
        self.note_outcome(&outcome, window, cx);
    }

    fn note_outcome(
        &mut self,
        outcome: &Outcome,
        window: &mut Window,
        cx: &mut Context<QueryEditor>,
    ) {
        let Outcome {
            planned,
            result,
            elapsed,
            ..
        } = outcome;
        self.messages_text
            .push_str(&message_block(&planned.shown, result, *elapsed));
        self.sync_messages(window, cx);
        self.rows.push(SummaryRow {
            sql: planned.shown.clone(),
            message: summary_message(result),
            elapsed: *elapsed,
            error: result.is_err(),
            range: planned.range.clone(),
        });
    }

    pub(crate) fn record(
        &mut self,
        outcome: Outcome,
        info: RunInfo,
        editor: WeakEntity<QueryEditor>,
        window: &mut Window,
        cx: &mut Context<QueryEditor>,
    ) {
        self.note_outcome(&outcome, window, cx);
        let Outcome {
            planned, result, ..
        } = outcome;
        let Ok(sets) = result else { return };
        for set in sets.into_iter().filter(|s| !s.columns.is_empty()) {
            if self.results_this_run >= MAX_RESULTS {
                break;
            }
            self.results_this_run += 1;
            let name = sql_text::name_hint(&planned.shown)
                .unwrap_or_else(|| t!("query.result_n", n = self.results_this_run).to_string());
            let view = ResultView::new(
                set,
                planned.sql.clone(),
                info.clone(),
                editor.clone(),
                window,
                cx,
            );
            let id = self.add_tab(
                name,
                Body::Grid(view.clone()),
                info.clone(),
                planned.sql.clone(),
            );
            let sub = cx.subscribe(
                &view,
                move |this: &mut QueryEditor, _, e: &ResultEvent, cx| match e {
                    ResultEvent::TogglePin => this.results.toggle_pin(id, cx),
                },
            );
            self.subscriptions.insert(id, sub);
        }
    }

    pub(crate) fn add_tab(&mut self, name: String, body: Body, info: RunInfo, sql: String) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.tabs.push(Tab {
            id,
            name,
            pinned: false,
            body,
            info,
            sql,
        });
        id
    }

    pub(crate) fn next_explain_name(&mut self) -> String {
        self.explains_this_run += 1;
        t!("query.explain_n", n = self.explains_this_run).to_string()
    }

    /// Picks the page "View > Show After Execution" asks for.
    pub(crate) fn end_run(&mut self, explain: bool, cx: &mut Context<QueryEditor>) {
        self.ended = Some(chrono::Local::now());
        let first_new = self
            .tabs
            .iter()
            .filter(|t| t.id > self.run_floor)
            .map(|t| t.id)
            .min();
        self.page = match Settings::global(cx).show_after_run {
            ShowAfterRun::Result => first_new.map(Page::Tab).unwrap_or(Page::Messages),
            ShowAfterRun::Messages if !explain => Page::Messages,
            ShowAfterRun::Summary if !explain => Page::Summary,
            _ => first_new.map(Page::Tab).unwrap_or(Page::Messages),
        };
        cx.notify();
    }

    pub(crate) fn show_profile(
        &mut self,
        profiled: Option<(ResultSet, Vec<(String, String)>)>,
        window: &mut Window,
        cx: &mut Context<QueryEditor>,
    ) {
        let Some((profile, changes)) = profiled else {
            return;
        };
        let status = ResultSet {
            columns: ["Variable_name", "Value"]
                .iter()
                .map(|n| Column {
                    name: n.to_string(),
                    ..Column::default()
                })
                .collect(),
            rows: changes
                .into_iter()
                .map(|(k, v)| vec![Some(k), Some(v)])
                .collect(),
            ..ResultSet::default()
        };
        self.profile = Some(read_only_grid(&profile, window, cx));
        self.status = Some(read_only_grid(&status, window, cx));
    }

    /// Pinned tabs sit at the far left in the order they were pinned (31 §4.2).
    pub(crate) fn toggle_pin(&mut self, id: u64, cx: &mut Context<QueryEditor>) {
        let Some(ix) = self.tabs.iter().position(|t| t.id == id) else {
            return;
        };
        let mut tab = self.tabs.remove(ix);
        tab.pinned = !tab.pinned;
        let pinned = tab.pinned;
        if let Body::Grid(view) = &tab.body {
            view.update(cx, |v, cx| {
                v.pinned = pinned;
                cx.notify();
            });
        }
        let at = self.tabs.iter().take_while(|t| t.pinned).count();
        self.tabs.insert(at, tab);
        cx.notify();
    }

    pub(crate) fn close_tab(&mut self, id: u64, cx: &mut Context<QueryEditor>) {
        self.tabs.retain(|t| t.id != id);
        self.subscriptions.remove(&id);
        if self.page == Page::Tab(id) {
            self.page = Page::Messages;
        }
        cx.notify();
    }

    pub(crate) fn page_exists(&self) -> bool {
        match self.page {
            Page::Tab(id) => self.tabs.iter().any(|t| t.id == id),
            Page::Profile => self.profile.is_some(),
            Page::Status => self.status.is_some(),
            _ => true,
        }
    }
}

fn read_only_grid(set: &ResultSet, window: &mut Window, cx: &mut App) -> Entity<ResultGrid> {
    let grid = cx.new(|cx| ResultGrid::new(window, cx));
    let (columns, rows) = grid_data(set);
    grid.update(cx, |g, cx| {
        g.set_quick_filter(false);
        g.set_data(columns, rows, cx);
        g.set_locked(true, cx);
    });
    grid
}

pub(super) fn grid_data(set: &ResultSet) -> (Vec<GridColumn>, Vec<Vec<Cell>>) {
    let columns = set
        .columns
        .iter()
        .map(|c| GridColumn {
            name: c.name.clone().into(),
            type_name: c.type_name.clone().into(),
            ..GridColumn::default()
        })
        .collect();
    let rows = set
        .rows
        .iter()
        .map(|r| r.iter().map(|v| v.clone().map(Into::into)).collect())
        .collect();
    (columns, rows)
}

/// One statement on the messages page (31 §4.3).
pub(super) fn message_block(
    sql: &str,
    result: &navidog_db::DbResult<Vec<ResultSet>>,
    elapsed: Duration,
) -> String {
    let mut out = format!("{}\n", sql.trim());
    match result {
        Ok(sets) => {
            for notice in sets.iter().flat_map(|s| &s.notices) {
                out.push_str(&format!("> {notice}\n"));
            }
            let affected: Option<u64> = sets
                .iter()
                .filter(|s| s.columns.is_empty())
                .filter_map(|s| s.affected_rows)
                .reduce(|a, b| a + b);
            match affected {
                Some(n) if sets.iter().all(|s| s.columns.is_empty()) => {
                    out.push_str(&format!("> {}: {n}\n", t!("query.affected_rows")))
                }
                _ => out.push_str("> OK\n"),
            }
        }
        Err(e) => out.push_str(&format!("> {}\n", error_message(e))),
    }
    out.push_str(&format!("> {}: {}\n\n", t!("query.time"), secs(elapsed)));
    out
}

pub(super) fn summary_message(result: &navidog_db::DbResult<Vec<ResultSet>>) -> String {
    match result {
        Ok(sets) if sets.iter().all(|s| s.columns.is_empty()) => {
            let n: u64 = sets.iter().filter_map(|s| s.affected_rows).sum();
            format!("{}: {n}", t!("query.affected_rows"))
        }
        Ok(_) => "OK".into(),
        Err(e) => error_message(e),
    }
}
