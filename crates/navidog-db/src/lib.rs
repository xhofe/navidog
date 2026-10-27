//! Drivers: the traits, the tokio runtime and the SSH tunnel. Protocol implementations live in
//! `navidog-mysql`, `navidog-postgres` and `navidog-sqlite`.

rust_i18n::i18n!("locales", fallback = "en");
// Track locale edits in Cargo as well as the translation proc-macro.
const _: &str = include_str!("../locales/database.yml");

pub mod commit;
pub mod driver;
pub mod execution;
pub mod log;
pub mod metadata;
pub mod outline;
pub mod params;
pub mod runtime;
pub mod sql;
pub mod ssh;

use std::sync::Arc;

use futures::FutureExt as _;
pub use futures::future::BoxFuture;

pub use driver::*;
pub use metadata::referenced_values_sql;
pub use params::{ConnectParams, Secrets};

// Named by the `relational_catalog!` expansion, which resolves paths in the expanding crate.
#[doc(hidden)]
pub use navidog_model;

/// Forwards the eight catalog methods of [`Relational`] to one database crate's `metadata`
/// module, so each driver writes one line instead of fifty. The module must expose
/// `list_databases`, `list_schemas`, `list_objects`, `object_ddl`, `table_columns`,
/// `table_children`, `foreign_keys` and `outline` with the trait's signatures.
/// The eight catalog methods of [`Relational`], every one reporting that this session has no
/// catalog. For test doubles: a real driver uses [`relational_catalog!`].
#[macro_export]
macro_rules! unsupported_catalog {
    () => {
        fn list_databases(&mut self) -> $crate::BoxFuture<'_, $crate::DbResult<Vec<String>>> {
            Box::pin(async { Err($crate::DbError::msg("This session does not list catalogs.")) })
        }
        fn list_schemas(&mut self) -> $crate::BoxFuture<'_, $crate::DbResult<Vec<String>>> {
            Box::pin(async { Ok(Vec::new()) })
        }
        fn list_objects<'a>(
            &'a mut self,
            _scope: &'a $crate::navidog_model::object::Scope,
            _kind: $crate::navidog_model::object::ObjectKind,
        ) -> $crate::BoxFuture<'a, $crate::DbResult<Vec<$crate::ObjectInfo>>> {
            Box::pin(async { Err($crate::DbError::msg("This session does not list objects.")) })
        }
        fn object_ddl<'a>(
            &'a mut self,
            _scope: &'a $crate::navidog_model::object::Scope,
            _kind: $crate::navidog_model::object::ObjectKind,
            _name: &'a str,
        ) -> $crate::BoxFuture<'a, $crate::DbResult<String>> {
            Box::pin(async {
                Err($crate::DbError::msg(
                    "This session does not read object definitions.",
                ))
            })
        }
        fn table_columns<'a>(
            &'a mut self,
            _scope: &'a $crate::navidog_model::object::Scope,
            _table: &'a str,
        ) -> $crate::BoxFuture<'a, $crate::DbResult<Vec<$crate::ColumnInfo>>> {
            Box::pin(async { Err($crate::DbError::msg("This session does not read columns.")) })
        }
        fn table_children<'a>(
            &'a mut self,
            _scope: &'a $crate::navidog_model::object::Scope,
            _table: &'a str,
        ) -> $crate::BoxFuture<'a, $crate::DbResult<$crate::TableChildren>> {
            Box::pin(async {
                Err($crate::DbError::msg(
                    "This session does not read table children.",
                ))
            })
        }
        fn foreign_keys<'a>(
            &'a mut self,
            _scope: &'a $crate::navidog_model::object::Scope,
            _table: &'a str,
        ) -> $crate::BoxFuture<'a, $crate::DbResult<Vec<$crate::ForeignKey>>> {
            Box::pin(async {
                Err($crate::DbError::msg(
                    "This session does not read foreign keys.",
                ))
            })
        }
        fn outline<'a>(
            &'a mut self,
            _scope: &'a $crate::navidog_model::object::Scope,
            _system: bool,
        ) -> $crate::BoxFuture<'a, $crate::DbResult<$crate::outline::Outline>> {
            Box::pin(async {
                Err($crate::DbError::msg(
                    "This session has no completion outline.",
                ))
            })
        }
    };
}

#[macro_export]
macro_rules! relational_catalog {
    ($metadata:ident) => {
        fn list_databases(&mut self) -> $crate::BoxFuture<'_, $crate::DbResult<Vec<String>>> {
            Box::pin($metadata::list_databases(self))
        }
        fn list_schemas(&mut self) -> $crate::BoxFuture<'_, $crate::DbResult<Vec<String>>> {
            Box::pin($metadata::list_schemas(self))
        }
        fn list_objects<'a>(
            &'a mut self,
            scope: &'a $crate::navidog_model::object::Scope,
            kind: $crate::navidog_model::object::ObjectKind,
        ) -> $crate::BoxFuture<'a, $crate::DbResult<Vec<$crate::ObjectInfo>>> {
            Box::pin($metadata::list_objects(self, scope, kind))
        }
        fn object_ddl<'a>(
            &'a mut self,
            scope: &'a $crate::navidog_model::object::Scope,
            kind: $crate::navidog_model::object::ObjectKind,
            name: &'a str,
        ) -> $crate::BoxFuture<'a, $crate::DbResult<String>> {
            Box::pin($metadata::object_ddl(self, scope, kind, name))
        }
        fn table_columns<'a>(
            &'a mut self,
            scope: &'a $crate::navidog_model::object::Scope,
            table: &'a str,
        ) -> $crate::BoxFuture<'a, $crate::DbResult<Vec<$crate::ColumnInfo>>> {
            Box::pin($metadata::table_columns(self, scope, table))
        }
        fn table_children<'a>(
            &'a mut self,
            scope: &'a $crate::navidog_model::object::Scope,
            table: &'a str,
        ) -> $crate::BoxFuture<'a, $crate::DbResult<$crate::TableChildren>> {
            Box::pin($metadata::table_children(self, scope, table))
        }
        fn foreign_keys<'a>(
            &'a mut self,
            scope: &'a $crate::navidog_model::object::Scope,
            table: &'a str,
        ) -> $crate::BoxFuture<'a, $crate::DbResult<Vec<$crate::ForeignKey>>> {
            Box::pin($metadata::foreign_keys(self, scope, table))
        }
        fn outline<'a>(
            &'a mut self,
            scope: &'a $crate::navidog_model::object::Scope,
            system: bool,
        ) -> $crate::BoxFuture<'a, $crate::DbResult<$crate::outline::Outline>> {
            Box::pin($metadata::outline(self, scope, system))
        }
    };
}

/// Serializes tests that mutate the process-global `NAVIDOG_HOME`; shared with
/// the session-crate tests that also write config directories.
pub fn lock_config_home() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

/// Open a session. With SSH, open the tunnel first and connect the driver to its local port.
/// Must run inside [`runtime::spawn`].
pub fn connect(
    driver: Arc<dyn Driver>,
    mut params: ConnectParams,
) -> BoxFuture<'static, DbResult<Box<dyn Session>>> {
    async move {
        let Some(ssh) = params.ssh.take() else {
            return driver.connect(params).await;
        };
        let tunnel = tokio::time::timeout(
            params.timeout(),
            ssh::open(&ssh, params.host.clone(), params.port),
        )
        .await
        .map_err(|_| DbError::msg("SSH: connection timed out"))??;
        params.tls_host = Some(std::mem::replace(&mut params.host, "127.0.0.1".into()));
        params.port = tunnel.local_port;
        let inner = driver.connect(params).await?;
        Ok(Box::new(Tunneled {
            inner,
            _tunnel: tunnel,
        }) as Box<dyn Session>)
    }
    .boxed()
}

/// A session that owns its tunnel. Dropping the session drops the tunnel.
struct Tunneled {
    inner: Box<dyn Session>,
    _tunnel: ssh::Tunnel,
}

impl Session for Tunneled {
    fn server_version(&self) -> &str {
        self.inner.server_version()
    }
    fn ping(&mut self) -> BoxFuture<'_, DbResult<()>> {
        self.inner.ping()
    }
    fn cancel_handle(&self) -> Option<CancelHandle> {
        self.inner.cancel_handle()
    }
    fn session_id(&self) -> Option<u64> {
        self.inner.session_id()
    }
    fn close(self: Box<Self>) -> BoxFuture<'static, DbResult<()>> {
        let Tunneled { inner, _tunnel } = *self;
        async move {
            let res = inner.close().await;
            drop(_tunnel);
            res
        }
        .boxed()
    }
    fn relational(&mut self) -> Option<&mut dyn Relational> {
        self.inner.relational()
    }
    fn document(&mut self) -> Option<&mut dyn Document> {
        self.inner.document()
    }
    fn key_value(&mut self) -> Option<&mut dyn KeyValue> {
        self.inner.key_value()
    }
}

/// Test a connection: open a session, ping once, and close it.
pub async fn test_connection(driver: Arc<dyn Driver>, params: ConnectParams) -> DbResult<String> {
    let mut session = connect(driver, params).await?;
    session.ping().await?;
    let version = session.server_version().to_string();
    session.close().await?;
    Ok(version)
}
