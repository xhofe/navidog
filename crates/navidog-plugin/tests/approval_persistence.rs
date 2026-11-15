//! A rejected settings file stays intact even when installed plugins are found.

use gpui_kit::{
    AppContext as _, Context, IntoElement, Render, TestAppContext, Window, WindowOptions,
};
use navidog_plugin::{
    Plugins,
    settings::{Approval, Limits, PluginsFile},
};

struct Empty;

impl Render for Empty {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui_kit::div()
    }
}

#[gpui_kit::test]
fn rejected_settings_are_preserved_until_restart(cx: &mut TestAppContext) {
    let app_config = tempfile::tempdir().unwrap();
    // SAFETY: This integration binary has one test, before app initialization.
    unsafe {
        std::env::set_var("NAVIDOG_HOME", app_config.path());
    }
    let (window, _) = cx
        .update(|cx| {
            gpui_kit::init(cx);
            navidog_app_state::init(cx);
            navidog_session::init(cx);
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Empty))
        })
        .unwrap();
    let id = "com.example.commands";
    for bytes in [
        b"{\"version\":1,".as_slice(),
        br#"{"version":2,"plugins":{}}"#,
        br#"{"version":1,"plugins":{"com.example.commands":{"limits":{"requests":0}}}}"#,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plugins.json");
        let root = dir.path().join("plugins");
        let installed = root.join(id);
        std::fs::create_dir_all(&installed).unwrap();
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/plugins")
            .join(id);
        for name in ["gpui-shell.json", "navidog.json", "main.js", "panel.js"] {
            std::fs::copy(source.join(name), installed.join(name)).unwrap();
        }
        std::fs::write(&path, bytes).unwrap();
        let error = PluginsFile::load(&path).unwrap_err();
        let plugins = cx.update(|cx| cx.new(|_| Plugins::new(root.clone())));
        cx.update(|cx| plugins.update(cx, |p, cx| p.scan(cx)));
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "startup scan");

        cx.update_window(window, |_, window, cx| {
            plugins.update(cx, |p, cx| {
                p.scan(cx);
                p.start_enabled(window, cx);
                assert_eq!(p.failure(), Some(error.as_str()));
                let [installation] = p.installations() else {
                    panic!("The plugin should still be discovered");
                };
                assert!(!installation.is_enabled());
                let manifest = installation.plugin().unwrap();
                let mut limits = Limits::default();
                limits.requests = 12;
                let approval = Approval::confirm(
                    manifest.requested(),
                    Default::default(),
                    manifest.fingerprint().into(),
                    limits,
                )
                .unwrap();
                assert_eq!(p.enable(id, approval, window, cx), Err(error.clone()));
                assert_eq!(p.disable(id, cx), Err(error.clone()));
                assert_eq!(p.reload(id, window, cx), Err(error.clone()));
            });
        })
        .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "rescan and edits");

        std::fs::write(
            &path,
            br#"{"version":1,"plugins":{"com.example.commands":{"limits":{"requests":12}}}}"#,
        )
        .unwrap();
        cx.update(|cx| {
            let restarted = cx.new(|_| Plugins::new(root));
            restarted.update(cx, |p, cx| {
                p.scan(cx);
                assert_eq!(p.failure(), None);
                p.disable(id, cx).unwrap();
            });
        });
        assert_eq!(
            PluginsFile::load(&path)
                .unwrap()
                .approval(id)
                .unwrap()
                .limits
                .requests,
            12
        );
    }
}
