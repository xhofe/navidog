//! Viewer unit tests: the ownership rules that keep the grid, the drafts and
//! the session's committed rows in sync.

use super::*;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{EntityInputHandler as _, TestAppContext, WindowOptions};
use navidog_app_state::store::ConnectionStore;
use navidog_db::{ConnectParams, Secrets, connect};
use navidog_model::connection::Connection;
use navidog_model::crypto::Key;
use navidog_sqlite::SqliteDriver;
use std::sync::Arc;

struct Host(Entity<TableViewer>);
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}

fn sql(file: &std::path::Path, sql: &str) -> Vec<Vec<Option<String>>> {
    navidog_db::runtime::runtime().block_on(async {
        let mut session = connect(
            Arc::new(SqliteDriver::default()),
            ConnectParams {
                file: Some(file.into()),
                create_new: !file.exists(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let rows = session
            .relational()
            .unwrap()
            .execute(sql)
            .await
            .unwrap()
            .pop()
            .unwrap()
            .rows;
        session.close().await.unwrap();
        rows
    })
}

fn wait(cx: &mut TestAppContext, done: impl Fn(&mut TestAppContext) -> bool) {
    navidog_test_support::wait(cx, "the viewer operation", done);
}

fn discard_grid_changes(viewer: &mut TableViewer, cx: &mut Context<TableViewer>) {
    viewer.view.grid.update(cx, |grid, cx| {
        grid.update_model(cx, |model| model.discard())
    });
}

fn notify_database_changed(cx: &mut TestAppContext, connection: &str) {
    cx.update(|cx| {
        Workspace::global(cx).update(cx, |workspace, cx| {
            workspace.database_changed(connection.to_string(), cx);
        });
    });
    cx.run_until_parked();
}

#[gpui_kit::test]
fn visibility_uses_accepted_updates_inserts_and_deletes(cx: &mut TestAppContext) {
    let home =
        std::env::temp_dir().join(format!("navidog-viewer-ownership-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    let file = home.join("rows.db");
    sql(
        &file,
        "CREATE TABLE t(id INTEGER PRIMARY KEY, name TEXT); INSERT INTO t VALUES(1, 'a'), (2, 'b')",
    );
    // SAFETY: this is the only test in this process that changes the configuration directory.
    unsafe {
        std::env::set_var("NAVIDOG_HOME", &home);
    }
    cx.executor().allow_parking();
    let id = cx.update(|cx| {
        gpui_kit::init(cx);
        navidog_app_state::init(cx);
        navidog_ui::theme::init(cx);
        navidog_session::init(cx);
        crate::init(cx);
        Settings::update(cx, |s| {
            s.sync_record = false;
            s.auto_begin_transaction = false;
        });
        ConnectionStore::global(cx).update(cx, |s, cx| {
            s.set_key(Key::random());
            let mut connection = Connection::new("sqlite");
            connection.name = "ownership".into();
            connection.general.file = Some(file.to_string_lossy().into_owned());
            let id = connection.id.clone();
            s.upsert(connection, &[], cx).unwrap();
            id
        })
    });
    cx.update(|cx| {
        Sessions::global(cx).update(cx, |s, cx| s.open(id.clone(), Secrets::default(), cx))
    });
    wait(cx, |cx| {
        cx.update(|cx| Sessions::global(cx).read(cx).state(&id).is_open())
    });
    let (handle, viewer) = cx.update(|cx| {
        let mut viewer = None;
        let handle = gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            let entity = TableViewer::new(
                ObjectRef {
                    scope: Scope::new(id.clone(), "main", None),
                    kind: ObjectKind::Table,
                    name: "t".into(),
                },
                window,
                cx,
            );
            viewer = Some(entity.clone());
            cx.new(|_| Host(entity))
        })
        .unwrap();
        (handle.0, viewer.unwrap())
    });
    wait(cx, |cx| {
        cx.update(|cx| {
            !viewer.read(cx).busy && viewer.read(cx).view.grid.read(cx).model(cx).row_count() == 2
        })
    });
    cx.update_window(handle, |_, window, cx| {
        viewer.update(cx, |v, cx| {
            v.view.grid.update(cx, |g, cx| {
                g.update_model(cx, |m| {
                    m.set_value(0, 1, Some("z".into()));
                    m.select((1, 0), false);
                    m.delete_selected_rows();
                    let row = m.insert_row();
                    m.set_value(row, 0, Some("3".into()));
                    m.set_value(row, 1, Some("c".into()));
                })
            });
            v.apply_changes(false, window, cx);
        })
    })
    .unwrap();
    wait(cx, |cx| cx.update(|cx| !viewer.read(cx).busy));
    cx.update_window(handle, |_, window, cx| {
        viewer.update(cx, |v, cx| {
            v.toggle_column("id".into(), window, cx);
            v.toggle_column("id".into(), window, cx);
            let model = v.view.grid.read(cx).model(cx);
            assert_eq!(model.row_count(), 2);
            assert_eq!(model.value(0, 0), Some(&Some("1".into())));
            assert_eq!(model.value(0, 1), Some(&Some("z".into())));
            assert_eq!(model.value(1, 0), Some(&Some("3".into())));
            assert_eq!(model.value(1, 1), Some(&Some("c".into())));

            v.insert_row(cx);
            v.toggle_column("id".into(), window, cx);
            assert!(v.view.hidden.contains("id"));
            let model = v.view.grid.read(cx).model(cx);
            assert_eq!(model.columns.len(), 1);
            assert_eq!(model.columns[0].name, "name");
            assert_eq!(model.row_count(), 3);
            assert_eq!(model.row_state(2), crate::model::RowState::Inserted);
            assert!(model.change_set().inserts.is_empty());
            v.toggle_column("id".into(), window, cx);
            let model = v.view.grid.read(cx).model(cx);
            assert_eq!(model.columns.len(), 2);
            assert_eq!(model.value(2, 0), None);
            assert_eq!(model.value(2, 1), None);
        })
    })
    .unwrap();
    assert_eq!(
        sql(&file, "SELECT id,name FROM t ORDER BY id"),
        [
            vec![Some("1".into()), Some("z".into())],
            vec![Some("3".into()), Some("c".into())]
        ]
    );

    // Keep a rejected hide out of the menu while its commit is running and after failure.
    sql(
        &file,
        "CREATE TRIGGER reject_update BEFORE UPDATE ON t BEGIN SELECT RAISE(ABORT, 'reject pending edit'); END",
    );
    cx.update_window(handle, |_, window, cx| {
        viewer.update(cx, |v, cx| {
            v.view.grid.update(cx, |g, cx| {
                g.update_model(cx, |m| {
                    m.discard();
                    m.set_value(0, 1, Some("pending".into()));
                });
            });
            v.toggle_column("id".into(), window, cx);
            assert!(v.busy);
            assert!(!v.view.hidden.contains("id"));
            assert_eq!(v.view.grid.read(cx).model(cx).columns.len(), 2);
            assert_eq!(
                v.view.grid.read(cx).model(cx).value(0, 1),
                Some(&Some("pending".into()))
            );
            // A second request during the same write must leave the candidate unchanged.
            v.toggle_column("name".into(), window, cx);
            assert!(matches!(
                &v.after_commit,
                AfterCommit::Hidden { hidden, .. } if hidden == &HashSet::from(["id".to_string()])
            ));
        });
    })
    .unwrap();
    wait(cx, |cx| cx.update(|cx| !viewer.read(cx).busy));
    cx.update(|cx| {
        let v = viewer.read(cx);
        assert!(!v.view.hidden.contains("id"));
        assert_eq!(v.after_commit, AfterCommit::None);
        let model = v.view.grid.read(cx).model(cx);
        assert_eq!(model.columns.len(), 2);
        assert_eq!(model.value(0, 1), Some(&Some("pending".into())));
        assert!(model.has_changes());
    });
    assert_eq!(
        sql(&file, "SELECT name FROM t WHERE id = 1"),
        [vec![Some("z".into())]]
    );

    // Explicit recovery replaces the physical session, preserving the local row draft.
    let previous_session = cx.update(|cx| viewer.read(cx).session.clone().unwrap());
    cx.update_window(handle, |_, window, cx| window.close_dialog(cx))
        .unwrap();
    cx.update(|cx| {
        Sessions::global(cx).update(cx, |sessions, cx| {
            sessions.close_for_reopen(&id, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|cx| {
        let v = viewer.read(cx);
        assert!(v.session.is_none());
        assert_eq!(
            v.view.grid.read(cx).model(cx).value(0, 1),
            Some(&Some("pending".into()))
        );
        assert!(v.view.grid.read(cx).model(cx).has_changes());
        assert!(!v.view.hidden.contains("id"));
    });
    cx.update(|cx| {
        Sessions::global(cx).update(cx, |sessions, cx| {
            sessions.open(id.clone(), Secrets::default(), cx);
        })
    });
    wait(cx, |cx| {
        cx.update(|cx| {
            Sessions::global(cx).read(cx).state(&id).is_open()
                && viewer.read(cx).session.is_some()
                && !viewer.read(cx).busy
        })
    });
    cx.update(|cx| {
        let v = viewer.read(cx);
        assert!(!Arc::ptr_eq(v.session.as_ref().unwrap(), &previous_session));
        let model = v.view.grid.read(cx).model(cx);
        assert_eq!(model.columns.len(), 2);
        assert_eq!(model.row_count(), 2);
        assert_eq!(model.value(0, 1), Some(&Some("pending".into())));
        assert!(model.has_changes());
        assert!(!v.view.hidden.contains("id"));
    });
    assert_eq!(
        sql(&file, "SELECT name FROM t WHERE id = 1"),
        [vec![Some("z".into())]]
    );
    // Once the user verifies and explicitly applies the draft, it uses the new session.
    sql(&file, "DROP TRIGGER reject_update");
    cx.update_window(handle, |_, window, cx| {
        viewer.update(cx, |v, cx| v.apply_changes(false, window, cx));
    })
    .unwrap();
    wait(cx, |cx| cx.update(|cx| !viewer.read(cx).busy));
    assert_eq!(
        sql(&file, "SELECT name FROM t WHERE id = 1"),
        [vec![Some("pending".into())]]
    );
    cx.update(|cx| assert!(!viewer.read(cx).view.grid.read(cx).model(cx).has_changes()));

    // Mixed writes must not consume independent blank drafts, whether synchronization is
    // disabled, succeeds, or fails after the database has already committed the batch.
    sql(
        &file,
        "CREATE TABLE write_audit(event TEXT); \
         CREATE TRIGGER audit_update AFTER UPDATE ON t BEGIN INSERT INTO write_audit VALUES('update'); END; \
         CREATE TRIGGER audit_insert AFTER INSERT ON t BEGIN INSERT INTO write_audit VALUES('insert'); END; \
         CREATE TRIGGER audit_delete AFTER DELETE ON t BEGIN INSERT INTO write_audit VALUES('delete'); END",
    );
    for (sync, fail_sync) in [(false, false), (true, false), (true, true)] {
        sql(
            &file,
            "DELETE FROM t; INSERT INTO t VALUES(1, 'a'), (2, 'b'); DELETE FROM write_audit",
        );
        cx.update(|cx| {
            Settings::update(cx, |s| s.sync_record = sync);
            viewer.update(cx, |v, cx| v.reload(true, cx));
        });
        wait(cx, |cx| cx.update(|cx| !viewer.read(cx).busy));
        cx.update_window(handle, |_, window, cx| {
            viewer.update(cx, |v, cx| {
                if fail_sync {
                    v.view.filter.update(cx, |input, cx| {
                        input.set_value("missing_sync_column = 1", window, cx);
                    });
                }
                v.view.grid.update(cx, |g, cx| {
                    g.update_model(cx, |m| {
                        m.set_value(0, 1, Some("updated".into()));
                        m.select((1, 0), false);
                        m.delete_selected_rows();
                        m.insert_row();
                        let filled = m.insert_row();
                        m.set_value(filled, 0, Some("3".into()));
                        m.set_value(filled, 1, Some("inserted".into()));
                        let blank = m.insert_row();
                        m.select((blank, 1), false);
                    });
                });
                v.apply_changes(false, window, cx);
            });
        })
        .unwrap();
        wait(cx, |cx| cx.update(|cx| !viewer.read(cx).busy));
        cx.update(|cx| {
            let v = viewer.read(cx);
            let model = v.view.grid.read(cx).model(cx);
            assert_eq!(model.row_count(), 4, "sync={sync}, fail_sync={fail_sync}");
            assert_eq!(model.value(0, 1), Some(&Some("updated".into())));
            assert_eq!(model.value(1, 1), Some(&Some("inserted".into())));
            assert_eq!(model.head(), (3, 1));
            for row in 2..4 {
                assert_eq!(model.row_state(row), crate::model::RowState::Inserted);
                assert_eq!(model.value(row, 0), None);
                assert_eq!(model.value(row, 1), None);
            }
            let changes = model.change_set();
            assert!(changes.updates.is_empty());
            assert!(changes.deletes.is_empty());
            assert!(changes.inserts.is_empty());
            if fail_sync {
                assert!(v.view.status.contains("missing_sync_column"));
            }
        });
        assert_eq!(
            sql(&file, "SELECT id,name FROM t ORDER BY id"),
            [
                vec![Some("1".into()), Some("updated".into())],
                vec![Some("3".into()), Some("inserted".into())]
            ]
        );
        assert_eq!(
            sql(&file, "SELECT COUNT(*) FROM write_audit"),
            [vec![Some("3".into())]]
        );

        // Apply again only sees blank drafts. It sends no writes and preserves defaults.
        cx.update_window(handle, |_, window, cx| {
            viewer.update(cx, |v, cx| v.apply_changes(false, window, cx));
        })
        .unwrap();
        cx.run_until_parked();
        assert_eq!(
            sql(&file, "SELECT COUNT(*) FROM write_audit"),
            [vec![Some("3".into())]]
        );
        for _ in 0..2 {
            cx.update_window(handle, |_, window, cx| {
                viewer.update(cx, |v, cx| {
                    v.view
                        .filter
                        .update(cx, |input, cx| input.set_value("", window, cx));
                    v.reload(true, cx);
                });
            })
            .unwrap();
            wait(cx, |cx| cx.update(|cx| !viewer.read(cx).busy));
            cx.update(|cx| {
                let model = viewer.read(cx).view.grid.read(cx).model(cx);
                assert_eq!(model.row_count(), 4, "sync must not duplicate blank drafts");
                assert!(model.change_set().inserts.is_empty());
            });
        }
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            gpui_kit::test::TestWindowExt::click(window, "row-discard", cx);
        })
        .unwrap();
        cx.update(|cx| {
            let model = viewer.read(cx).view.grid.read(cx).model(cx);
            assert_eq!(model.row_count(), 2);
            assert!(!model.has_changes());
        });
    }

    // Construct the FK picker through its real owner handler. The owner is already
    // mutably borrowed here, so the constructor must use its captured session generation.
    cx.update_window(handle, |_, window, cx| {
        viewer.update(cx, |v, cx| {
            v.view
                .grid
                .update(cx, |g, cx| g.update_model(cx, |m| m.select((0, 0), false)));
            v.foreign_keys = vec![navidog_db::ForeignKey {
                column: "id".into(),
                schema: "main".into(),
                table: "t".into(),
                referenced: "id".into(),
            }];
            v.open_fk_picker(window, cx);
        });
        assert!(window.has_active_dialog(cx));
        window.close_dialog(cx);
    })
    .unwrap();

    // A detached transaction command can finish after Reopening. Its callback, and stale
    // load/commit/connect results, must not mutate or unlock the replacement session's view.
    let (old_generation, old_session) = cx.update(|cx| {
        viewer.update(cx, |v, cx| {
            v.run_simple(
                "SELECT 1",
                |v, _| v.view.status = "stale statement".into(),
                cx,
            );
            (v.session_generation, v.session.clone().unwrap())
        })
    });
    cx.update(|cx| {
        Sessions::global(cx).update(cx, |sessions, cx| {
            sessions.close_for_reopen(&id, cx);
            sessions.open(id.clone(), Secrets::default(), cx);
        });
    });
    wait(cx, |cx| {
        cx.update(|cx| {
            let v = viewer.read(cx);
            v.session.is_some() && v.session_generation != old_generation && !v.busy
        })
    });
    cx.update_window(handle, |_, window, cx| {
        viewer.update(cx, |v, cx| {
            assert_ne!(v.view.status.as_ref(), "stale statement");
            v.view.grid.update(cx, |g, cx| {
                g.update_model(cx, |m| {
                    m.set_value(0, 1, Some("newer pending".into()));
                    m.insert_row();
                });
            });
            v.view.status = "replacement operation".into();
            v.set_busy(true, cx);
            let replacement = v.session.clone().unwrap();
            assert!(!v.apply_page(old_generation, Ok(Vec::new()), cx));
            assert!(!v.apply_page(old_generation, Err("stale SELECT".into()), cx));
            v.apply_commit_result(old_generation, Ok(()), true, window, cx);
            v.apply_commit_result(old_generation, Err("stale commit".into()), true, window, cx);
            assert!(!v.apply_connected(
                old_generation,
                Ok(Ok((
                    std::sync::Arc::new(navidog_sqlite::SqliteDialect),
                    Vec::new()
                ))),
                old_session,
                None,
                cx,
            ));
            assert!(v.busy, "old callbacks must not unlock a new operation");
            assert!(v.view.grid.read(cx).model(cx).locked);
            assert_eq!(v.view.status.as_ref(), "replacement operation");
            assert!(Arc::ptr_eq(v.session.as_ref().unwrap(), &replacement));
            let model = v.view.grid.read(cx).model(cx);
            assert_eq!(model.row_count(), 3);
            assert_eq!(model.value(0, 1), Some(&Some("newer pending".into())));
            assert!(model.has_changes());
            v.set_busy(false, cx);
        });
    })
    .unwrap();

    // The public direct-open path can also replace an already open connection. It must
    // publish Reopening so the existing editor releases its old session and reconnects.
    let (replaced_generation, replaced_session) = cx.update(|cx| {
        let v = viewer.read(cx);
        (v.session_generation, v.session.clone().unwrap())
    });
    cx.update(|cx| {
        Sessions::global(cx).update(cx, |sessions, cx| {
            sessions.open(id.clone(), Secrets::default(), cx);
        });
    });
    wait(cx, |cx| {
        cx.update(|cx| {
            let v = viewer.read(cx);
            Sessions::global(cx).read(cx).state(&id).is_open()
                && v.session.is_some()
                && v.session_generation != replaced_generation
                && !v.busy
        })
    });
    cx.update(|cx| {
        let v = viewer.read(cx);
        assert!(!Arc::ptr_eq(v.session.as_ref().unwrap(), &replaced_session));
        let model = v.view.grid.read(cx).model(cx);
        assert!(!model.locked);
        assert_eq!(model.row_count(), 3);
        assert_eq!(model.value(0, 1), Some(&Some("newer pending".into())));
        assert_eq!(model.row_state(2), crate::model::RowState::Inserted);
        assert!(model.has_changes());
    });

    // Reset the fixture after lifecycle checks, then exercise main's external SQL refresh
    // scenarios on the same test context so NAVIDOG_HOME has only one owner.
    sql(
        &file,
        "DROP TABLE t; CREATE TABLE t(id INTEGER PRIMARY KEY, name TEXT); INSERT INTO t VALUES(1, 'z'), (3, 'c')",
    );
    cx.update_window(handle, |_, window, cx| {
        Settings::update(cx, |settings| {
            settings.sync_record = false;
            settings.auto_begin_transaction = false;
        });
        viewer.update(cx, |v, cx| {
            discard_grid_changes(v, cx);
            v.metadata_stale = true;
            v.view
                .filter
                .update(cx, |input, cx| input.set_value("", window, cx));
            v.view
                .order
                .update(cx, |input, cx| input.set_value("", window, cx));
            v.page = 0;
            v.reload(true, cx);
        });
    })
    .unwrap();
    wait(cx, |cx| cx.update(|cx| !viewer.read(cx).busy));

    // Start with a clean page. Notifications for other connections must not reload it.
    cx.update(|cx| {
        viewer.update(cx, |v, cx| {
            v.view
                .grid
                .update(cx, |g, cx| g.update_model(cx, |m| m.discard()));
        })
    });
    sql(&file, "UPDATE t SET name='elsewhere' WHERE id=1");
    notify_database_changed(cx, "another-connection");
    cx.update(|cx| {
        let v = viewer.read(cx);
        assert!(!v.busy);
        assert_eq!(
            v.view.grid.read(cx).model(cx).value(0, 1),
            Some(&Some("z".into()))
        );
    });
    notify_database_changed(cx, &id);
    wait(cx, |cx| cx.update(|cx| !viewer.read(cx).busy));
    cx.update(|cx| {
        let v = viewer.read(cx);
        assert_eq!(v.total, Some(2));
        assert_eq!(
            v.view.grid.read(cx).model(cx).value(0, 1),
            Some(&Some("elsewhere".into()))
        );
    });

    // Keep a page request in flight while three notifications arrive. One pending refresh
    // must reread both rows and metadata after the operation releases the owned session.
    let session = cx.update(|cx| viewer.read(cx).session.clone().unwrap());
    let held = session.clone().try_lock_owned().unwrap();
    cx.update_window(handle, |_, window, cx| {
        viewer.update(cx, |v, cx| {
            v.view
                .filter
                .update(cx, |input, cx| input.set_value("id >= 1", window, cx));
            v.view
                .order
                .update(cx, |input, cx| input.set_value("id DESC", window, cx));
            v.limit = Some(1);
            v.page = 1;
            v.reload(true, cx);
        })
    })
    .unwrap();
    for _ in 0..3 {
        notify_database_changed(cx, &id);
    }
    cx.update(|cx| {
        let v = viewer.read(cx);
        assert!(v.busy);
        assert!(v.external_refresh_pending);
    });
    sql(
        &file,
        "CREATE TABLE parent(id INTEGER PRIMARY KEY); \
        ALTER TABLE t ADD COLUMN parent_id INTEGER REFERENCES parent(id); \
        INSERT INTO t(id,name) VALUES(4,'new row'); UPDATE t SET name='queued' WHERE id=3",
    );
    drop(held);
    wait(cx, |cx| {
        cx.update(|cx| {
            let v = viewer.read(cx);
            !v.busy && !v.external_refresh_pending && v.columns.len() == 3
        })
    });
    cx.update(|cx| {
        let v = viewer.read(cx);
        assert!(Arc::ptr_eq(v.session.as_ref().unwrap(), &session));
        assert_eq!(v.total, Some(3));
        assert_eq!(v.limit, Some(1));
        assert_eq!(v.page, 1);
        assert_eq!(v.view.filter.read(cx).value().as_ref(), "id >= 1");
        assert_eq!(v.view.order.read(cx).value().as_ref(), "id DESC");
        assert_eq!(
            v.view.grid.read(cx).model(cx).value(0, 1),
            Some(&Some("queued".into()))
        );
        assert_eq!(v.foreign_keys.len(), 1);
        assert_eq!(v.foreign_keys[0].column, "parent_id");
    });

    // An untouched inline editor is still input owned by the user. No notification may
    // settle that editor, send a write, or replace the loaded page.
    cx.update_window(handle, |_, window, cx| {
        viewer.update(cx, |v, cx| {
            v.view
                .grid
                .update(cx, |g, cx| g.start_edit((0, 1), window, cx));
            assert!(v.view.grid.read(cx).is_editing(cx));
        })
    })
    .unwrap();
    sql(&file, "UPDATE t SET name='latest' WHERE id=3");
    notify_database_changed(cx, &id);
    cx.update_window(handle, |_, window, cx| {
        viewer.update(cx, |v, cx| {
            assert!(!v.busy);
            assert!(v.view.grid.read(cx).is_editing(cx));
            assert_eq!(
                v.view.grid.read(cx).model(cx).value(0, 1),
                Some(&Some("queued".into()))
            );
            assert_eq!(
                v.view.status,
                t!("viewer.external_refresh_pending").to_string()
            );
            v.view.grid.update(cx, |g, cx| g.commit_edit(window, cx));
            discard_grid_changes(v, cx);
        })
    })
    .unwrap();

    // Dirty rows and completely blank inserted rows both prevent an external reload.
    for blank in [false, true] {
        cx.update(|cx| {
            viewer.update(cx, |v, cx| {
                v.view.grid.update(cx, |g, cx| {
                    g.update_model(cx, |m| {
                        if blank {
                            m.insert_row();
                        } else {
                            m.set_value(0, 1, Some("local".into()));
                        }
                    })
                });
            })
        });
        notify_database_changed(cx, &id);
        cx.update(|cx| {
            viewer.update(cx, |v, cx| {
                assert!(!v.busy);
                assert!(v.view.grid.read(cx).model(cx).has_changes());
                if blank {
                    assert_eq!(v.view.grid.read(cx).model(cx).row_count(), 2);
                    assert!(
                        v.view
                            .grid
                            .read(cx)
                            .model(cx)
                            .change_set()
                            .inserts
                            .is_empty()
                    );
                } else {
                    assert_eq!(
                        v.view.grid.read(cx).model(cx).value(0, 1),
                        Some(&Some("local".into()))
                    );
                }
                discard_grid_changes(v, cx);
            })
        });
    }
    assert_eq!(
        sql(&file, "SELECT name FROM t WHERE id=3"),
        [vec![Some("latest".into())]]
    );

    // Inputs not yet written back to the model need the same protection as model changes.
    cx.update_window(handle, |_, window, cx| {
        viewer.update(cx, |v, cx| {
            v.view.form = true;
            v.sync_form(window, cx);
            v.view.form_inputs[1].update(cx, |input, cx| input.set_value("form draft", window, cx));
        })
    })
    .unwrap();
    notify_database_changed(cx, &id);
    cx.update_window(handle, |_, window, cx| {
        viewer.update(cx, |v, cx| {
            assert!(!v.busy);
            assert_eq!(
                v.view.form_inputs[1].read(cx).value().as_ref(),
                "form draft"
            );
            assert!(!v.view.grid.read(cx).model(cx).has_changes());
            v.view.form = false;
            v.view.form_for = None;
            v.view.form_inputs.clear();
            v.open_editor(Kind::Text, window, cx);
            v.view
                .editor_input
                .update(cx, |input, cx| input.set_value("field draft", window, cx));
        })
    })
    .unwrap();
    notify_database_changed(cx, &id);
    cx.update(|cx| {
        viewer.update(cx, |v, cx| {
            assert!(!v.busy);
            assert_eq!(v.view.editor_input.read(cx).value().as_ref(), "field draft");
            assert!(!v.view.grid.read(cx).model(cx).has_changes());
            v.view.editor = None;
        })
    });

    // A blocked catalog change is not forgotten: the user's later explicit refresh must
    // read the new field definitions as well as rows, using the same owned session.
    sql(&file, "ALTER TABLE t ADD COLUMN note TEXT");
    cx.update_window(handle, |_, window, cx| {
        viewer.update(cx, |v, cx| {
            v.open_editor(Kind::Text, window, cx);
            v.view
                .editor_input
                .update(cx, |input, cx| input.set_value("unapplied", window, cx));
        })
    })
    .unwrap();
    notify_database_changed(cx, &id);
    cx.update(|cx| {
        viewer.update(cx, |v, cx| {
            assert!(v.metadata_stale);
            assert_eq!(v.columns.len(), 3);
            v.view.editor = None;
            v.reload(true, cx);
        })
    });
    wait(cx, |cx| cx.update(|cx| !viewer.read(cx).busy));
    cx.update(|cx| {
        let v = viewer.read(cx);
        assert!(!v.metadata_stale);
        assert_eq!(v.columns.len(), 4);
        assert_eq!(v.columns[3].name, "note");
        assert_eq!(v.foreign_keys.len(), 1);
    });

    // Input can arrive after an automatic refresh has begun. Apply must not mark a field
    // draft as accepted while the locked grid would reject the write.
    cx.update_window(handle, |_, window, cx| {
        viewer.update(cx, |v, cx| {
            v.view.grid.update(cx, |grid, cx| {
                grid.update_model(cx, |model| model.select((0, 1), false))
            });
            v.open_editor(Kind::Text, window, cx);
        })
    })
    .unwrap();
    let held = session.clone().try_lock_owned().unwrap();
    notify_database_changed(cx, &id);
    cx.update_window(handle, |_, window, cx| {
        viewer.update(cx, |v, cx| {
            assert!(v.busy);
            assert_eq!(v.view.editor_baseline, "latest");
            v.view.editor_input.update(cx, |input, cx| {
                input.set_value("during refresh", window, cx)
            });
            v.apply_editor(window, cx);
            assert_eq!(v.view.editor_baseline, "latest");
            assert!(!v.view.grid.read(cx).model(cx).has_changes());
        });
        window.render_frame(cx);
        assert_eq!(window.find("editor-update").disabled(), Some(true));
        window.click("editor-update", cx);
    })
    .unwrap();
    drop(held);
    wait(cx, |cx| cx.update(|cx| !viewer.read(cx).busy));
    cx.update(|cx| {
        viewer.update(cx, |v, cx| {
            assert_eq!(
                v.view.editor_input.read(cx).value().as_ref(),
                "during refresh"
            );
            assert_eq!(v.view.editor_baseline, "latest");
            assert_eq!(
                v.view.grid.read(cx).model(cx).value(0, 1),
                Some(&Some("latest".into()))
            );
            assert!(!v.view.grid.read(cx).model(cx).has_changes());
            v.view.editor = None;
        })
    });

    // Form navigation likewise cannot flush into a locked model and then replace the
    // input entities. The refresh completion must retain the text entered in flight.
    cx.update(|cx| {
        viewer.update(cx, |v, cx| {
            v.limit = Some(2);
            v.page = 0;
            v.reload(true, cx);
        })
    });
    wait(cx, |cx| cx.update(|cx| !viewer.read(cx).busy));
    cx.update_window(handle, |_, window, cx| {
        viewer.update(cx, |v, cx| {
            v.view.form = true;
            v.sync_form(window, cx);
        })
    })
    .unwrap();
    let held = session.clone().try_lock_owned().unwrap();
    notify_database_changed(cx, &id);
    cx.update_window(handle, |_, window, cx| {
        viewer.update(cx, |v, cx| {
            assert!(v.busy);
            let row = v.view.form_for;
            v.view.form_inputs[1].update(cx, |input, cx| {
                input.set_value("form during refresh", window, cx)
            });
            v.form_step(1, cx);
            assert_eq!(v.view.form_for, row);
            assert!(!v.view.grid.read(cx).model(cx).has_changes());
        });
        window.render_frame(cx);
        assert_eq!(window.find("form-next").disabled(), Some(true));
        assert_eq!(window.find("form-prev").disabled(), Some(true));
        assert_eq!(window.find("viewer-form").disabled(), Some(true));
        window.click("form-next", cx);
    })
    .unwrap();
    drop(held);
    wait(cx, |cx| cx.update(|cx| !viewer.read(cx).busy));
    cx.update(|cx| {
        viewer.update(cx, |v, cx| {
            assert_eq!(
                v.view.form_inputs[1].read(cx).value().as_ref(),
                "form during refresh"
            );
            assert!(!v.view.grid.read(cx).model(cx).has_changes());
            v.view.form = false;
            v.view.form_for = None;
            v.view.form_inputs.clear();
            v.limit = Some(1);
            v.page = 1;
            v.reload(true, cx);
        })
    });
    wait(cx, |cx| cx.update(|cx| !viewer.read(cx).busy));

    // An owned transaction, including writes already accepted into its session, remains
    // open and rollbackable after another panel announces a database change.
    cx.update(|cx| viewer.update(cx, |v, cx| v.begin_transaction(cx)));
    wait(cx, |cx| cx.update(|cx| viewer.read(cx).in_transaction));
    navidog_db::runtime::runtime().block_on(async {
        run_on(session.clone(), |rel| {
            Box::pin(async move {
                rel.execute("UPDATE t SET name='transaction' WHERE id=3")
                    .await
            })
        })
        .await
        .unwrap()
        .unwrap();
    });
    notify_database_changed(cx, &id);
    cx.update(|cx| {
        let v = viewer.read(cx);
        assert!(v.in_transaction);
        assert!(!v.busy);
        assert!(Arc::ptr_eq(v.session.as_ref().unwrap(), &session));
    });
    navidog_db::runtime::runtime().block_on(async {
        run_on(session, |rel| {
            Box::pin(async move {
                let inside = rel.execute("SELECT name FROM t WHERE id=3").await?;
                assert_eq!(inside[0].rows, [vec![Some("transaction".into())]]);
                rel.execute("ROLLBACK").await
            })
        })
        .await
        .unwrap()
        .unwrap();
    });
    cx.update(|cx| viewer.update(cx, |v, cx| v.set_in_transaction(false, cx)));
    assert_eq!(
        sql(&file, "SELECT name FROM t WHERE id=3"),
        [vec![Some("latest".into())]]
    );
    notify_database_changed(cx, &id);
    wait(cx, |cx| cx.update(|cx| !viewer.read(cx).busy));
    cx.update(|cx| {
        assert_eq!(
            viewer.read(cx).view.grid.read(cx).model(cx).value(0, 1),
            Some(&Some("latest".into()))
        )
    });

    // Retire a session with an external refresh already queued on its lock. Its late
    // result must not replace the page, status, or busy state of the reopened viewer.
    let (retired_generation, retired_session) = cx.update(|cx| {
        let viewer = viewer.read(cx);
        (viewer.session_generation, viewer.session.clone().unwrap())
    });
    let held = navidog_db::runtime::runtime().block_on(retired_session.lock());
    notify_database_changed(cx, &id);
    cx.update(|cx| assert!(viewer.read(cx).busy));
    sql(&file, "UPDATE t SET name='after reopen' WHERE id=3");
    cx.update(|cx| {
        Sessions::global(cx).update(cx, |sessions, cx| {
            sessions.close_for_reopen(&id, cx);
            sessions.open(id.clone(), Secrets::default(), cx);
        });
    });
    drop(held);
    wait(cx, |cx| {
        cx.update(|cx| {
            let viewer = viewer.read(cx);
            viewer.session_generation != retired_generation
                && viewer.session.is_some()
                && !viewer.busy
        })
    });
    cx.update(|cx| {
        let viewer = viewer.read(cx);
        assert!(!Arc::ptr_eq(
            viewer.session.as_ref().unwrap(),
            &retired_session
        ));
        assert!(!viewer.external_refresh_pending);
        assert!(!viewer.view.grid.read(cx).model(cx).locked);
        assert_eq!(
            viewer.view.grid.read(cx).model(cx).value(0, 1),
            Some(&Some("after reopen".into()))
        );
        assert!(viewer.view.status.starts_with("SELECT"));
    });
}

#[gpui_kit::test]
fn where_text_collapses_a_pasted_line_break(cx: &mut TestAppContext) {
    let home = std::env::temp_dir().join(format!("navidog-filter-line-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    // SAFETY: this test does not run beside another that reads NAVIDOG_HOME.
    unsafe { std::env::set_var("NAVIDOG_HOME", &home) }
    let (handle, viewer) = cx.update(|cx| {
        gpui_kit::init(cx);
        navidog_app_state::init(cx);
        navidog_ui::theme::init(cx);
        navidog_session::init(cx);
        crate::init(cx);
        let mut viewer = None;
        let handle = gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            let entity = TableViewer::new(
                ObjectRef {
                    scope: Scope::new("missing", "main", None),
                    kind: ObjectKind::Table,
                    name: "t".into(),
                },
                window,
                cx,
            );
            viewer = Some(entity.clone());
            cx.new(|_| Host(entity))
        })
        .unwrap();
        (handle.0, viewer.unwrap())
    });
    cx.update_window(handle, |_, window, cx| {
        viewer.update(cx, |v, cx| {
            v.view.filter.update(cx, |input, cx| {
                input.replace_text_in_range(None, "a\nb", window, cx);
            });
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(viewer.read(cx).view.filter.read(cx).value().as_ref(), "a b");
    });
    let _ = std::fs::remove_dir_all(&home);
}
