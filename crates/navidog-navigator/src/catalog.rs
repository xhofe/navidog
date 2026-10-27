//! Metadata cache: schemas, object lists, DDL, fields and table children. A pane states what it
//! needs with a [`Requirement`]; [`Catalog::require`] decides whether that starts a query. Loads
//! never start in render, and are invalidated when the connection closes or refreshes.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use gpui_kit::{App, AppContext as _, Context, Entity, Global};
use navidog_app_state::store::{ConnectionStore, StoreEvent};
use navidog_app_state::workspace::{Selection, Workspace, WorkspaceEvent};
use navidog_db::{ColumnInfo, DbResult, ObjectInfo, TableChildren};
use navidog_model::object::{ObjectKind, ObjectRef, Scope};
use navidog_session::scope_cache::ScopeCache;
use navidog_session::sessions::{Sessions, SessionsEvent};
use tokio::task::JoinHandle;

pub use navidog_session::scope_cache::Load;

/// Cache key of one database's schema list.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct SchemaKey {
    connection: String,
    database: String,
}

/// Cache key of one object category of a scope.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ObjectsKey {
    scope: Scope,
    kind: ObjectKind,
}

/// Cache key of a table's fields or children.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TableKey {
    scope: Scope,
    table: String,
}

/// One in-flight load, typed per cache: a fields load and a children load of the same table both
/// use a `(scope, table)` key and must not finish each other's entry.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum LoadId {
    Schemas(SchemaKey),
    Objects(ObjectsKey),
    Ddl(ObjectRef),
    Columns(TableKey),
    Children(TableKey),
}

impl LoadId {
    fn connection(&self) -> &str {
        match self {
            Self::Schemas(key) => &key.connection,
            Self::Objects(key) => &key.scope.connection,
            Self::Ddl(object) => &object.scope.connection,
            Self::Columns(key) | Self::Children(key) => &key.scope.connection,
        }
    }

    fn scope(&self) -> Option<&Scope> {
        match self {
            Self::Schemas(_) => None,
            Self::Objects(key) => Some(&key.scope),
            Self::Ddl(object) => Some(&object.scope),
            Self::Columns(key) | Self::Children(key) => Some(&key.scope),
        }
    }
}

/// What a pane shows from the metadata cache. Sending a requirement is how a pane asks for data;
/// whether a query starts is the cache's decision, so a pane never checks "is it loaded" itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Requirement {
    Schemas {
        connection: String,
        database: String,
    },
    Objects {
        scope: Scope,
        kind: ObjectKind,
    },
    Ddl(ObjectRef),
    Columns {
        scope: Scope,
        table: String,
    },
    Children {
        scope: Scope,
        table: String,
    },
}

#[derive(Default)]
pub struct Catalog {
    schemas: ScopeCache<SchemaKey, Vec<String>>,
    objects: ScopeCache<ObjectsKey, Vec<ObjectInfo>>,
    ddl: ScopeCache<ObjectRef, String>,
    columns: ScopeCache<TableKey, Vec<ColumnInfo>>,
    children: ScopeCache<TableKey, TableChildren>,
    /// One token per in-flight load, so a result from a load that an invalidation already
    /// replaced cannot overwrite the new one.
    generation: HashMap<LoadId, u64>,
    generation_clock: u64,
    /// Bumps on every cache change. Panes that draw from a snapshot (the
    /// navigator tree) compare this, because a catalog read during render
    /// makes gpui skip observers and only redraw the pane that read it.
    revision: u64,
}

struct GlobalCatalog(Entity<Catalog>);
impl Global for GlobalCatalog {}

impl Catalog {
    pub fn global(cx: &App) -> Entity<Self> {
        cx.global::<GlobalCatalog>().0.clone()
    }

    pub fn schemas(&self, conn: &str, db: &str) -> Option<&Load<Vec<String>>> {
        self.schemas.get(&SchemaKey {
            connection: conn.to_string(),
            database: db.to_string(),
        })
    }

    pub fn objects(&self, scope: &Scope, kind: ObjectKind) -> Option<&Load<Vec<ObjectInfo>>> {
        self.objects.get(&ObjectsKey {
            scope: scope.clone(),
            kind,
        })
    }

    pub fn object(&self, obj: &ObjectRef) -> Option<&ObjectInfo> {
        match self.objects(&obj.scope, obj.kind)? {
            Load::Ready(list) => list.iter().find(|o| o.name == obj.name),
            _ => None,
        }
    }

    pub fn ddl(&self, obj: &ObjectRef) -> Option<&Load<String>> {
        self.ddl.get(obj)
    }

    pub fn columns(&self, scope: &Scope, table: &str) -> Option<&Load<Vec<ColumnInfo>>> {
        self.columns.get(&TableKey {
            scope: scope.clone(),
            table: table.to_string(),
        })
    }

    pub fn table_children(&self, scope: &Scope, table: &str) -> Option<&Load<TableChildren>> {
        self.children.get(&TableKey {
            scope: scope.clone(),
            table: table.to_string(),
        })
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn bump(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Loads `requirements` that are not cached yet. A cached entry starts no query.
    pub fn require_all(
        &mut self,
        requirements: impl IntoIterator<Item = Requirement>,
        cx: &mut Context<Self>,
    ) {
        for requirement in requirements {
            self.require(requirement, cx);
        }
    }

    /// Loads a missing entry; a cached entry starts no query.
    pub fn require(&mut self, requirement: Requirement, cx: &mut Context<Self>) {
        self.require_with(requirement, false, cx);
    }

    /// Loads `requirements` again, even when they are cached (Refresh).
    pub fn reload_all(
        &mut self,
        requirements: impl IntoIterator<Item = Requirement>,
        cx: &mut Context<Self>,
    ) {
        for requirement in requirements {
            self.require_with(requirement, true, cx);
        }
    }

    fn require_with(&mut self, requirement: Requirement, force: bool, cx: &mut Context<Self>) {
        match requirement {
            Requirement::Schemas {
                connection,
                database,
            } => {
                let key = SchemaKey {
                    connection,
                    database,
                };
                let (c, d) = (key.connection.clone(), key.database.clone());
                let job = move |cx: &App| {
                    Sessions::global(cx)
                        .read(cx)
                        .run_rel(&c, Some(&d), |r| r.list_schemas())
                };
                self.start(
                    LoadId::Schemas(key.clone()),
                    key,
                    |c| &mut c.schemas,
                    job,
                    force,
                    cx,
                );
            }
            Requirement::Objects { scope, kind } => {
                let key = ObjectsKey { scope, kind };
                let id = LoadId::Objects(key.clone());
                // Saved queries are files in the profile folder: listed here, never by the driver
                if kind == ObjectKind::Query {
                    if force || !self.objects.contains(&key) {
                        let list = query_infos(&key.scope);
                        self.objects.insert(key, Load::Ready(list));
                        self.bump();
                        cx.notify();
                    }
                    return;
                }
                let (conn, db) = (key.scope.connection.clone(), key.scope.database.clone());
                let job_scope = key.scope.clone();
                let job = move |cx: &App| {
                    Sessions::global(cx)
                        .read(cx)
                        .run_rel(&conn, Some(&db), move |r| {
                            Box::pin(async move { r.list_objects(&job_scope, kind).await })
                        })
                };
                self.start(id, key, |c| &mut c.objects, job, force, cx);
            }
            Requirement::Ddl(object) => {
                let id = LoadId::Ddl(object.clone());
                let (conn, db) = (
                    object.scope.connection.clone(),
                    object.scope.database.clone(),
                );
                let job_object = object.clone();
                let job = move |cx: &App| {
                    Sessions::global(cx)
                        .read(cx)
                        .run_rel(&conn, Some(&db), move |r| {
                            Box::pin(async move {
                                r.object_ddl(&job_object.scope, job_object.kind, &job_object.name)
                                    .await
                            })
                        })
                };
                self.start(id, object, |c| &mut c.ddl, job, force, cx);
            }
            Requirement::Columns { scope, table } => {
                let key = TableKey { scope, table };
                let id = LoadId::Columns(key.clone());
                let (conn, db) = (key.scope.connection.clone(), key.scope.database.clone());
                let (job_scope, job_table) = (key.scope.clone(), key.table.clone());
                let job = move |cx: &App| {
                    Sessions::global(cx)
                        .read(cx)
                        .run_rel(&conn, Some(&db), move |r| {
                            Box::pin(async move { r.table_columns(&job_scope, &job_table).await })
                        })
                };
                self.start(id, key, |c| &mut c.columns, job, force, cx);
            }
            Requirement::Children { scope, table } => {
                let key = TableKey { scope, table };
                let id = LoadId::Children(key.clone());
                let (conn, db) = (key.scope.connection.clone(), key.scope.database.clone());
                let (job_scope, job_table) = (key.scope.clone(), key.table.clone());
                let job = move |cx: &App| {
                    Sessions::global(cx)
                        .read(cx)
                        .run_rel(&conn, Some(&db), move |r| {
                            Box::pin(async move { r.table_children(&job_scope, &job_table).await })
                        })
                };
                self.start(id, key, |c| &mut c.children, job, force, cx);
            }
        }
    }

    /// Loads the entries the workspace selection shows: a database's schema list, or the object
    /// list of the selected category. Panes no longer decide this themselves.
    pub fn require_selection(&mut self, cx: &mut Context<Self>) {
        let selection = Workspace::global(cx).read(cx).selection().clone();
        let mut requirements = Vec::new();
        if let Selection::Database(c, d) = &selection {
            match Sessions::global(cx).read(cx).dialect(c) {
                Some(dialect) if dialect.features().has_schemas => {
                    requirements.push(Requirement::Schemas {
                        connection: c.clone(),
                        database: d.clone(),
                    })
                }
                Some(_) => requirements.push(Requirement::Objects {
                    scope: Scope::new(c.clone(), d.clone(), None),
                    kind: ObjectKind::Table,
                }),
                None => {}
            }
        }
        if let Some((scope, kind)) = listed_kind(&selection) {
            requirements.push(Requirement::Objects { scope, kind });
        }
        self.require_all(requirements, cx);
    }

    /// `job` starts the query, so it runs only on a cache miss: `run_rel` begins executing as
    /// soon as it is called.
    fn start<K, T>(
        &mut self,
        id: LoadId,
        key: K,
        slot: fn(&mut Self) -> &mut ScopeCache<K, T>,
        job: impl FnOnce(&App) -> Option<JoinHandle<DbResult<T>>>,
        force: bool,
        cx: &mut Context<Self>,
    ) where
        K: Clone + Eq + Hash + 'static,
        T: Send + 'static,
    {
        if !force && slot(self).contains(&key) {
            return;
        }
        let Some(job) = job(cx) else { return };
        // The task marker only says "a load is in flight for this key". The generation token is
        // what tells a replaced load from the current one: after a forced reload both tasks see
        // the same key in the map, so only the token can keep the older result out.
        self.generation_clock = self.generation_clock.wrapping_add(1);
        let generation = self.generation_clock;
        self.generation.insert(id.clone(), generation);
        slot(self).insert_loading(key.clone());
        self.bump();
        let task_key = key.clone();
        let task = cx.spawn(async move |this, cx| {
            let result = match job.await {
                Ok(Ok(v)) => Load::Ready(v),
                Ok(Err(e)) => Load::Failed(crate::errors::database_message(&e).into()),
                Err(e) => Load::Failed(e.to_string().into()),
            };
            let _ = this.update(cx, |this, cx| {
                // A newer load, or an invalidation, owns this key now.
                if this.generation.get(&id) != Some(&generation) {
                    return;
                }
                this.generation.remove(&id);
                slot(this).finish_task(&task_key);
                slot(this).insert(task_key, result);
                this.bump();
                cx.notify();
            });
        });
        slot(self).set_task(key, task);
        cx.notify();
    }

    /// Drops caches for connections that no longer exist in the store.
    pub fn retain_connections(&mut self, live: &HashSet<String>, cx: &mut Context<Self>) {
        let mut changed = false;
        changed |= self.schemas.retain(|key| live.contains(&key.connection));
        changed |= self
            .objects
            .retain(|key| live.contains(&key.scope.connection));
        changed |= self.ddl.retain(|key| live.contains(&key.scope.connection));
        changed |= self
            .columns
            .retain(|key| live.contains(&key.scope.connection));
        changed |= self
            .children
            .retain(|key| live.contains(&key.scope.connection));
        changed |= self.retain_generations(|id| live.contains(id.connection()));
        if changed {
            self.bump();
            cx.notify();
        }
    }

    /// Drops everything cached for a connection (closed or refreshed).
    pub fn invalidate_connection(&mut self, conn: &str, cx: &mut Context<Self>) {
        let mut changed = false;
        changed |= self.schemas.retain(|key| key.connection != conn);
        changed |= self.objects.retain(|key| key.scope.connection != conn);
        changed |= self.ddl.retain(|key| key.scope.connection != conn);
        changed |= self.columns.retain(|key| key.scope.connection != conn);
        changed |= self.children.retain(|key| key.scope.connection != conn);
        changed |= self.retain_generations(|id| id.connection() != conn);
        if changed {
            self.bump();
            cx.notify();
        }
    }

    /// Drops the in-flight tokens that `keep` rejects; see [`Catalog::retain_connections`].
    fn retain_generations(&mut self, mut keep: impl FnMut(&LoadId) -> bool) -> bool {
        let before = self.generation.len();
        self.generation.retain(|id, _| keep(id));
        self.generation.len() != before
    }

    /// Reloads what is loaded under a scope (a database or schema).
    pub fn refresh_scope(&mut self, scope: &Scope, cx: &mut Context<Self>) {
        let kinds: Vec<ObjectKind> = self
            .objects
            .keys()
            .filter(|key| &key.scope == scope)
            .map(|key| key.kind)
            .collect();
        let tables: Vec<String> = self
            .columns
            .keys()
            .filter(|key| &key.scope == scope)
            .map(|key| key.table.clone())
            .collect();
        let children: Vec<String> = self
            .children
            .keys()
            .filter(|key| &key.scope == scope)
            .map(|key| key.table.clone())
            .collect();
        // Open DDL views read the cache, so a scope refresh must reload them instead of only
        // dropping their entries.
        let ddl: Vec<ObjectRef> = self
            .ddl
            .keys()
            .filter(|key| &key.scope == scope)
            .cloned()
            .collect();
        let mut changed = false;
        changed |= self.ddl.retain(|key| &key.scope != scope);
        changed |= self.columns.retain(|key| &key.scope != scope);
        changed |= self.children.retain(|key| &key.scope != scope);
        changed |= self.retain_generations(|id| id.scope() != Some(scope));
        if changed {
            self.bump();
        }
        for object in ddl {
            self.require_with(Requirement::Ddl(object), true, cx);
        }
        for kind in kinds {
            self.require_with(
                Requirement::Objects {
                    scope: scope.clone(),
                    kind,
                },
                true,
                cx,
            );
        }
        for table in tables {
            self.require_with(
                Requirement::Columns {
                    scope: scope.clone(),
                    table,
                },
                true,
                cx,
            );
        }
        for table in children {
            self.require_with(
                Requirement::Children {
                    scope: scope.clone(),
                    table,
                },
                true,
                cx,
            );
        }
        if scope.schema.is_none() && self.schemas(&scope.connection, &scope.database).is_some() {
            self.require_with(
                Requirement::Schemas {
                    connection: scope.connection.clone(),
                    database: scope.database.clone(),
                },
                true,
                cx,
            );
        }
    }

    /// Reloads existing entries after SQL on this connection, without expanding unopened nodes.
    fn refresh_connection(&mut self, conn: &str, cx: &mut Context<Self>) {
        // The catalog only owns metadata; the shell decides whether a database
        // switch also refreshes the session (see `init` below).
        let schemas: Vec<(String, String)> = self
            .schemas
            .keys()
            .filter(|key| key.connection == conn)
            .map(|key| (key.connection.clone(), key.database.clone()))
            .collect();
        let objects: Vec<(Scope, ObjectKind)> = self
            .objects
            .keys()
            .filter(|key| key.scope.connection == conn && key.kind != ObjectKind::Query)
            .map(|key| (key.scope.clone(), key.kind))
            .collect();
        let ddl: Vec<ObjectRef> = self
            .ddl
            .keys()
            .filter(|object| object.scope.connection == conn)
            .cloned()
            .collect();
        let columns: Vec<(Scope, String)> = self
            .columns
            .keys()
            .filter(|key| key.scope.connection == conn)
            .map(|key| (key.scope.clone(), key.table.clone()))
            .collect();
        let children: Vec<(Scope, String)> = self
            .children
            .keys()
            .filter(|key| key.scope.connection == conn)
            .map(|key| (key.scope.clone(), key.table.clone()))
            .collect();
        for (connection, database) in schemas {
            self.require_with(
                Requirement::Schemas {
                    connection,
                    database,
                },
                true,
                cx,
            );
        }
        for (scope, kind) in objects {
            self.require_with(Requirement::Objects { scope, kind }, true, cx);
        }
        for object in ddl {
            self.require_with(Requirement::Ddl(object), true, cx);
        }
        for (scope, table) in columns {
            self.require_with(Requirement::Columns { scope, table }, true, cx);
        }
        for (scope, table) in children {
            self.require_with(Requirement::Children { scope, table }, true, cx);
        }
    }
}

/// The object category to show for the selection's scope: the selected category or the selected
/// object's category, otherwise tables by default.
pub(crate) fn listed_kind(selection: &Selection) -> Option<(Scope, ObjectKind)> {
    match selection {
        Selection::Category(s, k) => Some((s.clone(), *k)),
        Selection::Object(o) => Some((o.scope.clone(), o.kind)),
        Selection::Schema(s) => Some((s.clone(), ObjectKind::Table)),
        _ => None,
    }
}

/// Saved queries of a scope as object rows: name, modification date and size (30 §7).
fn query_infos(scope: &Scope) -> Vec<ObjectInfo> {
    navidog_model::queries::list(scope)
        .into_iter()
        .map(|q| {
            let mut properties = Vec::new();
            if let Some(t) = q.modified {
                let t: chrono::DateTime<chrono::Local> = t.into();
                properties.push(("modified".into(), t.format("%Y-%m-%d %H:%M:%S").to_string()));
            }
            properties.push(("data_length".into(), q.size.to_string()));
            ObjectInfo {
                name: q.name,
                kind: ObjectKind::Query,
                comment: None,
                properties,
            }
        })
        .collect()
}

pub(crate) fn init(cx: &mut App) {
    let catalog = cx.new(|cx: &mut Context<Catalog>| {
        cx.subscribe(
            &Workspace::global(cx),
            |this, _, event: &WorkspaceEvent, cx| match event {
                WorkspaceEvent::DatabaseChanged(connection) => {
                    Sessions::global(cx)
                        .update(cx, |sessions, cx| sessions.refresh(connection, cx));
                    this.refresh_connection(connection, cx);
                }
                WorkspaceEvent::QueriesChanged => {
                    let scopes: Vec<Scope> = this
                        .objects
                        .keys()
                        .filter(|key| key.kind == ObjectKind::Query)
                        .map(|key| key.scope.clone())
                        .collect();
                    for scope in scopes {
                        this.require_with(
                            Requirement::Objects {
                                scope,
                                kind: ObjectKind::Query,
                            },
                            true,
                            cx,
                        );
                    }
                }
                WorkspaceEvent::SelectionChanged => this.require_selection(cx),
            },
        )
        .detach();
        cx.subscribe(
            &ConnectionStore::global(cx),
            |this, _, _: &StoreEvent, cx| {
                let live: HashSet<String> = ConnectionStore::global(cx)
                    .read(cx)
                    .connections()
                    .iter()
                    .map(|connection| connection.id.clone())
                    .collect();
                this.retain_connections(&live, cx);
            },
        )
        .detach();
        cx.subscribe(&Sessions::global(cx), |this, _, e: &SessionsEvent, cx| {
            let open = if let SessionsEvent::Closed(id)
            | SessionsEvent::Reopening(id)
            | SessionsEvent::Opened(id) = e
            {
                this.invalidate_connection(id, cx);
                matches!(e, SessionsEvent::Opened(_))
            } else {
                false
            };
            // Opening dropped the entries already; restart what the panes show so they do not
            // stay on "Loading…" waiting for an event that will not come again.
            if open {
                this.require_selection(cx);
            }
        })
        .detach();
        Catalog::default()
    });
    cx.set_global(GlobalCatalog(catalog));
}

#[cfg(test)]
mod tests {
    use super::*;
    use navidog_db::runtime;

    fn column(name: &str) -> ColumnInfo {
        ColumnInfo {
            name: name.into(),
            type_name: "int".into(),
            nullable: false,
            primary_key: true,
            default: None,
            comment: None,
        }
    }

    fn key(scope: &Scope, table: &str) -> TableKey {
        TableKey {
            scope: scope.clone(),
            table: table.into(),
        }
    }

    /// A cached key must not even start the query: `run_rel` executes as soon as it is called.
    #[gpui_kit::test]
    fn a_cache_hit_starts_no_query(cx: &mut gpui_kit::TestAppContext) {
        let catalog = cx.new(|_| Catalog::default());
        let scope = Scope::new("c", "db", None);
        catalog.update(cx, |cat, cx| {
            cat.columns
                .insert(key(&scope, "actor"), Load::Ready(vec![column("id")]));
            let revision = cat.revision();
            cat.require(
                Requirement::Columns {
                    scope: scope.clone(),
                    table: "actor".into(),
                },
                cx,
            );
            assert_eq!(cat.revision(), revision, "a cache hit is not a change");
            assert!(matches!(
                cat.columns(&scope, "actor"),
                Some(Load::Ready(columns)) if columns[0].name == "id"
            ));
            assert!(!cat.columns.has_pending_tasks());
        });
    }

    /// Invalidation drops the entries, the in-flight tasks and their generation tokens.
    #[gpui_kit::test]
    fn invalidation_drops_entries_tasks_and_generations(cx: &mut gpui_kit::TestAppContext) {
        let catalog = cx.new(|_| Catalog::default());
        let scope = Scope::new("c", "db", None);
        let (release, pending) = tokio::sync::oneshot::channel::<DbResult<Vec<ColumnInfo>>>();
        let job = runtime::spawn(async move {
            let _ = pending.await;
            Ok(Vec::new())
        });
        catalog.update(cx, |cat, cx| {
            cat.start(
                LoadId::Columns(key(&scope, "actor")),
                key(&scope, "actor"),
                |c| &mut c.columns,
                move |_| Some(job),
                false,
                cx,
            );
            assert!(cat.columns.task_pending(&key(&scope, "actor")));
            assert_eq!(cat.generation.len(), 1);
            cat.invalidate_connection("c", cx);
            assert!(cat.columns(&scope, "actor").is_none());
            assert!(!cat.columns.task_pending(&key(&scope, "actor")));
            assert!(cat.generation.is_empty());
            // The released result must not recreate the entry.
            let _ = release.send(Ok(vec![column("late")]));
        });
        cx.run_until_parked();
        catalog.read_with(cx, |cat, _| {
            assert!(cat.columns(&scope, "actor").is_none());
        });
    }

    /// A finished load clears its in-flight marker, and an invalidation that removes nothing
    /// leaves the revision alone (D-08: the revision only follows cache changes).
    #[gpui_kit::test]
    fn a_finished_load_clears_its_task_and_no_op_invalidation_keeps_the_revision(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        cx.executor().allow_parking();
        let catalog = cx.new(|_| Catalog::default());
        let scope = Scope::new("c", "db", None);
        let job = runtime::spawn(async { Ok(vec![column("id")]) });
        catalog.update(cx, |cat, cx| {
            cat.start(
                LoadId::Columns(key(&scope, "actor")),
                key(&scope, "actor"),
                |c| &mut c.columns,
                move |_| Some(job),
                false,
                cx,
            );
        });
        for _ in 0..50 {
            cx.run_until_parked();
            if catalog.read_with(cx, |cat, _| {
                matches!(cat.columns(&scope, "actor"), Some(Load::Ready(_)))
            }) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        catalog.update(cx, |cat, cx| {
            assert!(matches!(cat.columns(&scope, "actor"), Some(Load::Ready(_))));
            assert!(
                !cat.columns.task_pending(&key(&scope, "actor")),
                "a finished load drops its task marker"
            );
            let revision = cat.revision();
            let live = HashSet::from(["c".to_string()]);
            cat.retain_connections(&live, cx);
            cat.invalidate_connection("other", cx);
            assert_eq!(
                cat.revision(),
                revision,
                "an invalidation that removes nothing is not a cache change"
            );
        });
    }

    /// A failed load stays until someone reloads it: `require` never silently retries.
    #[gpui_kit::test]
    fn a_failed_entry_is_kept_until_a_reload(cx: &mut gpui_kit::TestAppContext) {
        cx.executor().allow_parking();
        let catalog = cx.new(|_| Catalog::default());
        let scope = Scope::new("c", "db", None);
        let failed = runtime::spawn(async { Err(navidog_db::DbError::msg("boom")) });
        catalog.update(cx, |cat, cx| {
            cat.start(
                LoadId::Columns(key(&scope, "actor")),
                key(&scope, "actor"),
                |c| &mut c.columns,
                move |_| Some(failed),
                false,
                cx,
            );
        });
        for _ in 0..50 {
            cx.run_until_parked();
            if catalog.read_with(cx, |cat, _| {
                matches!(cat.columns(&scope, "actor"), Some(Load::Failed(_)))
            }) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        catalog.update(cx, |cat, cx| {
            let before = cat.generation.len();
            cat.require(
                Requirement::Columns {
                    scope: scope.clone(),
                    table: "actor".into(),
                },
                cx,
            );
            assert_eq!(
                cat.generation.len(),
                before,
                "require does not retry a failure"
            );
            assert!(matches!(
                cat.columns(&scope, "actor"),
                Some(Load::Failed(_))
            ));
        });
        let good = runtime::spawn(async { Ok(vec![column("id")]) });
        catalog.update(cx, |cat, cx| {
            cat.start(
                LoadId::Columns(key(&scope, "actor")),
                key(&scope, "actor"),
                |c| &mut c.columns,
                move |_| Some(good),
                true,
                cx,
            );
        });
        for _ in 0..50 {
            cx.run_until_parked();
            if catalog.read_with(cx, |cat, _| {
                matches!(cat.columns(&scope, "actor"), Some(Load::Ready(columns)) if columns[0].name == "id")
            }) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("the reload did not replace the failure");
    }

    /// Fields and the other table children share a `(scope, table)` key. Both loads must finish.
    #[gpui_kit::test]
    fn fields_and_children_of_one_table_both_finish(cx: &mut gpui_kit::TestAppContext) {
        // The loads await `runtime::spawn` handles, so their completion wakes the
        // foreground task from the db thread. That is real IO: allow it, or the
        // scheduler records the cross-thread wake as non-determinism.
        cx.executor().allow_parking();
        let catalog = cx.new(|_| Catalog::default());
        let scope = Scope::new("c", "db", None);
        let columns = runtime::spawn(async { Ok(vec![column("id")]) });
        let children = runtime::spawn(async {
            Ok(TableChildren {
                indexes: vec!["idx".into()],
                ..TableChildren::default()
            })
        });
        catalog.update(cx, |cat, cx| {
            cat.start(
                LoadId::Columns(key(&scope, "actor")),
                key(&scope, "actor"),
                |c| &mut c.columns,
                move |_| Some(columns),
                true,
                cx,
            );
            cat.start(
                LoadId::Children(key(&scope, "actor")),
                key(&scope, "actor"),
                |c| &mut c.children,
                move |_| Some(children),
                true,
                cx,
            );
        });
        for _ in 0..50 {
            cx.run_until_parked();
            let done = catalog.read_with(cx, |cat, _| {
                matches!(cat.columns(&scope, "actor"), Some(Load::Ready(_)))
                    && matches!(cat.table_children(&scope, "actor"), Some(Load::Ready(_)))
            });
            if done {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let state = catalog.read_with(cx, |cat, _| {
            (
                format!(
                    "{:?}",
                    cat.columns(&scope, "actor")
                        .map(|l| matches!(l, Load::Ready(_)))
                ),
                format!(
                    "{:?}",
                    cat.table_children(&scope, "actor")
                        .map(|l| matches!(l, Load::Ready(_)))
                ),
            )
        });
        panic!("fields and children did not both finish: {state:?}");
    }

    #[gpui_kit::test]
    fn a_replaced_metadata_load_cannot_restore_older_values(cx: &mut gpui_kit::TestAppContext) {
        cx.executor().allow_parking();
        let catalog = cx.new(|_| Catalog::default());
        let scope = Scope::new("c", "db", None);
        let (release, pending) = tokio::sync::oneshot::channel();
        let (completed, finished) = std::sync::mpsc::channel();
        let older = runtime::spawn(async move {
            pending.await.unwrap();
            completed.send(()).unwrap();
            Ok(vec![column("older")])
        });
        let newer = runtime::spawn(async { Ok(vec![column("newer")]) });
        catalog.update(cx, |catalog, cx| {
            for job in [older, newer] {
                catalog.start(
                    LoadId::Columns(key(&scope, "items")),
                    key(&scope, "items"),
                    |catalog| &mut catalog.columns,
                    move |_| Some(job),
                    true,
                    cx,
                );
            }
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            cx.run_until_parked();
            if catalog.read_with(cx, |catalog, _| {
                matches!(
                    catalog.columns(&scope, "items"),
                    Some(Load::Ready(columns)) if columns[0].name == "newer"
                )
            }) {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        release.send(()).unwrap();
        finished
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        cx.run_until_parked();
        catalog.read_with(cx, |catalog, _| {
            assert!(matches!(
                catalog.columns(&scope, "items"),
                Some(Load::Ready(columns)) if columns[0].name == "newer"
            ));
        });
    }

    #[gpui_kit::test]
    fn a_saved_query_is_listed_under_queries(cx: &mut gpui_kit::TestAppContext) {
        let _home = crate::test_env::lock();
        let dir = std::env::temp_dir().join(format!("navidog-query-list-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: `lock` is held until this test returns.
        unsafe { std::env::set_var("NAVIDOG_HOME", &dir) };
        let scope = Scope::new("c1", "sakila", None);
        let path = navidog_model::queries::path(&scope, "actor");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "SELECT 1").unwrap();
        let catalog = cx.new(|_| Catalog::default());
        catalog.update(cx, |cat, cx| {
            cat.require_with(
                Requirement::Objects {
                    scope: scope.clone(),
                    kind: ObjectKind::Query,
                },
                true,
                cx,
            );
        });
        let names: Vec<String> =
            catalog.read_with(cx, |cat, _| match cat.objects(&scope, ObjectKind::Query) {
                Some(Load::Ready(list)) => list.iter().map(|o| o.name.clone()).collect(),
                other => panic!("query category was not ready: {other:?}"),
            });
        assert_eq!(names, vec!["actor".to_string()]);
    }
}
