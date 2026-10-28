//! Editor lifecycle, connection sessions and the toolbar's picked context.

use std::sync::Arc;

use super::*;

use gpui_kit::component::WindowExt as _;
use gpui_kit::{App, Context, Window};
use navidog_app_state::actions::OpenConnectionId;
use navidog_app_state::store::ConnectionStore;
use navidog_app_state::workspace::Workspace;
use navidog_session::sessions::{Sessions, SessionsEvent};
use navidog_sql::dialect::Dialect;
use navidog_ui::assets::lucide;
use navidog_ui::tabs;
use rust_i18n::t;

use crate::run::RunKind;

impl QueryEditor {
    /// Runs everything (the navigator's "Open Query").
    pub fn run_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run(RunKind::All, window, cx);
    }

    pub fn display_name(&self) -> String {
        match &self.source {
            QuerySource::Untitled(1) => t!("query.untitled").to_string(),
            QuerySource::Untitled(n) => format!("{} {n}", t!("query.untitled")),
            QuerySource::Saved(o) => o.name.clone(),
            QuerySource::External(path) => path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        }
    }

    pub fn source(&self) -> &QuerySource {
        &self.source
    }

    pub fn target(&self) -> &Target {
        &self.target
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Explains everything (the toolbar's "Explain").
    pub fn explain_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run(RunKind::Explain, window, cx);
    }

    pub(crate) fn dialect(&self, cx: &App) -> Option<Arc<dyn Dialect>> {
        let id = self.target.connection.as_deref()?;
        Sessions::global(cx).read(cx).dialect(id).or_else(|| {
            let driver = ConnectionStore::global(cx).read(cx).get(id)?.driver.clone();
            navidog_session::DriverRegistry::dialect(cx, &driver)
        })
    }

    pub(crate) fn on_sessions_event(
        &mut self,
        e: &SessionsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(conn) = self.target.connection.clone() else {
            return;
        };
        match e {
            SessionsEvent::Opened(id) if *id == conn => {
                self.load_schemas(cx);
                self.ensure_outline(false, cx);
                if let Some(kind) = self.pending_run.take() {
                    self.run(kind, window, cx);
                }
                cx.notify();
            }
            SessionsEvent::Failed(id, _) if *id == conn => {
                self.pending_run = None;
                cx.notify();
            }
            // A query is a document: closing the connection only drops this editor's session,
            // the next run opens the connection again (30 §2)
            SessionsEvent::Closed(id) | SessionsEvent::Reopening(id) if *id == conn => {
                if let Some(live) = self.live.take() {
                    live.close();
                }
                cx.notify();
            }
            _ => {}
        }
    }

    pub(crate) fn pick_connection(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.target.connection.as_deref() == Some(id.as_str()) {
            return;
        }
        let default_db = ConnectionStore::global(cx)
            .read(cx)
            .get(&id)
            .and_then(|c| c.general.database.clone())
            .filter(|d| !d.is_empty());
        self.target = Target {
            connection: Some(id.clone()),
            database: default_db,
            schema: None,
        };
        self.schemas.clear();
        if let Some(live) = self.live.take() {
            live.close();
        }
        if !Sessions::global(cx).read(cx).state(&id).is_open() {
            window.dispatch_action(Box::new(OpenConnectionId { id: id.into() }), cx);
        } else {
            self.load_schemas(cx);
        }
        self.target_changed(cx);
        cx.notify();
    }

    pub(crate) fn pick_database(&mut self, name: String, cx: &mut Context<Self>) {
        if self.target.database.as_deref() == Some(name.as_str()) {
            return;
        }
        self.target.database = Some(name);
        self.target.schema = None;
        self.schemas.clear();
        self.load_schemas(cx);
        self.ensure_outline(false, cx);
        self.target_changed(cx);
        cx.notify();
    }

    pub(crate) fn pick_schema(&mut self, name: String, cx: &mut Context<Self>) {
        self.target.schema = Some(name);
        self.ensure_outline(false, cx);
        self.target_changed(cx);
        cx.notify();
    }

    /// PostgreSQL: lists the schemas of the picked database; picks `public` when none is set.
    pub(crate) fn load_schemas(&mut self, cx: &mut Context<Self>) {
        let (Some(conn), Some(db)) = (self.target.connection.clone(), self.target.database.clone())
        else {
            return;
        };
        if !self.dialect(cx).is_some_and(|d| d.features().has_schemas) {
            return;
        }
        let Some(job) = Sessions::global(cx)
            .read(cx)
            .run_rel(&conn, Some(&db), |r| r.list_schemas())
        else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let Ok(Ok(list)) = job.await else { return };
            let _ = this.update(cx, |this, cx| {
                let user: Vec<String> = list
                    .into_iter()
                    .filter(|s| !this.dialect(cx).is_some_and(|d| d.is_system_schema(s)))
                    .collect();
                if this.target.schema.is_none() && user.iter().any(|s| s == "public") {
                    this.target.schema = Some("public".into());
                    this.ensure_outline(false, cx);
                    this.target_changed(cx);
                }
                this.schemas = user;
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn ask_to_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.dirty {
            self.close_now(window, cx);
            return;
        }
        let this = cx.entity();
        let panel = PanelId::from(cx.entity_id());
        let name = self.display_name();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (save, discard, enter) = (this.clone(), this.clone(), this.clone());
            alert
                .icon(lucide("info"))
                .title(t!("query.close_unsaved", name = name).to_string())
                .on_ok(move |_, window, cx| {
                    enter.update(cx, |e, cx| e.save_and_close(window, cx));
                    true
                })
                .on_cancel(move |_, _, cx| {
                    tabs::abort_pending_dirty_closes(panel, cx);
                    true
                })
                .footer(navidog_ui::skin::close_prompt_footer(
                    "query-close",
                    t!("query.cancel").to_string(),
                    t!("query.dont_save").to_string(),
                    t!("query.save").to_string(),
                    move |_, window, cx| {
                        tabs::abort_pending_dirty_closes(panel, cx);
                        window.close_dialog(cx);
                    },
                    move |_, window, cx| {
                        window.close_dialog(cx);
                        discard.update(cx, |e, cx| {
                            crate::recovery::forget(&e.id);
                            e.close_now(window, cx)
                        });
                    },
                    move |_, window, cx| {
                        window.close_dialog(cx);
                        save.update(cx, |e, cx| e.save_and_close(window, cx));
                    },
                ))
        });
    }

    /// Saving an untitled query asks for a name first; the tab stays until it is saved.
    fn save_and_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_query(true, window, cx);
    }

    pub(crate) fn close_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_approved = true;
        let close = tabs::CloseTabs {
            panel: Some(PanelId::from(cx.entity_id())),
            which: tabs::Close::This,
        };
        window.dispatch_action(Box::new(close), cx);
    }

    /// Tells the info pane where this (active) editor points.
    pub(crate) fn publish_context(&self, cx: &mut Context<Self>) {
        let context = navidog_app_state::workspace::QueryContext {
            connection: self.target.connection.clone(),
            database: self.target.database.clone(),
            schema: self.target.schema.clone(),
        };
        Workspace::global(cx).update(cx, |w, cx| w.set_query_context(Some(context), cx));
    }

    fn target_changed(&mut self, cx: &mut Context<Self>) {
        let active = active_editor(cx).is_some_and(|e| e.entity_id() == cx.entity_id());
        if active {
            self.publish_context(cx);
        }
    }

    pub(crate) fn connection_name(&self, cx: &App) -> String {
        self.target
            .connection
            .as_ref()
            .and_then(|id| ConnectionStore::global(cx).read(cx).get(id))
            .map(|c| c.name.clone())
            .unwrap_or_default()
    }
}
