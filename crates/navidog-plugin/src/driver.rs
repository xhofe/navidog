//! Relational requests are pure data; each JS session belongs to one plugin generation.

use std::sync::Arc;
use std::time::Duration;

use crate::protocol::Bridge;
use futures::{FutureExt as _, future::BoxFuture};
use navidog_db::{
    CancelHandle, ColumnInfo, ConnectParams, DbError, DbResult, Driver, DriverInfo, ForeignKey,
    ObjectInfo, Relational, ResultSet, Session, TableChildren,
};
use navidog_model::object::{ObjectKind, Scope};
use navidog_sql::{
    Dialect,
    declared::{DeclaredDialect, DialectDescriptor},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct DriverDescriptor {
    pub id: String,
    pub name: String,
    pub family: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub default_port: Option<u16>,
    #[serde(default)]
    pub fields: Vec<String>,
    pub dialect: DialectDescriptor,
}
impl DriverDescriptor {
    pub fn validate(&self) -> Result<(), String> {
        if self.family != "relational" || self.name.trim().is_empty() {
            return Err("API v1 drivers must declare a relational name and dialect.".into());
        }
        if self.fields.iter().any(|f| {
            !["host", "port", "user", "password", "database", "file"].contains(&f.as_str())
        }) {
            return Err("Unknown connection field.".into());
        }
        self.dialect.validate()
    }
}
pub struct JsDriver {
    info: DriverInfo,
    bridge: Bridge,
}
impl JsDriver {
    pub fn new(plugin: &str, descriptor: DriverDescriptor, bridge: Bridge) -> Result<Self, String> {
        descriptor.validate()?;
        let dialect = Arc::new(
            DeclaredDialect::new(descriptor.dialect)?.with_generation(bridge.generation()),
        );
        let mut info = DriverInfo::relational(
            format!("{plugin}/{}", descriptor.id),
            descriptor.name,
            dialect,
        );
        info.icon = descriptor.icon.unwrap_or_default();
        info.default_port = descriptor.default_port;
        info.supports_network = descriptor.fields.iter().any(|f| f == "host");
        info.file_based = descriptor.fields.iter().any(|f| f == "file");
        info.per_database_sessions = true;
        Ok(Self { info, bridge })
    }
}
impl Driver for JsDriver {
    fn info(&self) -> &DriverInfo {
        &self.info
    }
    fn connect(&self, params: ConnectParams) -> BoxFuture<'static, DbResult<Box<dyn Session>>> {
        let bridge = self.bridge.clone();
        let dialect = self.info.dialect.clone();
        let driver = self.info.id.clone();
        async move {
            let request=bridge.request("driver.request","connect",json!({"driver_id":driver,"connection":{"name":params.name,"host":params.host,"port":params.port,"user":params.user,"password":params.password,"database":params.database,"file":params.file}}),params.connect_timeout)?;
            let response=request.await?;
            let token=response.value()["session_token"].as_str().filter(|s|!s.is_empty()).ok_or_else(||DbError::msg("Driver connect must return a non-empty session_token."))?.to_owned();
            let version=response.value()["version"].as_str().ok_or_else(||DbError::msg("Driver connect must return a cache version."))?.to_owned();
            let session_id=response.value().get("session_id").filter(|v|!v.is_null()).map(|v|v.as_str().ok_or_else(||DbError::msg("Database session id must be a decimal string."))?.parse::<u64>().map_err(|_|DbError::msg("Invalid database session id."))).transpose()?;
            let mut session=JsSession {bridge,dialect,driver,token,version,session_id,timeout:params.read_timeout};
            if !params.initial_queries.trim().is_empty() {session.execute(&params.initial_queries).await?;}
            Ok(Box::new(session) as Box<dyn Session>)
        }.boxed()
    }
}
pub struct JsSession {
    bridge: Bridge,
    dialect: Arc<dyn Dialect>,
    driver: String,
    token: String,
    version: String,
    session_id: Option<u64>,
    timeout: Option<Duration>,
}
impl JsSession {
    async fn value(&self, op: &str, mut args: Value) -> DbResult<Value> {
        args["driver_id"] = self.driver.clone().into();
        args["session_token"] = self.token.clone().into();
        Ok(self
            .bridge
            .request("driver.request", op, args, self.timeout)?
            .await?
            .value()
            .clone())
    }
    async fn read<T: serde::de::DeserializeOwned>(&self, op: &str, args: Value) -> DbResult<T> {
        serde_json::from_value(self.value(op, args).await?)
            .map_err(|e| DbError::msg(format!("Invalid {op} response: {e}")))
    }
}
impl Session for JsSession {
    fn server_version(&self) -> &str {
        &self.version
    }
    fn ping(&mut self) -> BoxFuture<'_, DbResult<()>> {
        async move {
            self.value("ping", json!({})).await?;
            Ok(())
        }
        .boxed()
    }
    fn cancel_handle(&self) -> Option<CancelHandle> {
        let bridge = self.bridge.clone();
        let driver = self.driver.clone();
        let token = self.token.clone();
        Some(Arc::new(move || {
            let bridge = bridge.clone();
            let driver = driver.clone();
            let token = token.clone();
            async move {
                bridge
                    .request(
                        "driver.request",
                        "cancel",
                        json!({"driver_id":driver,"session_token":token}),
                        None,
                    )?
                    .await?;
                Ok(())
            }
            .boxed()
        }))
    }
    fn session_id(&self) -> Option<u64> {
        self.session_id
    }
    fn close(self: Box<Self>) -> BoxFuture<'static, DbResult<()>> {
        async move {
            self.value("disconnect", json!({})).await?;
            Ok(())
        }
        .boxed()
    }
    fn relational(&mut self) -> Option<&mut dyn Relational> {
        Some(self)
    }
}
impl Relational for JsSession {
    fn object_snapshot<'a>(
        &'a mut self,
        scope: &'a Scope,
        kind: ObjectKind,
        name: &'a str,
    ) -> BoxFuture<'a, DbResult<Option<navidog_model::table_design::ObjectSnapshot>>> {
        self.read(
            "object_snapshot",
            json!({"scope":scope,"kind":kind,"name":name}),
        )
        .boxed()
    }

    fn table_snapshot<'a>(
        &'a mut self,
        scope: &'a Scope,
        name: &'a str,
    ) -> BoxFuture<'a, DbResult<Option<navidog_model::table_design::StructuredTableSnapshot>>> {
        self.read("table_snapshot", json!({"scope":scope,"name":name}))
            .boxed()
    }

    fn dialect(&self) -> Arc<dyn Dialect> {
        self.dialect.clone()
    }
    fn execute<'a>(&'a mut self, sql: &'a str) -> BoxFuture<'a, DbResult<Vec<ResultSet>>> {
        async move {
            let request = self.bridge.request(
                "driver.request",
                "execute",
                json!({"driver_id":self.driver,"session_token":self.token,"sql":sql}),
                self.timeout,
            )?;
            let mut tag = navidog_db::log::Tag::new(&self.driver, &self.driver);
            tag.plugin_source = self.driver.split_once('/').map(|(plugin, _)| plugin.into());
            navidog_db::log::logged(&tag, sql, async { Ok(request.await?.into_sets()) }).await
        }
        .boxed()
    }
    fn list_databases(&mut self) -> BoxFuture<'_, DbResult<Vec<String>>> {
        self.read("list_databases", json!({})).boxed()
    }
    fn list_schemas(&mut self) -> BoxFuture<'_, DbResult<Vec<String>>> {
        self.read("list_schemas", json!({})).boxed()
    }
    fn list_objects<'a>(
        &'a mut self,
        scope: &'a Scope,
        kind: ObjectKind,
    ) -> BoxFuture<'a, DbResult<Vec<ObjectInfo>>> {
        self.read("list_objects", json!({"scope":scope,"kind":kind}))
            .boxed()
    }
    fn object_ddl<'a>(
        &'a mut self,
        scope: &'a Scope,
        kind: ObjectKind,
        name: &'a str,
    ) -> BoxFuture<'a, DbResult<String>> {
        self.read("object_ddl", json!({"scope":scope,"kind":kind,"name":name}))
            .boxed()
    }
    fn table_columns<'a>(
        &'a mut self,
        scope: &'a Scope,
        table: &'a str,
    ) -> BoxFuture<'a, DbResult<Vec<ColumnInfo>>> {
        self.read("table_columns", json!({"scope":scope,"table":table}))
            .boxed()
    }
    fn table_children<'a>(
        &'a mut self,
        scope: &'a Scope,
        table: &'a str,
    ) -> BoxFuture<'a, DbResult<TableChildren>> {
        self.read("table_children", json!({"scope":scope,"table":table}))
            .boxed()
    }
    fn foreign_keys<'a>(
        &'a mut self,
        scope: &'a Scope,
        table: &'a str,
    ) -> BoxFuture<'a, DbResult<Vec<ForeignKey>>> {
        self.read("foreign_keys", json!({"scope":scope,"table":table}))
            .boxed()
    }
    fn outline<'a>(
        &'a mut self,
        scope: &'a Scope,
        system: bool,
    ) -> BoxFuture<'a, DbResult<navidog_db::outline::Outline>> {
        self.read("outline", json!({"scope":scope,"system":system}))
            .boxed()
    }
}
