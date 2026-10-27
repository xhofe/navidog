//! The metadata cache state machine: the workspace selection drives loads, closing a connection
//! clears its entries, reopening restarts what the panes show, and removing a connection drops
//! everything cached for it.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::TestAppContext;
use navidog_app_state::store::ConnectionStore;
use navidog_app_state::workspace::{Selection, Workspace};
use navidog_db::{ConnectParams, Secrets, connect, runtime};
use navidog_model::connection::Connection;
use navidog_model::crypto::Key;
use navidog_model::object::{ObjectKind, ObjectRef, Scope};
use navidog_navigator::catalog::{Catalog, Load, Requirement};
use navidog_session::sessions::Sessions;
use navidog_sqlite::SqliteDriver;

fn wait(cx: &mut TestAppContext, what: &str, done: impl Fn(&mut TestAppContext) -> bool) {
    navidog_test_support::wait_with(
        cx,
        what,
        Duration::from_secs(10),
        Duration::from_millis(10),
        false,
        done,
    );
}

fn create_table(file: &Path) {
    runtime::runtime().block_on(async {
        let mut session = connect(
            Arc::new(SqliteDriver::default()),
            ConnectParams {
                file: Some(file.into()),
                create_new: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        session
            .relational()
            .unwrap()
            .execute("CREATE TABLE items(id INTEGER PRIMARY KEY)")
            .await
            .unwrap();
        session.close().await.unwrap();
    });
}

#[gpui_kit::test]
fn selection_drives_loads_and_the_session_lifecycle_clears_them(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let home = tempfile::tempdir().unwrap();
    let file = home.path().join("catalog.db");
    create_table(&file);
    // SAFETY: this integration-test binary has one test and owns its environment until exit.
    unsafe { std::env::set_var("NAVIDOG_HOME", home.path()) };
    let id = cx.update(|cx| {
        gpui_kit::init(cx);
        navidog_app_state::init(cx);
        navidog_ui::theme::init(cx);
        navidog_session::init(cx);
        navidog_navigator::init(cx);
        ConnectionStore::global(cx).update(cx, |store, cx| {
            store.set_key(Key::random());
            let mut connection = Connection::new("sqlite");
            connection.name = "catalog".into();
            connection.general.file = Some(file.to_string_lossy().into_owned());
            let id = connection.id.clone();
            store.upsert(connection, &[], cx).unwrap();
            id
        })
    });
    cx.update(|cx| {
        Sessions::global(cx).update(cx, |sessions, cx| {
            sessions.open(id.clone(), Secrets::default(), cx)
        });
    });
    wait(cx, "the session opens", |cx| {
        cx.update(|cx| Sessions::global(cx).read(cx).state(&id).is_open())
    });
    let scope = Scope::new(&id, "main", None);

    // Selecting the database loads its object list; the pane asks no questions of its own.
    cx.update(|cx| {
        Workspace::global(cx).update(cx, |workspace, cx| {
            workspace.select(Selection::Database(id.clone(), "main".into()), cx)
        });
    });
    wait(cx, "the selected database's tables", |cx| {
        cx.update(|cx| {
            matches!(
                Catalog::global(cx).read(cx).objects(&scope, ObjectKind::Table),
                Some(Load::Ready(objects)) if objects.iter().any(|o| o.name == "items")
            )
        })
    });

    // A category selection loads that category.
    cx.update(|cx| {
        Workspace::global(cx).update(cx, |workspace, cx| {
            workspace.select(Selection::Category(scope.clone(), ObjectKind::View), cx)
        });
    });
    wait(cx, "the selected category", |cx| {
        cx.update(|cx| {
            matches!(
                Catalog::global(cx)
                    .read(cx)
                    .objects(&scope, ObjectKind::View),
                Some(Load::Ready(_))
            )
        })
    });

    // Refreshing a scope reloads the DDL entries open views read instead of only dropping them.
    let items = ObjectRef {
        scope: scope.clone(),
        kind: ObjectKind::Table,
        name: "items".into(),
    };
    cx.update(|cx| {
        Catalog::global(cx).update(cx, |catalog, cx| {
            catalog.require(Requirement::Ddl(items.clone()), cx)
        });
    });
    wait(cx, "the DDL entry", |cx| {
        cx.update(|cx| {
            matches!(
                Catalog::global(cx).read(cx).ddl(&items),
                Some(Load::Ready(_))
            )
        })
    });
    cx.update(|cx| Catalog::global(cx).update(cx, |catalog, cx| catalog.refresh_scope(&scope, cx)));
    wait(cx, "the DDL entry after a scope refresh", |cx| {
        cx.update(|cx| {
            matches!(
                Catalog::global(cx).read(cx).ddl(&items),
                Some(Load::Ready(_))
            )
        })
    });

    // Closing the connection drops every entry of it.
    cx.update(|cx| {
        Sessions::global(cx).update(cx, |sessions, cx| sessions.close(&id, cx));
    });
    wait(cx, "closing clears the cache", |cx| {
        cx.update(|cx| {
            let catalog = Catalog::global(cx);
            let catalog = catalog.read(cx);
            catalog.objects(&scope, ObjectKind::Table).is_none()
                && catalog.objects(&scope, ObjectKind::View).is_none()
                && catalog.schemas(&id, "main").is_none()
        })
    });

    // Reopening restarts what the current selection shows, without another selection event.
    cx.update(|cx| {
        Sessions::global(cx).update(cx, |sessions, cx| {
            sessions.open(id.clone(), Secrets::default(), cx)
        });
    });
    wait(cx, "reopening restores the shown entries", |cx| {
        cx.update(|cx| {
            matches!(
                Catalog::global(cx)
                    .read(cx)
                    .objects(&scope, ObjectKind::View),
                Some(Load::Ready(_))
            )
        })
    });

    // Removing the connection drops what is cached for it.
    cx.update(|cx| {
        ConnectionStore::global(cx).update(cx, |store, cx| store.remove(&id, cx).unwrap());
    });
    wait(cx, "removing the connection clears the cache", |cx| {
        cx.update(|cx| {
            let catalog = Catalog::global(cx);
            let catalog = catalog.read(cx);
            catalog.objects(&scope, ObjectKind::Table).is_none()
                && catalog.objects(&scope, ObjectKind::View).is_none()
        })
    });
}
