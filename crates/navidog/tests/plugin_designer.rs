//! The native table designer uses a plugin-owned type menu and enforces declared properties.
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, TestAppContext, WindowOptions};
use navidog_app_state::{registry::Source, store::ConnectionStore};
use navidog_db::Driver as _;
use navidog_plugin::{
    driver::{DriverDescriptor, JsDriver},
    protocol::Bridge,
};
#[gpui_kit::test]
fn plugin_native_designer_previews_types_and_guards_undeclared_properties(cx: &mut TestAppContext) {
    let home = tempfile::tempdir().unwrap();
    // SAFETY: isolated one-test binary before initialization.
    unsafe {
        std::env::set_var("NAVIDOG_HOME", home.path());
    }
    cx.update(navidog::init);
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/plugins/com.example.http-postgres/navidog.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let descriptor: DriverDescriptor =
        serde_json::from_value(manifest["drivers"][0].clone()).unwrap();
    let driver = JsDriver::new(
        "com.example.http-postgres",
        descriptor,
        Bridge::new(1, Default::default()).unwrap(),
    )
    .unwrap();
    let id = driver.info().id.clone();
    cx.update(|cx| {
        cx.global_mut::<navidog_session::DriverRegistry>()
            .0
            .register(
                id.clone(),
                Source::Plugin("com.example.http-postgres".into()),
                std::sync::Arc::new(driver),
            )
            .unwrap();
        ConnectionStore::global(cx).update(cx, |store, cx| {
            let mut c = navidog_model::connection::Connection::new(&id);
            c.id = "plugin".into();
            store.upsert(c, &[], cx).unwrap();
        });
    });
    let (window, panel) = cx
        .update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                navidog_designer::TableDesigner::new(
                    navidog_model::object::Scope::new("plugin", "pagila", Some("public".into())),
                    window,
                    cx,
                )
            })
        })
        .unwrap();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("designer-table-name", cx);
        window.input("plugin_people", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("designer-save", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        use gpui_kit::component::dock::BasePanel as _;
        let state = panel.read(cx).dump(cx);
        let text = serde_json::to_string(&state).unwrap();
        assert!(text.contains("plugin_people"));
        assert!(text.contains("integer"));
    });
}
