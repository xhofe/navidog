//! Capability functions bind to an invocation's original window and operation session.

use std::collections::BTreeSet;

use futures::FutureExt as _;

use gpui_shell::{HostError, HostModule, HostValue};
use navidog_app_state::{plugin::ContextBridge, store::ConnectionStore};
use navidog_session::sessions::Sessions;
use serde_json::{Value, json};

use crate::{
    protocol::Bridge,
    runtime::{core_module, host_value},
};

pub fn module(
    plugin: &str,
    bridge: Bridge,
    granted: &BTreeSet<String>,
    locale: &str,
) -> HostModule {
    let mut module = core_module(bridge.clone(), locale.into());
    let failure = bridge.clone();
    module = module.function("runtime_failed", move |args| {
        failure.stop(args.string(0)?);
        Ok(HostValue::Null)
    });
    let id = plugin.to_owned();
    let generation = bridge.generation();
    let active = bridge.clone();
    module = module.function("notify", move |args| {
        if !active.is_active() {
            return Err(HostError::new("Plugin generation has ended."));
        }
        let token = args.string(0)?.to_owned();
        let level = args.string(1)?.to_owned();
        let message = args.string(2)?.to_owned();
        if !["info", "success", "warning", "error"].contains(&level.as_str()) {
            return Err(HostError::new("Unknown notification level."));
        }
        gpui_shell::with_current_app(|cx| {
            let target =
                ContextBridge::window(cx, &token, &id, generation).map_err(HostError::new)?;
            let window = cx
                .windows()
                .into_iter()
                .find(|w| w.window_id() == target)
                .ok_or_else(|| HostError::new("Plugin context window has closed."))?;
            window
                .update(cx, |_, window, cx| {
                    use gpui_kit::component::{WindowExt as _, notification::Notification};
                    window.push_notification(Notification::new().message(message), cx);
                })
                .map_err(|e| HostError::new(e.to_string()))?;
            Ok(HostValue::Null)
        })
        .ok_or_else(|| HostError::new("Host application is unavailable."))?
    });
    if granted.contains("connections.read") {
        let active = bridge.clone();
        module=module.function("list_connections",move |_| {
            if !active.is_active() {return Err(HostError::new("Plugin generation has ended."));}
            gpui_shell::with_current_app(|cx| {
                let store=ConnectionStore::global(cx); let store=store.read(cx);
                Ok(host_value(Value::Array(store.connections().iter().map(|c|json!({"id":c.id,"name":c.name,"driver":c.driver,"host":c.general.host,"color":c.color})).collect())))
            }).ok_or_else(||HostError::new("Host application is unavailable."))?
        });
    }
    if granted.contains("grid.read") {
        let id = plugin.to_owned();
        let active = bridge.clone();
        module = module.function("grid_selection", move |args| {
            if !active.is_active() {
                return Err(HostError::new("Plugin generation has ended."));
            }
            let token = args.string(0)?;
            gpui_shell::with_current_app(|cx| {
                ContextBridge::grid(cx, token, &id, generation)
                    .map(host_value)
                    .map_err(HostError::new)
            })
            .ok_or_else(|| HostError::new("Host application is unavailable."))?
        });
    }
    if granted.contains("editor.write") {
        let id = plugin.to_owned();
        let active = bridge.clone();
        module = module.function("insert_text", move |args| {
            if !active.is_active() {
                return Err(HostError::new("Plugin generation has ended."));
            }
            let token = args.string(0)?.to_owned();
            let text = args.string(1)?.to_owned();
            let id = id.clone();
            gpui_shell::with_current_app(|cx| {
                let target =
                    ContextBridge::window(cx, &token, &id, generation).map_err(HostError::new)?;
                let window = cx
                    .windows()
                    .into_iter()
                    .find(|w| w.window_id() == target)
                    .ok_or_else(|| HostError::new("Plugin context window has closed."))?;
                // Host functions never drive another render or enter the VM.
                cx.defer(move |cx| {
                    let _ = window.update(cx, |_, window, cx| {
                        if let Err(error) =
                            ContextBridge::insert(cx, window, &token, &id, generation, text)
                        {
                            tracing::warn!("{error}");
                        }
                    });
                });
                Ok(HostValue::Null)
            })
            .ok_or_else(|| HostError::new("Host application is unavailable."))?
        });
    }
    if granted.contains("query.execute") {
        let plugin = plugin.to_owned();
        let active = bridge.clone();
        module = module.async_function("execute", move |args| {
            if !active.is_active() {
                return Err(HostError::new("Plugin generation has ended."));
            }
            let token = args.string(0)?;
            let connection = args.string(1)?;
            let database = if args.value(2)?.is_null() {
                None
            } else {
                Some(args.string(2)?)
            };
            let sql = args.string(3)?.to_owned();
            let source = plugin.clone();
            let operation = gpui_shell::with_current_app(|cx| {
                ContextBridge::window(cx, token, &plugin, generation).map_err(HostError::new)?;
                let cancellation = active.cancellation();
                let timeout = std::time::Duration::from_secs_f64(active.limits().timeout_seconds);
                Sessions::global(cx)
                    .read(cx)
                    .run_plugin_operation_rel(connection, database, cancellation, move |rel| {
                        async move {
                            let result = tokio::time::timeout(
                                timeout,
                                navidog_db::log::with_plugin_source(source, rel.execute(&sql)),
                            )
                            .await;
                            match result {
                                Ok(result) => result,
                                Err(_) => {
                                    Err(navidog_db::DbError::msg("Plugin SQL operation timed out.")
                                        .with_session_loss(true))
                                }
                            }
                        }
                        .boxed()
                    })
                    .ok_or_else(|| {
                        HostError::new("SQL execution requires an already opened connection.")
                    })
            })
            .ok_or_else(|| HostError::new("Host application is unavailable."))??;
            let active = active.clone();
            Ok(async move {
                let result = operation
                    .await
                    .map_err(|e| HostError::new(e.to_string()))?
                    .map_err(|e| HostError::new(e.to_string()))?;
                if !active.is_active() {
                    return Err(HostError::new("Plugin generation has ended."));
                }
                let rows = result.iter().map(|set| set.rows.len()).sum::<usize>();
                let value = Value::Array(result.into_iter().map(result_json).collect());
                if rows > active.limits().result_rows
                    || serde_json::to_vec(&value)
                        .map_err(|error| HostError::new(error.to_string()))?
                        .len()
                        > active.limits().result_bytes
                {
                    return Err(HostError::new("SQL result exceeds plugin limits."));
                }
                Ok(host_value(value))
            })
        });
    }
    for (name, import) in [("import_rows", true), ("export_rows", false)] {
        let bridge = bridge.clone();
        let plugin = plugin.to_string();
        module = module.async_function(name, move |args| {
            let context = args.string(0)?.to_owned();
            let format = args.string(1)?.to_owned();
            let input = crate::runtime::json_value(args.value(2)?)?;
            gpui_shell::with_current_app(|cx| {
                ContextBridge::window(cx, &context, &plugin, generation).map_err(HostError::new)?;
                let key = format!("{plugin}/{format}");
                let formats = &cx
                    .global::<navidog_app_state::plugin::Contributions>()
                    .formats;
                let descriptor = formats
                    .get(&key)
                    .ok_or_else(|| HostError::new("Format is unavailable."))?;
                if descriptor[if import { "import" } else { "export" }].as_bool() != Some(true) {
                    return Err(HostError::new("Format does not support this operation."));
                }
                Ok(())
            })
            .ok_or_else(|| HostError::new("Host application is unavailable."))??;
            let bridge = bridge.clone();
            Ok(async move {
                if import {
                    let input = input
                        .as_str()
                        .ok_or_else(|| HostError::new("Import input must be text."))?
                        .to_owned();
                    let sets = crate::format::import_rows(&bridge, &format, &context, input)
                        .await
                        .map_err(|error| HostError::new(error.to_string()))?;
                    Ok(host_value(Value::Array(
                        sets.into_iter().map(result_json).collect(),
                    )))
                } else {
                    crate::format::export_rows(&bridge, &format, &context, input)
                        .await
                        .map(HostValue::Str)
                        .map_err(|error| HostError::new(error.to_string()))
                }
            })
        });
    }
    let mut declarations = String::from(
        "export function runtime_failed(reason:string):void;\nexport function api_version(): number;\nexport function locale(): string;\nexport function next_event(): Promise<unknown>;\nexport function respond(id:string,generation:number,reply:unknown):void;\nexport function respond_chunk(id:string,generation:number,chunk:unknown):void;\nexport function notify(context:string,level:string,message:string):void;\nexport function import_rows(context:string,format:string,input:string):Promise<unknown>;\nexport function export_rows(context:string,format:string,result:unknown):Promise<string>;\n",
    );
    for (permission, declaration) in [
        (
            "connections.read",
            "export function list_connections(): unknown[];\n",
        ),
        (
            "grid.read",
            "export function grid_selection(context:string):unknown;\n",
        ),
        (
            "editor.write",
            "export function insert_text(context:string,text:string):void;\n",
        ),
        (
            "query.execute",
            "export function execute(context:string,connection:string,database:string|null,sql:string):Promise<unknown[]>;\n",
        ),
    ] {
        if granted.contains(permission) {
            declarations.push_str(declaration);
        }
    }
    module.declarations(declarations)
}

fn result_json(set: navidog_db::ResultSet) -> Value {
    json!({"columns":set.columns.into_iter().map(|column|json!({"name":column.name,"type_name":column.type_name,"origin":column.origin.map(|origin|json!({"schema":origin.schema,"table":origin.table,"column":origin.column}))})).collect::<Vec<_>>(),"rows":set.rows,"notices":set.notices,"affected_rows":set.affected_rows.map(|rows|rows.to_string())})
}
