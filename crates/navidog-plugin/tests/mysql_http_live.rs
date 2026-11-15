//! Complete Shell -> HTTP -> PostgreSQL path, with an unrendered service VM.
use gpui_kit::{AppContext as _, TestAppContext, WindowOptions};
use navidog_db::{ConnectParams, Driver};
use navidog_plugin::{
    driver::{DriverDescriptor, JsDriver},
    manifest::InstalledPlugin,
    protocol::Bridge,
    runtime::PluginRuntime,
};
use std::{collections::BTreeSet, sync::Arc};
#[path = "../examples/http_server.rs"]
mod gateway;

#[gpui_kit::test]
fn mysql_http_nontransactional_failure_reloads_without_replay(cx: &mut TestAppContext) {
    if std::env::var_os("NAVIDOG_LIVE").is_none() {
        return;
    }
    cx.executor().allow_parking();
    let server = navidog_db::runtime::spawn(async {
        gateway::serve(
            tokio::net::TcpListener::bind("127.0.0.1:18123")
                .await
                .unwrap(),
        )
        .await;
    });
    let directory = tempfile::tempdir().unwrap();
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/plugins/com.example.http-mysql");
    for name in ["gpui-shell.json", "navidog.json", "main.js"] {
        std::fs::copy(source.join(name), directory.path().join(name)).unwrap();
    }
    let installed = InstalledPlugin::read(directory.path()).unwrap();
    let granted: BTreeSet<_> = installed.requested();
    let bridge = Bridge::new(1, Default::default()).unwrap();
    let module =
        navidog_plugin::host::module(installed.shell().id(), bridge.clone(), &granted, "en");
    cx.update(|cx| {
        gpui_kit::init(cx);
        navidog_app_state::init(cx);
        navidog_session::init(cx);
        gpui_shell::init(cx);
    });
    let (handle, _) = cx
        .update(|cx| gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Empty)))
        .unwrap();
    let runtime = cx
        .update_window(handle, |_, window, cx| {
            PluginRuntime::load(
                &installed,
                &granted,
                directory.path(),
                bridge.clone(),
                module,
                window,
                cx,
            )
        })
        .unwrap()
        .unwrap();
    let descriptor: DriverDescriptor =
        serde_json::from_value(installed.extensions().drivers[0].clone()).unwrap();
    let driver = JsDriver::new(installed.shell().id(), descriptor, bridge).unwrap();
    let result = Arc::new(std::sync::Mutex::new(None));
    let done = result.clone();
    let task = navidog_db::runtime::spawn(async move {
        let mut session = driver
            .connect(ConnectParams {
                host: "127.0.0.1".into(),
                port: 13306,
                user: "root".into(),
                password: Some("navidog".into()),
                database: Some("sakila".into()),
                ..Default::default()
            })
            .await?;
        let rel = session.relational().unwrap();
        let databases = rel.list_databases().await?;
        assert!(databases.iter().any(|d| d == "sakila"));
        let scope = navidog_model::object::Scope::new("http", "sakila", None);
        assert!(!rel.table_columns(&scope, "actor").await?.is_empty());
        assert!(!rel.outline(&scope, false).await?.tables.is_empty());
        let sets = rel.execute("SELECT 9007199254740993; SELECT 2").await?;
        assert_eq!(sets.len(), 2);
        assert_eq!(sets[0].rows[0][0].as_deref(), Some("9007199254740993"));
        rel.execute("DROP TABLE IF EXISTS sakila.navidog_plugin_m5; CREATE TABLE sakila.navidog_plugin_m5(id integer NOT NULL, PRIMARY KEY(id))").await?;
        let baseline = navidog_schema::table_design::load(rel, &scope, "navidog_plugin_m5").await?;
        let mut draft = baseline.design.clone();
        draft
            .columns
            .push(navidog_model::table_design::DesignColumn::new(
                99, "name", "text",
            ));
        let dialect = rel.dialect();
        let statements =
            navidog_schema::table_design::plan(dialect.as_ref(), &scope, Some(&baseline), &draft)?;
        let plan = navidog_schema::table_design::PreviewPlan::new(
            dialect.as_ref(),
            Some(baseline),
            draft.clone(),
            statements,
        );
        let saved = plan.execute(rel, &scope, &draft).await?;
        assert!(saved.design.columns.iter().any(|c| c.name == "name"));
        let baseline = navidog_schema::table_design::load(rel, &scope, "navidog_plugin_m5").await?;
        let mut next = baseline.design.clone();
        next.columns
            .push(navidog_model::table_design::DesignColumn::new(
                100, "partial", "int",
            ));
        let dialect = rel.dialect();
        let statements = vec![
            "ALTER TABLE sakila.navidog_plugin_m5 ADD COLUMN partial int".into(),
            "ALTER TABLE sakila.navidog_plugin_m5 ADD COLUMN partial int".into(),
        ];
        let plan = navidog_schema::table_design::PreviewPlan::new(
            dialect.as_ref(),
            Some(baseline),
            next.clone(),
            statements,
        );
        let error = plan.execute(rel, &scope, &next).await.unwrap_err();
        assert!(error.message.contains("completed 1 of 2"));
        let reloaded = navidog_schema::table_design::load(rel, &scope, "navidog_plugin_m5").await?;
        assert!(reloaded.design.columns.iter().any(|c| c.name == "partial"));
        assert!(plan.execute(rel, &scope, &next).await.is_err());
        rel.execute("DROP TABLE sakila.navidog_plugin_m5").await?;
        session.close().await?;
        Ok::<_, navidog_db::DbError>(())
    });
    navidog_db::runtime::spawn(async move {
        *done.lock().unwrap() = Some(task.await.unwrap());
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while result.lock().unwrap().is_none() {
        assert!(
            std::time::Instant::now() < deadline,
            "HTTP plugin timed out"
        );
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    result.lock().unwrap().take().unwrap().unwrap();
    drop(runtime);
    server.abort();
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
