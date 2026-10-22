use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use futures::FutureExt as _;

use super::*;
use crate::{CancelHandle, DriverInfo, Family, ResultSet};

async fn sessions_with_metadata(
    driver: Arc<dyn Driver>,
    params: ConnectParams,
) -> (Sessions, SharedSession) {
    let mut session = crate::connect(driver.clone(), params.clone())
        .await
        .unwrap();
    let dialect = session.relational().map(|rel| rel.dialect());
    let primary = Arc::new(Mutex::new(Some(session)));
    let primary_db = driver
        .info()
        .per_database_sessions
        .then(|| params.database.clone().unwrap_or_else(|| "postgres".into()));
    let mut sessions = Sessions::default();
    sessions.live.insert(
        "test".into(),
        Live {
            driver,
            params,
            dialect,
            primary: primary.clone(),
            primary_db,
            per_db: Default::default(),
            keepalive: None,
        },
    );
    (sessions, primary)
}

#[derive(Default)]
struct Calls {
    opened: AtomicUsize,
    submitted: AtomicUsize,
    closed: AtomicUsize,
    fail_connect: AtomicBool,
}

struct CountingDriver {
    info: DriverInfo,
    calls: Arc<Calls>,
}

impl CountingDriver {
    fn new(calls: Arc<Calls>) -> Self {
        Self {
            info: DriverInfo {
                id: "counting".into(),
                name: "Counting".into(),
                family: Family::Relational,
                default_port: None,
                supports_ssl: false,
                supports_ssh: false,
                per_database_sessions: false,
            },
            calls,
        }
    }
}

impl Driver for CountingDriver {
    fn info(&self) -> &DriverInfo {
        &self.info
    }

    fn connect(&self, _: ConnectParams) -> BoxFuture<'static, DbResult<Box<dyn Session>>> {
        let calls = self.calls.clone();
        async move {
            calls.opened.fetch_add(1, Ordering::SeqCst);
            if calls.fail_connect.load(Ordering::SeqCst) {
                return Err(DbError::coded("CONNECT", "connection refused"));
            }
            Ok(Box::new(CountingSession(calls)) as Box<dyn Session>)
        }
        .boxed()
    }
}

struct CountingSession(Arc<Calls>);

impl Session for CountingSession {
    fn server_version(&self) -> &str {
        "test"
    }

    fn ping(&mut self) -> BoxFuture<'_, DbResult<()>> {
        async { Ok(()) }.boxed()
    }

    fn cancel_handle(&self) -> Option<CancelHandle> {
        None
    }

    fn session_id(&self) -> Option<u64> {
        None
    }

    fn close(self: Box<Self>) -> BoxFuture<'static, DbResult<()>> {
        async move {
            self.0.closed.fetch_add(1, Ordering::SeqCst);
            Err(DbError::coded("CLOSE", "close response lost"))
        }
        .boxed()
    }

    fn relational(&mut self) -> Option<&mut dyn Relational> {
        Some(self)
    }
}

impl Relational for CountingSession {
    fn dialect(&self) -> Dialect {
        Dialect::Sqlite
    }

    fn execute<'a>(&'a mut self, _: &'a str) -> BoxFuture<'a, DbResult<Vec<ResultSet>>> {
        self.0.submitted.fetch_add(1, Ordering::SeqCst);
        async { Ok(Vec::new()) }.boxed()
    }
}

#[test]
fn operation_cleanup_preserves_success_and_failure_without_replaying() {
    runtime::runtime().block_on(async {
        let calls = Arc::new(Calls::default());
        let driver = Arc::new(CountingDriver::new(calls.clone()));
        let (sessions, metadata) = sessions_with_metadata(driver, ConnectParams::default()).await;
        for expected in [
            Ok(7),
            Err(DbError::coded("WRITE", "original write error")),
            Err(crate::execution::interrupted(
                DbError::coded("08006", "write response lost"),
                true,
            )),
        ] {
            let outcome = expected.clone();
            let result = sessions
                .run_operation_rel("test", None, move |rel| {
                    async move {
                        rel.execute("INSERT INTO fixture VALUES (1)").await?;
                        outcome
                    }
                    .boxed()
                })
                .unwrap()
                .await
                .unwrap();
            assert_eq!(
                result, expected,
                "close failure must preserve the operation result"
            );
        }
        assert_eq!(calls.opened.load(Ordering::SeqCst), 4);
        assert_eq!(calls.submitted.load(Ordering::SeqCst), 3);
        assert_eq!(calls.closed.load(Ordering::SeqCst), 3);
        assert_eq!(sessions.session_count("test"), 1);

        calls.fail_connect.store(true, Ordering::SeqCst);
        let error = sessions
            .run_operation_rel::<()>("test", None, |_| {
                async { panic!("failed connection must not invoke the operation") }.boxed()
            })
            .unwrap()
            .await
            .unwrap()
            .unwrap_err();
        assert_eq!(error.code.as_deref(), Some("CONNECT"));
        assert_eq!(error.message, "connection refused");
        assert!(!error.is_outcome_unknown());
        assert_eq!(calls.submitted.load(Ordering::SeqCst), 3);
        assert_eq!(calls.closed.load(Ordering::SeqCst), 3);
        let _ = metadata.lock().await.take().unwrap().close().await;
    });
}

fn network_drivers() -> Vec<(Arc<dyn Driver>, ConnectParams)> {
    [
        (
            Arc::new(crate::mysql::MySqlDriver::mysql()) as Arc<dyn Driver>,
            13306,
            "root",
            "sakila",
        ),
        (
            Arc::new(crate::mysql::MySqlDriver::mariadb()),
            13307,
            "root",
            "sakila",
        ),
        (
            Arc::new(crate::postgres::PostgresDriver::default()),
            15432,
            "postgres",
            "pagila",
        ),
    ]
    .into_iter()
    .map(|(driver, port, user, database)| {
        (
            driver,
            ConnectParams {
                host: "127.0.0.1".into(),
                port,
                user: user.into(),
                password: Some("navidog".into()),
                database: Some(database.into()),
                ..Default::default()
            },
        )
    })
    .collect()
}

#[test]
fn completed_operation_does_not_pin_or_replay_on_metadata_recovery() {
    if std::env::var_os("NAVIDOG_LIVE").is_none() {
        return;
    }
    runtime::runtime().block_on(async {
        for (driver, params) in network_drivers() {
            let mut observer = crate::connect(driver.clone(), params.clone())
                .await
                .unwrap();
            let dialect = observer.relational().unwrap().dialect();
            let table = dialect.quote_ident(&format!("navidog_operation_{}", std::process::id()));
            observer
                .relational()
                .unwrap()
                .execute(&format!(
                    "DROP TABLE IF EXISTS {table}; CREATE TABLE {table} (n INTEGER)"
                ))
                .await
                .unwrap();
            let (sessions, metadata) = sessions_with_metadata(driver, params).await;
            let old_id = metadata
                .lock()
                .await
                .as_ref()
                .unwrap()
                .session_id()
                .unwrap();
            let insert = format!("INSERT INTO {table} VALUES (1)");
            sessions
                .run_operation_rel("test", None, move |rel| {
                    async move { rel.execute(&insert).await }.boxed()
                })
                .unwrap()
                .await
                .unwrap()
                .unwrap();
            assert_eq!(sessions.session_count("test"), 1);
            assert_eq!(
                metadata.lock().await.as_ref().unwrap().session_id(),
                Some(old_id)
            );

            let kill = match dialect {
                Dialect::MySql => format!("KILL {old_id}"),
                Dialect::Postgres => format!("SELECT pg_terminate_backend({old_id})"),
                _ => unreachable!(),
            };
            observer.relational().unwrap().execute(&kill).await.unwrap();
            let select = format!("SELECT COUNT(*) FROM {table}");
            let result = sessions
                .run_rel("test", None, move |rel| {
                    async move { rel.execute_read(&select).await }.boxed()
                })
                .unwrap()
                .await
                .unwrap()
                .unwrap();
            assert_eq!(result[0].rows[0][0].as_deref(), Some("1"));
            assert_ne!(
                metadata.lock().await.as_ref().unwrap().session_id(),
                Some(old_id)
            );
            assert_eq!(sessions.session_count("test"), 1);
            metadata.lock().await.take().unwrap().close().await.unwrap();
            observer
                .relational()
                .unwrap()
                .execute(&format!("DROP TABLE {table}"))
                .await
                .unwrap();
            observer.close().await.unwrap();
        }
    });
}
