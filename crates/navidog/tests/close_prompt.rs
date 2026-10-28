//! Headless test of closing a table viewer tab with an uncommitted edit: ⌘W asks first and keeps
//! the tab, and confirming the prompt writes the edit and then closes it.

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AnyWindowHandle, AppContext as _, ElementId, Focusable as _, TestAppContext};
use navidog::main_window::MainWindow;
use navidog::state::AppState;
use navidog_app_state::actions::{NewQuery, OpenObject};
use navidog_app_state::store::ConnectionStore;
use navidog_db::{ConnectParams, Secrets, connect, runtime};
use navidog_model::connection::Connection;
use navidog_model::crypto::Key;
use navidog_model::object::{ObjectKind, ObjectRef, Scope};
use navidog_session::sessions::Sessions;
use navidog_sqlite::SqliteDriver;
use navidog_ui::tabs::{self, Close, CloseTab, CloseTabs};

fn home_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
use navidog_ui::tokens;

fn fit(rem: gpui_kit::Pixels, header: &str, values: &[&str]) -> gpui_kit::Pixels {
    navidog_grid::grid::auto_width(
        header,
        values.iter().copied(),
        tokens::font::SIZE_BODY.to_pixels(rem).as_f32(),
    )
}

fn wait(cx: &mut TestAppContext, what: &str, done: impl Fn(&mut TestAppContext) -> bool) {
    navidog_test_support::wait_with(
        cx,
        what,
        Duration::from_secs(30),
        Duration::from_millis(20),
        true,
        done,
    );
}

fn sql(file: &std::path::Path, statements: &str) -> Vec<Vec<Option<String>>> {
    runtime::runtime().block_on(async {
        let params = ConnectParams {
            file: Some(file.to_path_buf()),
            create_new: !file.exists(),
            ..Default::default()
        };
        let mut s = connect(Arc::new(SqliteDriver::default()), params)
            .await
            .unwrap();
        let sets = s.relational().unwrap().execute(statements).await.unwrap();
        s.close().await.unwrap();
        sets.into_iter()
            .last()
            .map(|set| set.rows)
            .unwrap_or_default()
    })
}

fn tab_count(cx: &mut TestAppContext) -> usize {
    cx.update(|cx| {
        tabs::group_of(tabs::active(cx).unwrap(), cx)
            .unwrap()
            .read(cx)
            .panels()
            .len()
    })
}

fn has(cx: &mut TestAppContext, handle: AnyWindowHandle, id: impl Into<ElementId>) -> bool {
    let id = id.into();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.try_find(id).is_some()
    })
    .unwrap()
}

#[gpui_kit::test]
fn closing_a_tab_with_an_edit_asks_and_commits(cx: &mut TestAppContext) {
    let _guard = home_lock();
    cx.update(|cx| cx.set_reduce_motion(true));
    let home = std::env::temp_dir().join(format!("navidog-close-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    let file = home.join("close.db");
    sql(
        &file,
        "CREATE TABLE t(id INTEGER PRIMARY KEY, name TEXT); INSERT INTO t(name) VALUES ('a')",
    );
    // SAFETY: this test binary has a single test; nothing else reads the environment concurrently
    unsafe { std::env::set_var("NAVIDOG_HOME", &home) };
    cx.update(navidog::init);
    let id = cx.update(|cx| {
        ConnectionStore::global(cx).update(cx, |s, cx| {
            s.set_key(Key::random());
            let mut c = Connection::new("sqlite");
            c.name = "close".into();
            c.general.file = Some(file.to_string_lossy().into_owned());
            let id = c.id.clone();
            s.upsert(c, &[], cx).unwrap();
            id
        })
    });
    let (handle, main) = cx.update(|cx| {
        let mut options = navidog::main_window_options(&AppState::default(), cx);
        options.show = false;
        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| MainWindow::new(AppState::default(), window, cx))
        })
        .unwrap()
    });
    cx.executor().allow_parking();
    cx.update(|cx| {
        Sessions::global(cx).update(cx, |s, cx| s.open(id.clone(), Secrets::default(), cx))
    });
    wait(cx, "the connection to open", |cx| {
        cx.update(|cx| Sessions::global(cx).read(cx).state(&id).is_open())
    });
    let table = ObjectRef {
        scope: Scope::new(&id, "main", None),
        kind: ObjectKind::Table,
        name: "t".into(),
    };
    let dispatch = |cx: &mut TestAppContext, action: Box<dyn gpui_kit::Action>| {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.focus(&main.focus_handle(cx), cx);
            window.dispatch_action(action, cx);
        })
        .unwrap();
        cx.run_until_parked();
    };
    dispatch(cx, Box::new(OpenObject { object: table }));
    wait(cx, "the table viewer's rows", |cx| {
        has(cx, handle, ElementId::NamedInteger("row".into(), 0))
    });
    for _ in 0..10 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
    let tabs_before = tab_count(cx);

    // Edit the name of row 1 and stay on the row, so nothing commits yet
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let rem = window.rem_size();
        let x = tokens::grid::ROW_NUMBER_WIDTH.to_pixels(rem)
            + fit(rem, "id", &["1"])
            + fit(rem, "name", &["a"]) / 2.;
        window.click_at(
            ElementId::NamedInteger("row".into(), 0),
            gpui_kit::point(x, rem / 2.),
            cx,
        );
        window.press("enter", cx);
        window.render_frame(cx);
        // `secondary` is cmd on macOS and ctrl elsewhere, matching the text field.
        window.press("secondary-a", cx);
        window.input("z", cx);
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .unwrap();

    // ⌘W asks instead of closing
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.dispatch_action(Box::new(CloseTab), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        tab_count(cx),
        tabs_before,
        "the tab stays while the prompt is open"
    );
    assert!(has(cx, handle, "close-commit"), "the close prompt is shown");
    assert_eq!(
        sql(&file, "SELECT name FROM t WHERE id = 1"),
        [vec![Some("a".to_string())]]
    );

    // Confirm writes the edit, then the tab closes. The prompt's OK action is Enter
    // (`on_ok`); a headless click on the footer button does not reach it. Let the
    // dialog finish sliding in before the key, or the entrance frame eats it.
    cx.executor().advance_clock(Duration::from_millis(300));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press("enter", cx);
    })
    .unwrap();
    wait(cx, "the commit and the tab to close", |cx| {
        tab_count(cx) == tabs_before - 1
    });
    assert_eq!(
        sql(&file, "SELECT name FROM t WHERE id = 1"),
        [vec![Some("z".to_string())]]
    );

    // Close All asks each dirty query, and Cancel stops the rest. Same process, so this
    // stays in the test above: `init` cannot run twice.
    let dispatch = |cx: &mut TestAppContext, action: Box<dyn gpui_kit::Action>| {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.focus(&main.focus_handle(cx), cx);
            window.dispatch_action(action, cx);
        })
        .unwrap();
        cx.run_until_parked();
    };
    let dirty = |cx: &mut TestAppContext, text: &str| {
        let text = text.to_string();
        cx.update_window(handle, |_, window, cx| {
            navidog_query::active_editor(cx)
                .expect("query editor")
                .update(cx, |editor, cx| editor.set_text(&text, window, cx));
        })
        .unwrap();
    };
    dispatch(cx, Box::new(NewQuery));
    dirty(cx, "select 1");
    dispatch(cx, Box::new(NewQuery));
    dirty(cx, "select 2");
    let tabs_before = tab_count(cx);

    dispatch(
        cx,
        Box::new(CloseTabs {
            panel: None,
            which: Close::All,
        }),
    );
    assert!(
        has(cx, handle, "query-close-discard"),
        "the first dirty query asks"
    );
    assert_eq!(
        tab_count(cx),
        tabs_before,
        "nothing dirty closes before an answer"
    );

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("query-close-discard", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(tab_count(cx), tabs_before - 1, "discard closes that query");
    assert!(
        has(cx, handle, "query-close-discard"),
        "the next dirty query asks"
    );

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("query-close-keep", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        tab_count(cx),
        tabs_before - 1,
        "cancel leaves the remaining query"
    );
    assert!(
        !has(cx, handle, "query-close-discard"),
        "cancel stops the rest of the prompts"
    );
    let _ = std::fs::remove_dir_all(&home);
}
