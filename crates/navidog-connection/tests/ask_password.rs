//! Saving the password can fail without failing the connection that was just typed.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui_kit::component::WindowExt as _;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AppContext as _, IntoElement, Render, Styled as _, TestAppContext, Window, WindowBounds,
    WindowOptions, div, size,
};
use navidog_app_state::store::ConnectionStore;
use navidog_model::connection::Connection;
use navidog_model::crypto::Key;
use navidog_model::paths::config_dir;

struct Host;

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut gpui_kit::Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

#[gpui_kit::test]
fn a_failed_save_still_connects_once(cx: &mut TestAppContext) {
    let home = std::env::temp_dir().join(format!("navidog-ask-password-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    // SAFETY: this file is its own test binary, and the directory is unique to this process.
    unsafe { std::env::set_var("NAVIDOG_HOME", &home) };

    let id = cx.update(|cx| {
        gpui_kit::init(cx);
        navidog_app_state::init(cx);
        navidog_ui::theme::init(cx);
        navidog_connection::init(cx);
        cx.set_reduce_motion(true);
        ConnectionStore::global(cx).update(cx, |store, cx| {
            store.set_key(Key::random());
            let mut connection = Connection::new("mysql");
            connection.name = "ask".into();
            connection.general.save_password = false;
            let id = connection.id.clone();
            store.upsert(connection, &[], cx).unwrap();
            id
        })
    });
    let path = config_dir().join("connections.json");
    let before = std::fs::read(&path).unwrap();
    if !block_writes(&path, true) {
        return;
    }

    let hits = Rc::new(Cell::new(0usize));
    let password = Rc::new(RefCell::new(None));
    let (handle, _) = cx.update(|cx| {
        // raw-px: test window size
        let test_size = size(gpui_kit::px(800.), gpui_kit::px(600.));
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(gpui_kit::Bounds {
                origin: Default::default(),
                size: test_size,
            })),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |_, cx| cx.new(|_| Host)).unwrap()
    });
    let seen = hits.clone();
    let saved = password.clone();
    cx.update_window(handle, |_, window, cx| {
        navidog_connection::resolve(id.clone(), window, cx, move |secrets, _, _| {
            seen.set(seen.get() + 1);
            *saved.borrow_mut() = secrets.password;
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.input("s3cret", cx);
        window.click("save", cx);
        window.click("ok", cx);
    })
    .unwrap();
    cx.run_until_parked();

    assert_eq!(hits.get(), 1);
    assert_eq!(password.borrow().as_deref(), Some("s3cret"));
    cx.update_window(handle, |_, window, cx| {
        assert_eq!(window.notifications(cx).len(), 1);
    })
    .unwrap();
    cx.update(|cx| {
        let conn = ConnectionStore::global(cx)
            .read(cx)
            .get(&id)
            .unwrap()
            .clone();
        assert!(!conn.general.save_password);
        assert!(conn.general.password.is_none());
    });
    assert_eq!(std::fs::read(&path).unwrap(), before);
    block_writes(&path, false);
}

fn block_writes(path: &std::path::Path, block: bool) -> bool {
    if !block {
        restore_writes(path);
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let dir = path.parent().unwrap();
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o500)).unwrap();
        let probe = dir.join(format!(".navidog-write-probe-{}", std::process::id()));
        let blocked = std::fs::File::create(&probe).is_err();
        let _ = std::fs::remove_file(&probe);
        if !blocked {
            restore_writes(path);
        }
        blocked
    }
    #[cfg(not(unix))]
    {
        let mut permissions = std::fs::metadata(path).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(path, permissions).unwrap();
        let blocked = std::fs::OpenOptions::new().write(true).open(path).is_err();
        if !blocked {
            restore_writes(path);
        }
        blocked
    }
}

#[allow(clippy::permissions_set_readonly_false)] // test helper; Windows needs the attribute cleared
fn restore_writes(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let dir = path.parent().unwrap();
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    {
        let Ok(mut permissions) = std::fs::metadata(path).map(|m| m.permissions()) else {
            return;
        };
        permissions.set_readonly(false);
        let _ = std::fs::set_permissions(path, permissions);
    }
}
