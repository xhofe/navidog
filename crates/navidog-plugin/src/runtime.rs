//! Synchronous mount scopes freeze grants on every view, then restore an empty policy.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::path::Path;
use std::rc::Rc;

use gpui_kit::{App, Entity, Window};
use gpui_shell::{HostError, HostModule, HostValue, ScriptView, ShellRuntime, policy::Policy};
use serde_json::Value;

use crate::manifest::InstalledPlugin;
use crate::protocol::{Bridge, Chunk};

thread_local! { static MOUNTING: Cell<bool> = const { Cell::new(false) }; }
struct MountGuard;
impl MountGuard {
    fn install(policy: Policy) -> anyhow::Result<Self> {
        anyhow::ensure!(
            !MOUNTING.with(|mounted| mounted.get()),
            "Plugin mount cannot re-enter."
        );
        MOUNTING.with(|mounted| mounted.set(true));
        gpui_shell::policy::set_default(policy);
        Ok(Self)
    }
}
impl Drop for MountGuard {
    fn drop(&mut self) {
        gpui_shell::policy::set_default(Policy::new());
        MOUNTING.with(|mounted| mounted.set(false));
    }
}

/// Views drop before the last VM handle. Service remains retained without entering render.
pub struct PluginRuntime {
    service: Entity<ScriptView>,
    panels: Vec<(String, Entity<ScriptView>)>,
    bridge: Bridge,
    runtime: Rc<ShellRuntime>,
}
impl PluginRuntime {
    pub fn load(
        plugin: &InstalledPlugin,
        granted: &BTreeSet<String>,
        data_dir: &Path,
        bridge: Bridge,
        module: HostModule,
        window: &mut Window,
        cx: &mut App,
    ) -> anyhow::Result<Self> {
        let components = gpui_shell::components::components()?;
        let runtime = ShellRuntime::new_isolated_with_components(components)?;
        runtime.set_editor_files(false);
        let policy = Policy::new()
            .with_application(plugin.shell().id())
            .with_capabilities(
                plugin
                    .capabilities(granted, data_dir)
                    .map_err(anyhow::Error::msg)?,
            )
            .with_data_dir(data_dir)
            .with_host_module(module)?;
        let _guard = MountGuard::install(policy)?;
        let service_path = plugin
            .directory()
            .join(plugin.shell().entry())
            .canonicalize()?;
        anyhow::ensure!(
            service_path.starts_with(plugin.directory().canonicalize()?),
            "Service entry escapes plugin directory."
        );
        let loaded = runtime.load_application(plugin.directory(), plugin.shell().entry())?;
        let service = runtime.mount_application(&loaded, window, cx)?;
        let mut panels = Vec::new();
        for descriptor in &plugin.extensions().panels {
            let path = plugin.directory().join(&descriptor.entry);
            let canonical = path.canonicalize()?;
            anyhow::ensure!(
                canonical.starts_with(plugin.directory().canonicalize()?),
                "Panel entry escapes plugin directory."
            );
            let loaded = runtime.load_application(plugin.directory(), &descriptor.entry)?;
            let view = runtime.mount_application(&loaded, window, cx)?;
            panels.push((descriptor.id.clone(), view));
        }
        Ok(Self {
            service,
            panels,
            bridge,
            runtime,
        })
    }
    pub fn is_panel_available(&self, id: &str, cx: &App) -> bool {
        self.panel(id)
            .is_some_and(|panel| panel.read(cx).build_error().is_none())
    }
    pub fn panel(&self, id: &str) -> Option<Entity<ScriptView>> {
        self.panels
            .iter()
            .find(|(key, _)| key == id)
            .map(|(_, view)| view.clone())
    }
    pub fn bridge(&self) -> &Bridge {
        &self.bridge
    }
    pub fn authority(&self, cx: &App) -> std::rc::Rc<gpui_shell::policy::Policy> {
        self.service.read(cx).policy()
    }
    pub fn refresh(&self, cx: &mut App) {
        self.service.update(cx, |view, cx| view.refresh(cx));
        for (_, panel) in &self.panels {
            panel.update(cx, |view, cx| view.refresh(cx));
        }
    }
    pub fn failure(&self, cx: &App) -> Option<String> {
        self.panels
            .iter()
            .find_map(|(_, view)| view.read(cx).build_error().map(ToOwned::to_owned))
    }
    pub fn metrics(&self) -> gpui_shell::RuntimeMetrics {
        self.runtime.read_metrics()
    }
}
impl Drop for PluginRuntime {
    fn drop(&mut self) {
        self.bridge.stop("Plugin runtime stopped.");
    }
}

pub(crate) fn host_value(value: Value) -> HostValue {
    match value {
        Value::Null => HostValue::Null,
        Value::Bool(v) => HostValue::Bool(v),
        Value::Number(v) => HostValue::Number(v.as_f64().unwrap_or_default()),
        Value::String(v) => HostValue::Str(v),
        Value::Array(v) => HostValue::Array(v.into_iter().map(host_value).collect()),
        Value::Object(v) => {
            HostValue::Object(v.into_iter().map(|(k, v)| (k, host_value(v))).collect())
        }
    }
}
pub(crate) fn json_value(value: &HostValue) -> Result<Value, HostError> {
    Ok(match value {
        HostValue::Null => Value::Null,
        HostValue::Bool(v) => Value::Bool(*v),
        HostValue::Number(v) => {
            if !v.is_finite() {
                return Err(HostError::new(
                    "Non-finite numbers cannot cross the plugin boundary.",
                ));
            }
            if v.fract() == 0. && *v >= 0. && *v <= 9_007_199_254_740_991. {
                Value::Number((*v as u64).into())
            } else if v.fract() == 0. && *v < 0. && *v >= -9_007_199_254_740_991. {
                Value::Number((*v as i64).into())
            } else {
                serde_json::Number::from_f64(*v)
                    .map(Value::Number)
                    .ok_or_else(|| HostError::new("Invalid numeric value."))?
            }
        }
        HostValue::Str(v) => Value::String(v.clone()),
        HostValue::Array(v) => Value::Array(v.iter().map(json_value).collect::<Result<_, _>>()?),
        HostValue::Object(v) => Value::Object(
            v.iter()
                .map(|(k, v)| Ok((k.clone(), json_value(v)?)))
                .collect::<Result<_, HostError>>()?,
        ),
    })
}

/// Core functions are always exported. Capability functions are added by the application bridge.
pub fn core_module(bridge: Bridge, locale: String) -> HostModule {
    let events = bridge.clone();
    let responses = bridge.clone();
    HostModule::new("navidog")
        .function("api_version", |_| Ok(1_u32.into()))
        .function("locale", move |_| {
            let _ = &locale;
            Ok(rust_i18n::locale().to_string().into())
        })
        .async_function("next_event", move |_| {
            let events = events.clone();
            Ok(async move {
                events
                    .next_event()
                    .await
                    .map(host_value)
                    .map_err(|e| HostError::new(e.to_string()))
            })
        })
        .function("respond", move |args| {
            let id = args.string(0)?;
            let generation = generation(args.integer(1)?)?;
            let reply = json_value(args.value(2)?)?;
            let result = match reply.get("ok").and_then(Value::as_bool) {
                Some(true) => Ok(reply.get("data").cloned().unwrap_or(Value::Null)),
                Some(false) => Err(navidog_db::DbError::msg(
                    reply
                        .get("error")
                        .and_then(Value::as_str)
                        .ok_or_else(|| HostError::new("Failure reply requires an error string."))?,
                )),
                None => return Err(HostError::new("Reply requires a boolean ok field.")),
            };
            responses
                .respond(id, generation, result)
                .map_err(|e| HostError::new(e.to_string()))?;
            Ok(HostValue::Null)
        })
        .function("respond_chunk", move |args| {
            let chunk: Chunk = serde_json::from_value(json_value(args.value(2)?)?)
                .map_err(|e| HostError::new(e.to_string()))?;
            bridge
                .chunk(args.string(0)?, generation(args.integer(1)?)?, chunk)
                .map_err(|e| HostError::new(e.to_string()))?;
            Ok(HostValue::Null)
        })
}
fn generation(generation: i64) -> Result<u64, HostError> {
    generation
        .try_into()
        .map_err(|_| HostError::new("Generation must be non-negative."))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn policy_scope_restores_even_on_unwind_and_rejects_reentry() {
        let result = std::panic::catch_unwind(|| {
            let _guard = MountGuard::install(Policy::new().with_application("p")).unwrap();
            assert!(MountGuard::install(Policy::new()).is_err());
            panic!("mount failure");
        });
        assert!(result.is_err());
        let _guard = MountGuard::install(Policy::new()).unwrap();
    }
    #[test]
    fn core_module_exports_match_declarations() {
        core_module(Bridge::new(1,Default::default()).unwrap(),"en".into()).declarations(
            "export function api_version(): number;\nexport function locale(): string;\nexport function next_event(): Promise<unknown>;\nexport function respond(id: string, generation: number, reply: unknown): void;\nexport function respond_chunk(id: string, generation: number, chunk: unknown): void;"
        ).validate().unwrap();
    }
}
