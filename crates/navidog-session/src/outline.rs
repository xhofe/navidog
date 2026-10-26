//! Cached code completion info (30-query-editor.md §4): JSON on disk and one
//! in-memory entry per scope. Each database session fetches its own outline.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::{App, AppContext as _, Context, Entity, Global};
use navidog_app_state::settings::Settings;
pub use navidog_db::outline::{Outline, OutlineTable};
use navidog_model::json_store::JsonStore;
use navidog_model::object::Scope;
use navidog_model::queries::encode_segment;

use crate::sessions::Sessions;

fn path_in(root: &Path, scope: &Scope) -> PathBuf {
    // A leading dot cannot clash with a database folder: those names encode `.` as `%2E`.
    let base = navidog_model::paths::profile_dir_in(root, &scope.connection)
        .join(".outline")
        .join(encode_segment(&scope.database));
    match &scope.schema {
        Some(s) => base.join(format!("{}.json", encode_segment(s))),
        None => base.with_extension("json"),
    }
}

pub fn path(scope: &Scope) -> PathBuf {
    path_in(&navidog_model::paths::config_dir(), scope)
}

pub fn load(scope: &Scope) -> Option<Outline> {
    JsonStore::new(path(scope)).load_optional()
}

pub fn save(scope: &Scope, outline: &Outline) {
    JsonStore::new(path(scope)).save_lossy(outline);
}

fn clear_files() {
    let root = navidog_model::paths::config_dir();
    if let Ok(entries) = std::fs::read_dir(root.join("profiles")) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_dir_all(entry.path().join(".outline"));
        }
    }
}

pub type OutlineState = crate::scope_cache::Load<Arc<Outline>>;

/// Outlines by scope, for the query editor's completion and the info pane's identifiers page.
#[derive(Default)]
pub struct OutlineCache {
    cache: crate::scope_cache::ScopeCache<Scope, Arc<Outline>>,
}

struct GlobalOutlines(Entity<OutlineCache>);
impl Global for GlobalOutlines {}

impl OutlineCache {
    pub fn global(cx: &App) -> Entity<Self> {
        cx.global::<GlobalOutlines>().0.clone()
    }

    pub fn get(&self, scope: &Scope) -> Option<&OutlineState> {
        self.cache.get(scope)
    }

    pub fn ready(&self, scope: &Scope) -> Option<Arc<Outline>> {
        self.cache.ready(scope)
    }

    /// Makes the outline available: with "auto update" (or `refresh`) it is fetched again;
    /// otherwise the copy on disk is used, and fetched only when there is none.
    pub fn ensure(&mut self, scope: &Scope, refresh: bool, cx: &mut Context<Self>) {
        let s = Settings::global(cx);
        let (auto, system) = (s.auto_update_outline, s.outline_system_objects);
        if !refresh && self.cache.task_pending(scope) {
            return;
        }
        let fetch_now =
            refresh || (auto && !matches!(self.cache.get(scope), Some(OutlineState::Ready(_))));
        if !fetch_now {
            if self.cache.contains(scope) {
                return;
            }
            if let Some(o) = load(scope) {
                self.cache
                    .insert(scope.clone(), OutlineState::Ready(Arc::new(o)));
                cx.notify();
                return;
            }
        }
        let target = scope.clone();
        let job = Sessions::global(cx).read(cx).run_rel(
            &scope.connection,
            Some(&scope.database),
            move |rel| Box::pin(async move { rel.outline(&target, system).await }),
        );
        let Some(job) = job else { return };
        if !self.cache.contains(scope) {
            self.cache.insert_loading(scope.clone());
        }
        let key = scope.clone();
        let task = cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                // Dropped with the connection, or cleared: do not write the file back.
                if !this.cache.finish_task(&key) {
                    return;
                }
                let state = match result {
                    Ok(Ok(o)) => {
                        save(&key, &o);
                        OutlineState::Ready(Arc::new(o))
                    }
                    Ok(Err(e)) => OutlineState::Failed(e.to_string().into()),
                    Err(e) => OutlineState::Failed(e.to_string().into()),
                };
                // A failed refresh keeps the outline it had
                if !(matches!(state, OutlineState::Failed(_))
                    && matches!(this.cache.get(&key), Some(OutlineState::Ready(_))))
                {
                    this.cache.insert(key, state);
                }
                cx.notify();
            });
        });
        self.cache.set_task(scope.clone(), task);
        cx.notify();
    }

    /// "Clear Code Completion Info": every file and everything in memory.
    pub fn clear_all(&mut self, cx: &mut Context<Self>) {
        self.cache.clear();
        clear_files();
        cx.notify();
    }

    /// Drops cached outlines for a connection that is about to be deleted, so a finished fetch
    /// does not recreate `.outline` after the profile directory is removed.
    pub fn forget_connection(&mut self, id: &str, cx: &mut Context<Self>) {
        self.cache.retain(|scope| scope.connection != id);
        cx.notify();
    }

    pub fn drop_connection(id: &str, cx: &mut App) {
        if !cx.has_global::<GlobalOutlines>() {
            return;
        }
        Self::global(cx).update(cx, |cache, cx| cache.forget_connection(id, cx));
    }
}

pub(crate) fn init(cx: &mut App) {
    let cache = cx.new(|_| OutlineCache::default());
    cx.set_global(GlobalOutlines(cache));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_encode_names_inside_the_connection_folder() {
        let root = Path::new("/cfg");
        let s = Scope::new("c1", "a/b", Some("public".into()));
        let p = path_in(root, &s);
        assert!(
            p.ends_with("profiles/c1/.outline/a%2Fb/public.json"),
            "{p:?}"
        );
        let m = Scope::new("c1", "sakila", None);
        assert!(path_in(root, &m).ends_with("profiles/c1/.outline/sakila.json"));
        let dotted = Scope::new("c1", ".outline", None);
        assert!(path_in(root, &dotted).ends_with("profiles/c1/.outline/%2Eoutline.json"));
    }

    #[gpui_kit::test]
    fn outline_on_disk_is_used_when_auto_update_is_off_and_clear_removes_it(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        let _guard = navidog_db::lock_config_home();
        let dir = std::env::temp_dir().join(format!("navidog-outline-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: `LOCK` is held until this test returns, and this binary has no other writer.
        unsafe { std::env::set_var("NAVIDOG_HOME", &dir) };

        let scope = Scope::new("c1", "sakila", None);
        let outline = Outline {
            tables: vec![OutlineTable {
                name: "actor".into(),
                view: false,
                columns: vec![("actor_id".into(), "smallint".into())],
            }],
            ..Outline::default()
        };
        save(&scope, &outline);
        assert_eq!(
            load(&scope).unwrap().table("actor").unwrap().columns,
            vec![("actor_id".into(), "smallint".into())]
        );

        let cache = cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_global(navidog_app_state::settings::Settings {
                auto_update_outline: false,
                ..navidog_app_state::settings::Settings::default()
            });
            cx.new(|_| OutlineCache::default())
        });
        cache.update(cx, |cache, cx| {
            cache.ensure(&scope, false, cx);
            assert_eq!(
                cache.ready(&scope).unwrap().table("actor").unwrap().name,
                "actor"
            );
            assert!(
                !cache.cache.has_pending_tasks(),
                "a disk hit must not fetch"
            );
            cache.clear_all(cx);
            assert!(cache.ready(&scope).is_none());
        });
        assert!(load(&scope).is_none());
    }
}
