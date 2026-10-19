use crate::panel::TableDesigner;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    TestAppContext, Window, WindowOptions,
};
use navidog_core::object::Scope;

fn isolate_home(name: &str) -> (std::sync::MutexGuard<'static, ()>, std::path::PathBuf) {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let home = std::env::temp_dir().join(format!("navidog-designer-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    // SAFETY: every test in this process holds LOCK while accessing the config directory.
    unsafe {
        std::env::set_var("NAVIDOG_HOME", &home);
    }
    (guard, home)
}

struct Host(Entity<TableDesigner>);
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui_kit::div().size_full().child(self.0.clone())
    }
}

#[gpui_kit::test]
fn edits_fields_previews_sql_and_restores_drafts(cx: &mut TestAppContext) {
    let (_guard, _) = isolate_home("draft");
    cx.update(|cx| {
        gpui_kit::init(cx);
        navidog_core::init(cx);
        navidog_ui::theme::init(cx);
        navidog_ui::tabs::init(cx);
        navidog_db::init(cx);
        crate::init(cx);
    });
    let (handle, panel) = cx.update(|cx| {
        let mut panel = None;
        let window = gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            let p = TableDesigner::new(Scope::new("test", "main", None), window, cx);
            panel = Some(p.clone());
            cx.new(|_| Host(p))
        })
        .unwrap();
        (window.0, panel.unwrap())
    });
    assert_ne!(rust_i18n::t!("designer.save").to_string(), "designer.save");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("designer-table-name", cx);
        window.input("people", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.click("designer-add", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.click("designer-field-name", cx);
        // Input uses Command on macOS and Control on Linux/Windows.
        window.press("secondary-a", cx);
        window.input("display_name", cx);
        window.click("designer-field-definition", cx);
        window.press("secondary-a", cx);
        window.input("TEXT NOT NULL DEFAULT 'guest'", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.click("designer-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    panel.update(cx, |panel, cx| {
        use gpui_kit::component::dock::BasePanel as _;
        let dumped = panel.dump(cx);
        let gpui_kit::component::dock::PanelInfo::Panel(value) = dumped.info else {
            panic!("saved draft")
        };
        let saved: super::Saved = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(saved.draft.columns.len(), 2);
        assert_eq!(value["draft"]["name"], "people");
        assert_eq!(value["draft"]["columns"][1]["name"], "display_name");
        assert_eq!(
            value["draft"]["columns"][1]["definition"],
            "TEXT NOT NULL DEFAULT 'guest'"
        );
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_ne!(window.find("designer-execute").disabled(), Some(true));
        use gpui_kit::component::dock::{BasePanel as _, PanelInfo};
        let PanelInfo::Panel(value) = panel.read(cx).dump(cx).info else {
            panic!("draft")
        };
        let restored = TableDesigner::restore(serde_json::from_value(value).unwrap(), window, cx);
        assert_eq!(restored.read(cx).draft.columns[1].name, "display_name");
        assert_eq!(
            restored.read(cx).definition.read(cx).value(),
            "INTEGER NOT NULL"
        );
        assert!(restored.read(cx).dirty());
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.click("designer-page-fields", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.click("designer-field-definition", cx);
        window.press("cmd-d", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let added = cx.update(|cx| {
        let p = panel.read(cx);
        assert_eq!(p.draft.columns.len(), 3);
        p.selected.unwrap()
    });
    cx.update_window(handle, |_, window, cx| {
        window.click("designer-field-name", cx);
        window.press("cmd-up", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(panel.read(cx).draft.columns[1].id, added));
    cx.update_window(handle, |_, window, cx| {
        window.click("designer-delete", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(panel.read(cx).draft.columns.len(), 2));
}

#[gpui_kit::test]
fn saves_only_after_preview_and_keeps_failed_draft(cx: &mut TestAppContext) {
    let (_guard, home) = isolate_home("save");
    use navidog_core::{connection::Connection, crypto::Key, store::ConnectionStore};
    use navidog_db::{ConnectParams, Secrets, sessions::Sessions};
    use std::sync::Arc;
    cx.executor().allow_parking();
    cx.update(|cx| cx.set_reduce_motion(true));
    let file = home.join(format!(
        "{}.db",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let sql = |query: &str| {
        navidog_db::runtime::runtime().block_on(async {
            let mut s = navidog_db::connect(
                Arc::new(navidog_db::sqlite::SqliteDriver::default()),
                ConnectParams {
                    file: Some(file.clone()),
                    create_new: true,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            let result = s.relational().unwrap().execute(query).await;
            s.close().await.unwrap();
            result.unwrap()
        })
    };
    sql("SELECT 1");
    let id = cx.update(|cx| {
        gpui_kit::init(cx);
        navidog_core::init(cx);
        navidog_ui::theme::init(cx);
        navidog_ui::tabs::init(cx);
        navidog_db::init(cx);
        crate::init(cx);
        ConnectionStore::global(cx).update(cx, |store, cx| {
            store.set_key(Key::random());
            let mut connection = Connection::new("sqlite");
            connection.name = "designer-test".into();
            connection.general.file = Some(file.to_string_lossy().into_owned());
            let id = connection.id.clone();
            store.upsert(connection, &[], cx).unwrap();
            id
        })
    });
    cx.update(|cx| {
        Sessions::global(cx).update(cx, |s, cx| s.open(id.clone(), Secrets::default(), cx))
    });
    let wait = |cx: &mut TestAppContext, done: &dyn Fn(&mut TestAppContext) -> bool| {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !done(cx) {
            assert!(std::time::Instant::now() < end);
            cx.run_until_parked();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    };
    wait(cx, &|cx| {
        cx.update(|cx| Sessions::global(cx).read(cx).state(&id).is_open())
    });
    let (handle, panel) = cx.update(|cx| {
        let mut panel = None;
        let handle = gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            let p = TableDesigner::new(Scope::new(id, "main", None), window, cx);
            panel = Some(p.clone());
            cx.new(|_| Host(p))
        })
        .unwrap();
        (handle.0, panel.unwrap())
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("designer-table-name", cx);
        window.input("people", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.click("designer-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(
        sql("SELECT name FROM sqlite_schema WHERE name='people'")[0]
            .rows
            .is_empty()
    );
    cx.update_window(handle, |_, window, cx| {
        window.click("designer-execute", cx);
    })
    .unwrap();
    cx.run_until_parked();
    wait(cx, &|cx| cx.update(|cx| !panel.read(cx).busy));
    cx.update(|cx| {
        assert!(panel.read(cx).baseline.is_some());
        assert!(!panel.read(cx).dirty());
    });
    sql("INSERT INTO people(id) VALUES(1)");
    cx.update_window(handle, |_, window, cx| {
        window.click("designer-page-fields", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.click("designer-add", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.click("designer-field-definition", cx);
        window.press("secondary-a", cx);
        window.input("TEXT NOT NULL", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        let panel = panel.read(cx);
        let selected = panel.selected.unwrap();
        assert_eq!(
            panel
                .draft
                .columns
                .iter()
                .find(|c| c.id == selected)
                .unwrap()
                .definition,
            "TEXT NOT NULL"
        );
    });
    cx.update_window(handle, |_, window, cx| {
        window.click("designer-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.click("designer-execute", cx);
    })
    .unwrap();
    cx.run_until_parked();
    wait(cx, &|cx| cx.update(|cx| !panel.read(cx).busy));
    cx.update(|cx| {
        let p = panel.read(cx);
        assert!(p.error.is_some());
        assert!(p.dirty());
        assert_eq!(p.draft.columns.len(), 2);
        assert_eq!(p.baseline.as_ref().unwrap().design.columns.len(), 1);
    });
    assert_eq!(
        sql("SELECT count(*) FROM people")[0].rows[0][0].as_deref(),
        Some("1")
    );
}
