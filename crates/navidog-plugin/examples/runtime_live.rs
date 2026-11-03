//! Live executor acceptance of concurrent JS fetch cancellation. Run with the compose fixtures.
use gpui_kit::{AppContext as _, Context, IntoElement, Render, Window, WindowOptions};
use navidog_db::{ConnectParams, Driver};
use navidog_plugin::{
    driver::{DriverDescriptor, JsDriver},
    manifest::InstalledPlugin,
    protocol::Bridge,
    runtime::PluginRuntime,
};
use std::sync::{Arc, Mutex};
#[path = "http_server.rs"]
#[allow(dead_code)]
mod gateway;
struct Acceptance {
    runtime: Option<PluginRuntime>,
}
impl Render for Acceptance {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui_kit::div()
    }
}
fn main() {
    let outcome = Arc::new(Mutex::new(None));
    let observed = outcome.clone();
    let retained = std::rc::Rc::new(std::cell::RefCell::new(None));
    gpui_kit::application()
        .with_assets(navidog_ui::NavidogAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            navidog_app_state::init(cx);
            navidog_session::init(cx);
            gpui_shell::init(cx);
            let server = navidog_db::runtime::spawn(async {
                gateway::serve(
                    tokio::net::TcpListener::bind("127.0.0.1:18123")
                        .await
                        .unwrap(),
                )
                .await;
            });
            let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/plugins/com.example.http-postgres");
            let installed = InstalledPlugin::read(&directory).unwrap();
            let granted = installed.requested();
            let bridge = Bridge::new(1, Default::default()).unwrap();
            let (_, view) = gpui_kit::open_window(
                WindowOptions {
                    show: false,
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    let runtime = PluginRuntime::load(
                        &installed,
                        &granted,
                        &std::env::temp_dir(),
                        bridge.clone(),
                        navidog_plugin::host::module(
                            installed.shell().id(),
                            bridge.clone(),
                            &granted,
                            "en",
                        ),
                        window,
                        cx,
                    )
                    .unwrap();
                    cx.new(|_| Acceptance {
                        runtime: Some(runtime),
                    })
                },
            )
            .unwrap();
            *retained.borrow_mut() = Some(view.clone());
            let descriptor: DriverDescriptor =
                serde_json::from_value(installed.extensions().drivers[0].clone()).unwrap();
            let driver = JsDriver::new(installed.shell().id(), descriptor, bridge).unwrap();
            let operation = navidog_db::runtime::spawn(async move {
                let mut session = driver
                    .connect(ConnectParams {
                        host: "127.0.0.1".into(),
                        port: 15432,
                        user: "postgres".into(),
                        password: Some("navidog".into()),
                        database: Some("pagila".into()),
                        ..Default::default()
                    })
                    .await?;
                let cancel = session.cancel_handle().unwrap();
                let cancelling = navidog_db::runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    cancel().await
                });
                let started = std::time::Instant::now();
                let result = session
                    .relational()
                    .unwrap()
                    .execute("SELECT pg_sleep(10)")
                    .await;
                cancelling.await.unwrap()?;
                assert!(result.is_err(), "Slow query was not cancelled");
                assert!(
                    started.elapsed() < std::time::Duration::from_secs(5),
                    "Cancellation took too long"
                );
                session.close().await?;
                Ok::<_, navidog_db::DbError>(started.elapsed())
            });
            let outcome = outcome.clone();
            cx.activate(true);
            view.update(cx, |_, cx| {
                cx.spawn(async move |this, cx| {
                    let result = operation
                        .await
                        .map_err(|error| error.to_string())
                        .and_then(|result| result.map_err(|error| error.to_string()));
                    eprintln!("Live JS HTTP cancellation result: {result:?}");
                    if result.is_err() {
                        std::process::exit(1);
                    }
                    *outcome.lock().unwrap() = Some(result);
                    let _ = this.update(cx, |this, _| {
                        this.runtime.take();
                    });
                    server.abort();
                    cx.update(|cx| cx.quit());
                })
                .detach()
            });
        });
    match observed
        .lock()
        .unwrap()
        .take()
        .expect("No live runtime result")
    {
        Ok(elapsed) => println!("Live JS HTTP cancellation passed in {elapsed:?}"),
        Err(error) => panic!("{error}"),
    }
}
