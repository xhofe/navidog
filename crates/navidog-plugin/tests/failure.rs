//! A service task ending is detected without rendering its view or re-entering the VM.
use gpui_kit::{AppContext as _, TestAppContext, WindowOptions};
use navidog_plugin::{manifest::InstalledPlugin, protocol::Bridge, runtime::PluginRuntime};
#[gpui_kit::test]
fn service_task_failure_revokes_transport(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("gpui-shell.json"),
        r#"{"id":"com.example.failure","name":"Failure","entry":"main.js"}"#,
    )
    .unwrap();
    std::fs::write(directory.path().join("navidog.json"), r#"{"api":1}"#).unwrap();
    std::fs::write(directory.path().join("main.js"),"import {View,div} from 'gpui-kit'; export default class Service extends View {init(_props,cx){cx.spawn(()=>{for(;;){} });}render(){return div();}}").unwrap();
    let installed = InstalledPlugin::read(directory.path()).unwrap();
    let bridge = Bridge::new(1, Default::default()).unwrap();
    let module = navidog_plugin::host::module(
        installed.shell().id(),
        bridge.clone(),
        &Default::default(),
        "en",
    );
    cx.update(|cx| {
        gpui_kit::init(cx);
        gpui_shell::init(cx);
    });
    let (window, _) = cx
        .update(|cx| gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Empty)))
        .unwrap();
    let runtime = cx
        .update_window(window, |_, window, cx| {
            PluginRuntime::load(
                &installed,
                &Default::default(),
                directory.path(),
                bridge.clone(),
                module,
                window,
                cx,
            )
        })
        .unwrap()
        .unwrap();
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(2));
    cx.run_until_parked();
    assert!(!bridge.is_active());
    drop(runtime);
}
struct Empty;
impl gpui_kit::Render for Empty {
    fn render(
        &mut self,
        _: &mut gpui_kit::Window,
        _: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        gpui_kit::div()
    }
}
