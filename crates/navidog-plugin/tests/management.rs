//! Approval UI invokes the actual isolated runtime and publishes all declared contributions.
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, TestAppContext, WindowOptions};
use navidog_plugin::{Plugins, preferences::PluginPreferences};
#[gpui_kit::test]
fn discover_partial_approval_disable_and_reload_cleanup(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| cx.set_reduce_motion(true));
    let directory = tempfile::tempdir().unwrap();
    // SAFETY: this integration binary has one test, before the app is initialized.
    unsafe {
        std::env::set_var("NAVIDOG_HOME", directory.path());
    }
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/plugins/com.example.commands");
    let installed = directory.path().join("plugins/com.example.commands");
    std::fs::create_dir_all(&installed).unwrap();
    for name in ["gpui-shell.json", "navidog.json", "main.js", "panel.js"] {
        std::fs::copy(source.join(name), installed.join(name)).unwrap();
    }
    cx.update(|cx| {
        gpui_kit::init(cx);
        navidog_app_state::init(cx);
        navidog_session::init(cx);
        navidog_ui::theme::init(cx);
        navidog_ui::tabs::init(cx);
        navidog_plugin::init(cx);
    });
    let (handle, _) = cx
        .update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PluginPreferences::new)
            })
        })
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("plugin-select-com.example.commands", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("plugin-toggle", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("grant-editor.write", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("plugin-confirm", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        let plugins = Plugins::global(cx);
        assert!(
            plugins.read(cx).installations()[0].is_enabled(),
            "{:?}",
            plugins.read(cx).installations()[0].failure()
        );
        let approval = plugins.read(cx).approval("com.example.commands");
        assert!(!approval.granted.contains("editor.write"));
        assert!(approval.granted.contains("grid.read"));
        assert_eq!(
            cx.global::<navidog_app_state::plugin::Contributions>()
                .commands
                .len(),
            5
        );
    });
    let (panel_window, _) = cx
        .update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(|cx| {
                    navidog_plugin::panel::PluginPanel::new(
                        "com.example.commands/help".into(),
                        "Help".into(),
                        cx,
                    )
                })
            })
        })
        .unwrap();
    cx.update_window(panel_window, |_, window, cx| {
        window.render_frame(cx);
        window.click("plugin-about", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(panel_window, |_, window, cx| {
        use gpui_kit::component::WindowExt as _;
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx));
        window.close_dialog(cx);
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        Plugins::global(cx).update(cx, |plugins, cx| {
            plugins.reload("com.example.commands", window, cx).unwrap()
        })
    })
    .unwrap();
    cx.read(|cx| {
        assert_eq!(
            cx.global::<navidog_app_state::plugin::Contributions>()
                .commands
                .len(),
            5
        )
    });
    let manifest = installed.join("navidog.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    value["permissions"]
        .as_array_mut()
        .unwrap()
        .push("connections.read".into());
    std::fs::write(&manifest, value.to_string()).unwrap();
    cx.update_window(handle, |_, window, cx| {
        assert!(
            Plugins::global(cx)
                .update(cx, |plugins, cx| plugins.reload(
                    "com.example.commands",
                    window,
                    cx
                ))
                .is_err()
        )
    })
    .unwrap();
    cx.read(|cx| {
        assert!(
            !Plugins::global(cx)
                .read(cx)
                .approval("com.example.commands")
                .enabled
        );
        assert!(
            cx.global::<navidog_app_state::plugin::Contributions>()
                .commands
                .is_empty()
        );
    });
    cx.update(|cx| {
        Plugins::global(cx).update(cx, |p, cx| p.disable("com.example.commands", cx).unwrap())
    });
    cx.read(|cx| {
        assert!(
            cx.global::<navidog_app_state::plugin::Contributions>()
                .commands
                .is_empty()
        )
    });
    cx.read(|cx| {
        assert!(
            Plugins::global(cx)
                .read(cx)
                .panel("com.example.commands/help")
                .is_none()
        )
    });
}
