//! Local HTTP fixture; never deployed with the application.
use navidog_db::{ConnectParams, Driver, Session};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    sync::Mutex,
};
type Sessions = Arc<Mutex<HashMap<String, Arc<Mutex<Box<dyn Session>>>>>>;
type Cancels = Arc<Mutex<HashMap<String, navidog_db::CancelHandle>>>;
async fn request(event: Value, sessions: Sessions, cancels: Cancels) -> Result<Value, String> {
    let args = &event["arguments"];
    let op = event["op"].as_str().unwrap_or("cancel");
    if std::env::var_os("NAVIDOG_HTTP_TRACE").is_some() {
        eprintln!("HTTP {op}");
    }
    if op == "connect" {
        let c = &args["connection"];
        let mysql = args["driver_id"].as_str().unwrap_or("").contains("mysql");
        let driver: Arc<dyn Driver> = if mysql {
            Arc::new(navidog_mysql::MySqlDriver::mysql())
        } else {
            Arc::new(navidog_postgres::PostgresDriver::default())
        };
        let params = ConnectParams {
            host: c["host"].as_str().unwrap_or("127.0.0.1").into(),
            port: c["port"]
                .as_u64()
                .unwrap_or(if mysql { 13306 } else { 15432 }) as u16,
            user: c["user"]
                .as_str()
                .unwrap_or(if mysql { "root" } else { "postgres" })
                .into(),
            password: c["password"].as_str().map(Into::into),
            database: c["database"].as_str().map(Into::into),
            ..Default::default()
        };
        let session = driver.connect(params).await.map_err(|e| e.to_string())?;
        let token = uuid::Uuid::new_v4().to_string();
        let result = json!({"session_token":token,"version":session.server_version(),"session_id":session.session_id().map(|id|id.to_string())});
        if let Some(cancel) = session.cancel_handle() {
            cancels.lock().await.insert(token.clone(), cancel);
        }
        sessions
            .lock()
            .await
            .insert(token, Arc::new(Mutex::new(session)));
        return Ok(json!({"data":result}));
    }
    let token = args["session_token"]
        .as_str()
        .ok_or("Missing session token.")?;
    if op == "cancel" {
        if let Some(cancel) = cancels.lock().await.get(token).cloned() {
            cancel().await.map_err(|e| e.to_string())?;
        }
        return Ok(json!({"data":null}));
    }
    if op == "disconnect" {
        let session = sessions
            .lock()
            .await
            .remove(token)
            .ok_or("Unknown session.")?;
        cancels.lock().await.remove(token);
        let session = Arc::try_unwrap(session)
            .map_err(|_| "Session is busy.")?
            .into_inner();
        session.close().await.map_err(|e| e.to_string())?;
        return Ok(json!({"data":null}));
    }
    let session = sessions
        .lock()
        .await
        .get(token)
        .cloned()
        .ok_or("Unknown session.")?;
    let mut session = session.lock().await;
    if op == "ping" {
        session.ping().await.map_err(|e| e.to_string())?;
        return Ok(json!({"data":null}));
    }
    let rel = session.relational().ok_or("Not relational.")?;
    let scope = serde_json::from_value(args["scope"].clone()).unwrap_or_else(|_| {
        navidog_model::object::Scope::new("fixture", "pagila", Some("public".into()))
    });
    let name = args["name"]
        .as_str()
        .or_else(|| args["table"].as_str())
        .unwrap_or("");
    let data = match op {
        "execute" => {
            let sets = rel
                .execute(args["sql"].as_str().ok_or("Missing SQL.")?)
                .await
                .map_err(|e| e.to_string())?;
            return Ok(
                json!({"results":sets.into_iter().map(|s|json!({"columns":s.columns.into_iter().map(|c|json!({"name":c.name,"type_name":c.type_name,"origin":c.origin.map(|o|json!({"schema":o.schema,"table":o.table,"column":o.column}))})).collect::<Vec<_>>(),"rows":s.rows,"notices":s.notices,"affected_rows":s.affected_rows.map(|n|n.to_string())})).collect::<Vec<_>>()}),
            );
        }
        "list_databases" => json!(rel.list_databases().await.map_err(|e| e.to_string())?),
        "list_schemas" => json!(rel.list_schemas().await.map_err(|e| e.to_string())?),
        "list_objects" => json!(
            rel.list_objects(
                &scope,
                serde_json::from_value(args["kind"].clone()).map_err(|e| e.to_string())?
            )
            .await
            .map_err(|e| e.to_string())?
        ),
        "object_snapshot" => {
            let kind = serde_json::from_value(args["kind"].clone()).map_err(|e| e.to_string())?;
            let ddl = rel
                .object_ddl(&scope, kind, name)
                .await
                .map_err(|e| e.to_string())?;
            let body = rel
                .execute(&format!(
                    "SELECT definition FROM pg_views WHERE schemaname={} AND viewname={}",
                    rel.dialect()
                        .string_literal(scope.schema.as_deref().unwrap_or("public")),
                    rel.dialect().string_literal(name)
                ))
                .await
                .map_err(|e| e.to_string())?
                .first()
                .and_then(|set| set.rows.first())
                .and_then(|row| row.first())
                .and_then(|cell| cell.clone())
                .unwrap_or_default();
            json!({"name":name,"body":body.trim_end_matches(';'),"fields":{},"ddl":ddl,"revision":ddl,"unknown":[]})
        }
        "object_ddl" => json!(
            rel.object_ddl(
                &scope,
                serde_json::from_value(args["kind"].clone()).map_err(|e| e.to_string())?,
                name
            )
            .await
            .map_err(|e| e.to_string())?
        ),
        "table_columns" => json!(
            rel.table_columns(&scope, name)
                .await
                .map_err(|e| e.to_string())?
        ),
        "table_children" => json!(
            rel.table_children(&scope, name)
                .await
                .map_err(|e| e.to_string())?
        ),
        "foreign_keys" => json!(
            rel.foreign_keys(&scope, name)
                .await
                .map_err(|e| e.to_string())?
        ),
        "outline" => json!(
            rel.outline(&scope, args["system"].as_bool().unwrap_or(false))
                .await
                .map_err(|e| e.to_string())?
        ),
        "table_snapshot" => {
            let snapshot = navidog_schema::table_design::load(rel, &scope, name)
                .await
                .map_err(|e| e.to_string())?;
            json!({"revision":snapshot.ddl,"snapshot":snapshot,"unknown":[],"attachments":[]})
        }
        _ => return Err(format!("Unsupported operation {op}")),
    };
    Ok(json!({"data":data}))
}
pub async fn serve(listener: tokio::net::TcpListener) {
    let sessions: Sessions = Default::default();
    let cancels: Cancels = Default::default();
    while let Ok((mut socket, _)) = listener.accept().await {
        let sessions = sessions.clone();
        let cancels = cancels.clone();
        tokio::spawn(async move {
            let result = async {
                let mut bytes = Vec::new();
                let mut buffer = [0; 4096];
                let (end, length) = loop {
                    let n = socket.read(&mut buffer).await.map_err(|e| e.to_string())?;
                    if n == 0 {
                        return Err("Incomplete request.".into());
                    }
                    bytes.extend_from_slice(&buffer[..n]);
                    if bytes.len() > 2_097_152 {
                        return Err("Request too large.".into());
                    }
                    if let Some(end) = bytes.windows(4).position(|s| s == b"\r\n\r\n") {
                        let headers =
                            std::str::from_utf8(&bytes[..end]).map_err(|e| e.to_string())?;
                        let length = headers
                            .lines()
                            .find_map(|l| {
                                l.split_once(':')
                                    .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                                    .and_then(|(_, v)| v.trim().parse::<usize>().ok())
                            })
                            .ok_or("Missing content length.")?;
                        break (end + 4, length);
                    }
                };
                if length > 2_097_152 {
                    return Err("Request too large.".into());
                }
                while bytes.len() < end + length {
                    let n = socket.read(&mut buffer).await.map_err(|e| e.to_string())?;
                    if n == 0 {
                        return Err("Incomplete body.".into());
                    }
                    bytes.extend_from_slice(&buffer[..n]);
                }
                request(
                    serde_json::from_slice(&bytes[end..end + length]).map_err(|e| e.to_string())?,
                    sessions,
                    cancels,
                )
                .await
            }
            .await;
            let body = match result {
                Ok(mut value) => {
                    value["ok"] = true.into();
                    value.to_string()
                }
                Err(error) => json!({"ok":false,"error":error}).to_string(),
            };
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(reply.as_bytes()).await;
        });
    }
}
#[cfg(not(test))]
fn main() {
    navidog_db::runtime::runtime().block_on(async {
        println!("HTTP fixture: 127.0.0.1:18123");
        serve(
            tokio::net::TcpListener::bind("127.0.0.1:18123")
                .await
                .unwrap(),
        )
        .await;
    });
}
