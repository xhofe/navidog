//! Service applications receive events without rendering under an isolated frozen grant.
use gpui_kit::{AppContext as _, TestAppContext, WindowOptions};
use gpui_shell::{HostModule, HostValue};
use navidog_plugin::{manifest::InstalledPlugin, protocol::Bridge, runtime::PluginRuntime};
use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[gpui_kit::test]
fn unrendered_service_consumes_events_and_freezes_partial_permissions(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("gpui-shell.json"),r#"{"id":"com.example.runtime","name":"Runtime","entry":"main.js","shell-version":"0.8.1","capabilities":{"clipboard":{"read":true,"write":true}}}"#).unwrap();
    std::fs::write(directory.path().join("navidog.json"), r#"{"api":1}"#).unwrap();
    std::fs::write(
        directory.path().join("main.js"),
        r#"
import {View,div} from 'gpui-kit';
import {next_event,seen} from 'navidog';
export default class Service extends View {
 init(_props,cx) {cx.spawn(async ()=>{for(;;){const event=await next_event();seen(event.kind);}});}
 render(){throw new Error('Service must not render');}
}
"#,
    )
    .unwrap();
    let installed = InstalledPlugin::read(directory.path()).unwrap();
    let bridge = Bridge::new(9, Default::default()).unwrap();
    let events = bridge.clone();
    let count = Arc::new(AtomicUsize::new(0));
    let seen = count.clone();
    let module = HostModule::new("navidog")
        .function("runtime_failed", |_| Ok(HostValue::Null))
        .async_function("next_event", move |_| {
            let events = events.clone();
            Ok(async move {
                let event = events
                    .next_event()
                    .await
                    .map_err(|e| gpui_shell::HostError::new(e.to_string()))?;
                Ok(HostValue::Object(vec![(
                    "kind".into(),
                    HostValue::Str(event["kind"].as_str().unwrap().into()),
                )]))
            })
        })
        .function("seen", move |_| {
            seen.fetch_add(1, Ordering::SeqCst);
            Ok(HostValue::Null)
        });
    cx.update(|cx| {
        gpui_kit::init(cx);
        gpui_shell::init(cx);
    });
    let (handle, _) = cx
        .update(|cx| gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Empty)))
        .unwrap();
    let runtime = cx
        .update_window(handle, |_, window, cx| {
            PluginRuntime::load(
                &installed,
                &BTreeSet::from(["shell.clipboard.read=true".into()]),
                directory.path(),
                bridge.clone(),
                module,
                window,
                cx,
            )
        })
        .unwrap()
        .unwrap();
    bridge.event(serde_json::json!({"kind":"command"})).unwrap();
    for _ in 0..50 {
        cx.run_until_parked();
        if count.load(Ordering::SeqCst) > 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.metrics().script_renders(), 0);
    cx.read(|cx| {
        let authority = runtime.authority(cx);
        assert!(authority.capabilities().is_clipboard_readable());
        assert!(!authority.capabilities().is_clipboard_writable());
    });
    drop(runtime);
    assert!(!bridge.is_active());
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
