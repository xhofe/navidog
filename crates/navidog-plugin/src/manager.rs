//! Plugin lifecycle owns approval persistence, runtime generations and contribution cleanup.

use std::path::PathBuf;

use gpui_kit::{App, AppContext as _, Context, Entity, Global, Window};
use navidog_app_state::{
    plugin::{CommandContribution, ContextBridge, Contributions},
    registry::Source,
};

use crate::{
    manifest::{InstalledPlugin, discover},
    protocol::Bridge,
    runtime::PluginRuntime,
    settings::{Approval, PluginsFile},
};

pub struct Installation {
    directory: PathBuf,
    plugin: Result<InstalledPlugin, String>,
    failure: Option<String>,
    runtime: Option<PluginRuntime>,
    generation: u64,
    source_revision: String,
}
impl Installation {
    pub fn id(&self) -> &str {
        self.plugin
            .as_ref()
            .map(|p| p.shell().id())
            .unwrap_or_else(|_| self.directory.to_str().unwrap_or("invalid"))
    }
    pub fn plugin(&self) -> Result<&InstalledPlugin, &str> {
        self.plugin.as_ref().map_err(String::as_str)
    }
    pub fn failure(&self) -> Option<&str> {
        self.failure
            .as_deref()
            .or_else(|| self.plugin.as_ref().err().map(String::as_str))
    }
    pub fn is_enabled(&self) -> bool {
        self.runtime
            .as_ref()
            .is_some_and(|r| r.bridge().is_active())
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn directory(&self) -> &std::path::Path {
        &self.directory
    }
}
pub struct Plugins {
    root: PathBuf,
    file: PluginsFile,
    installations: Vec<Installation>,
    failure: Option<String>,
    watch: Option<gpui_kit::Task<()>>,
    next_generation: u64,
}
struct GlobalPlugins(Entity<Plugins>);
impl Global for GlobalPlugins {}
impl Plugins {
    pub fn global(cx: &App) -> Entity<Self> {
        cx.global::<GlobalPlugins>().0.clone()
    }
    pub fn new(root: PathBuf) -> Self {
        let loaded = PluginsFile::load(&root.parent().unwrap_or(&root).join("plugins.json"));
        let failure = loaded.as_ref().err().cloned();
        Self {
            root,
            file: loaded.unwrap_or_default(),
            installations: Vec::new(),
            failure,
            watch: None,
            next_generation: 0,
        }
    }
    pub fn installations(&self) -> &[Installation] {
        &self.installations
    }
    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }
    pub fn root(&self) -> &std::path::Path {
        &self.root
    }
    pub fn approval(&self, id: &str) -> Approval {
        self.file.approval(id).cloned().unwrap_or_default()
    }
    pub fn scan(&mut self, cx: &mut Context<Self>) {
        // Rescanning uses the same stop boundary; a new manifest never replaces live code in place.
        let ids: Vec<_> = self
            .installations
            .iter()
            .map(|p| p.id().to_string())
            .collect();
        for id in ids {
            self.stop(&id, "Plugin installation rescanned.", cx);
        }
        self.installations = discover(&self.root)
            .into_iter()
            .map(|(directory, plugin)| Installation {
                directory: directory.clone(),
                plugin,
                failure: None,
                runtime: None,
                generation: 0,
                source_revision: source_revision(&directory),
            })
            .collect();
        for installation in &mut self.installations {
            if let Ok(plugin) = &installation.plugin {
                let mut approval = self
                    .file
                    .approval(plugin.shell().id())
                    .cloned()
                    .unwrap_or_default();
                if approval.reconcile(plugin.requested(), plugin.fingerprint().into()) {
                    installation.failure = Some("New capabilities require approval.".into());
                }
                let path = self
                    .root
                    .parent()
                    .unwrap_or(&self.root)
                    .join("plugins.json");
                if let Err(error) =
                    self.file
                        .save_approval(&path, plugin.shell().id().into(), approval)
                {
                    self.failure = Some(error);
                }
            }
        }
        cx.notify();
    }
    fn stop(&mut self, id: &str, reason: &str, cx: &mut Context<Self>) {
        if let Some(installation) = self.installations.iter_mut().find(|p| p.id() == id) {
            self.next_generation = self
                .next_generation
                .checked_add(1)
                .filter(|g| *g <= 9_007_199_254_740_991)
                .expect("plugin generation exhausted");
            installation.generation = self.next_generation;
            if let Some(runtime) = installation.runtime.take() {
                runtime.bridge().stop(reason);
                cx.defer(move |_| drop(runtime));
            }
        }
        let driver_ids: Vec<_> = cx
            .global::<navidog_session::DriverRegistry>()
            .0
            .iter()
            .filter(|(_, source, _)| *source == &Source::Plugin(id.to_string().into()))
            .map(|(id, _, _)| id.to_string())
            .collect();
        let connection_ids: Vec<_> = navidog_app_state::store::ConnectionStore::global(cx)
            .read(cx)
            .connections()
            .iter()
            .filter(|c| driver_ids.contains(&c.driver))
            .map(|c| c.id.clone())
            .collect();
        for connection in connection_ids {
            navidog_session::sessions::Sessions::global(cx).update(cx, |sessions, cx| {
                sessions.close_for_reopen(&connection, cx)
            });
        }
        let source = Source::Plugin(id.to_string().into());
        cx.global_mut::<Contributions>().remove(&source);
        cx.global_mut::<navidog_app_state::registry::CommandRegistry>()
            .unregister_source(&source);
        cx.global_mut::<navidog_session::DriverRegistry>()
            .0
            .unregister_source(&source);
        ContextBridge::invalidate(cx, id);
        cx.refresh_windows();
        cx.notify();
    }
    pub fn disable(&mut self, id: &str, cx: &mut Context<Self>) -> Result<(), String> {
        let mut approval = self.approval(id);
        approval.enabled = false;
        self.file.save_approval(
            &self
                .root
                .parent()
                .unwrap_or(&self.root)
                .join("plugins.json"),
            id.into(),
            approval,
        )?;
        self.stop(id, "Plugin disabled.", cx);
        Ok(())
    }
    pub fn enable(
        &mut self,
        id: &str,
        approval: Approval,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let ix = self
            .installations
            .iter()
            .position(|p| p.id() == id)
            .ok_or("Plugin is no longer installed.")?;
        let plugin = InstalledPlugin::read(&self.installations[ix].directory)?;
        if approval.fingerprint != plugin.fingerprint()
            || approval.requested != plugin.requested()
            || !approval.granted.is_subset(&plugin.requested())
        {
            return Err("Plugin capabilities changed; review permissions again.".into());
        }
        self.file.save_approval(
            &self
                .root
                .parent()
                .unwrap_or(&self.root)
                .join("plugins.json"),
            id.into(),
            approval.clone(),
        )?;
        self.stop(id, "Plugin reloaded.", cx);
        self.installations[ix].plugin = Ok(plugin.clone());
        let bridge = Bridge::new(self.installations[ix].generation, approval.limits)
            .map_err(|e| e.to_string())?;
        let module =
            crate::host::module(id, bridge.clone(), &approval.granted, &rust_i18n::locale());
        let data_dir = self
            .root
            .parent()
            .unwrap_or(&self.root)
            .join("plugin-data")
            .join(id);
        let result = PluginRuntime::load(
            &plugin,
            &approval.granted,
            &data_dir,
            bridge.clone(),
            module,
            window,
            cx,
        )
        .and_then(|runtime| {
            self.register(&plugin, bridge.clone(), cx)
                .map_err(anyhow::Error::msg)?;
            Ok(runtime)
        });
        match result {
            Ok(runtime) => {
                self.installations[ix].runtime = Some(runtime);
                self.installations[ix].failure = None;
                self.installations[ix].source_revision =
                    source_revision(&self.installations[ix].directory);
            }
            Err(error) => {
                bridge.stop("Plugin load failed.");
                self.stop(id, "Plugin load failed.", cx);
                self.installations[ix].failure = Some(error.to_string());
                cx.notify();
                return Err(error.to_string());
            }
        }
        cx.refresh_windows();
        cx.notify();
        Ok(())
    }
    pub fn reload(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let mut approval = self.approval(id);
        let directory = self
            .installations
            .iter()
            .find(|installation| installation.id() == id)
            .ok_or("Plugin is not installed.")?
            .directory
            .clone();
        let plugin = match InstalledPlugin::read(&directory) {
            Ok(plugin) => plugin,
            Err(error) => {
                self.stop(id, &error, cx);
                return Err(error);
            }
        };
        if approval.reconcile(plugin.requested(), plugin.fingerprint().into()) {
            self.file.save_approval(
                &self
                    .root
                    .parent()
                    .unwrap_or(&self.root)
                    .join("plugins.json"),
                id.into(),
                approval,
            )?;
            self.stop(id, "New capabilities require approval.", cx);
            return Err("New capabilities require approval.".into());
        }
        if !approval.enabled {
            return Err("Plugin needs approval before it can be reloaded.".into());
        }
        self.enable(id, approval, window, cx)
    }
    fn register(
        &self,
        plugin: &InstalledPlugin,
        bridge: Bridge,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let source = Source::Plugin(plugin.shell().id().to_string().into());
        for command in &plugin.extensions().commands {
            if let Some(shortcut) = &command.shortcut {
                let key = gpui_kit::KeyBinding::load(
                    shortcut,
                    Box::new(navidog_app_state::plugin::PluginCommand {
                        id: format!("{}/{}", plugin.shell().id(), command.id).into(),
                    }),
                    None,
                    false,
                    None,
                    cx,
                )
                .map_err(|e| e.to_string())?;
                cx.bind_keys([key]);
            }
        }
        for command in &plugin.extensions().commands {
            let id: gpui_kit::SharedString =
                format!("{}/{}", plugin.shell().id(), command.id).into();
            let action_id = id.clone();
            cx.global_mut::<navidog_app_state::registry::CommandRegistry>()
                .register(
                    id,
                    source.clone(),
                    navidog_app_state::registry::CommandInfo {
                        title: command.title.clone().into(),
                        icon: command.icon.clone().map(Into::into),
                        shortcut: command.shortcut.clone().map(Into::into),
                        action: std::rc::Rc::new(move || {
                            Box::new(navidog_app_state::plugin::PluginCommand {
                                id: action_id.clone(),
                            })
                        }),
                        locations: vec![command.menu.clone().into()],
                        enabled: None,
                    },
                )
                .map_err(|e| e.to_string())?;
        }
        let contributions = cx.global_mut::<Contributions>();
        for command in &plugin.extensions().commands {
            contributions
                .commands
                .register(
                    format!("{}/{}", plugin.shell().id(), command.id),
                    source.clone(),
                    {
                        let mut contribution = CommandContribution::default();
                        contribution.title = command.title.clone().into();
                        contribution.title_zh = command.title_zh.clone().map(Into::into);
                        contribution.icon = command.icon.as_ref().map(|icon| {
                            plugin
                                .directory()
                                .join(icon)
                                .to_string_lossy()
                                .to_string()
                                .into()
                        });
                        contribution.shortcut = command.shortcut.clone().map(Into::into);
                        contribution.location = command.menu.clone().into();
                        contribution.when = command.when.clone().map(Into::into);
                        contribution
                    },
                )
                .map_err(|e| e.to_string())?;
        }
        for format in &plugin.extensions().formats {
            contributions
                .formats
                .register(
                    format!("{}/{}", plugin.shell().id(), format.id),
                    source.clone(),
                    serde_json::to_value(format).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
        }
        for formatter in &plugin.extensions().cell_formatters {
            contributions
                .formatters
                .register(
                    format!(
                        "{}/{}",
                        plugin.shell().id(),
                        formatter["id"].as_str().unwrap()
                    ),
                    source.clone(),
                    formatter.clone(),
                )
                .map_err(|e| e.to_string())?;
        }
        for panel in &plugin.extensions().panels {
            contributions
                .panels
                .register(
                    format!("{}/{}", plugin.shell().id(), panel.id),
                    source.clone(),
                    serde_json::to_value(panel).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
        }
        for descriptor in &plugin.extensions().drivers {
            let descriptor: crate::driver::DriverDescriptor =
                serde_json::from_value(descriptor.clone()).map_err(|e| e.to_string())?;
            let mut descriptor = descriptor;
            descriptor.icon = descriptor
                .icon
                .map(|icon| plugin.directory().join(icon).to_string_lossy().to_string());
            let driver =
                crate::driver::JsDriver::new(plugin.shell().id(), descriptor, bridge.clone())?;
            use navidog_db::Driver as _;
            let id = driver.info().id.clone();
            cx.global_mut::<navidog_session::DriverRegistry>()
                .0
                .register(id, source.clone(), std::sync::Arc::new(driver))
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub fn command(
        &self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let (plugin, command) = id.split_once('/').ok_or("Invalid plugin command id.")?;
        let installation = self
            .installations
            .iter()
            .find(|p| p.id() == plugin)
            .ok_or("Plugin is not installed.")?;
        let descriptor = cx
            .global::<Contributions>()
            .commands
            .get(id)
            .ok_or("Command is unavailable.")?;
        if !descriptor.is_enabled(window.window_handle().window_id(), cx) {
            return Err("Command is unavailable in this context.".into());
        }
        let runtime = installation.runtime.as_ref().ok_or("Plugin is disabled.")?;
        let token = ContextBridge::capture(
            cx,
            window.window_handle().window_id(),
            plugin,
            installation.generation,
        );
        runtime
            .bridge()
            .event(serde_json::json!({"kind":"command","command_id":command,"context":token}))
            .map_err(|e| e.to_string())
    }
    pub fn panel(&self, id: &str) -> Option<Entity<gpui_shell::ScriptView>> {
        let (plugin, panel) = id.split_once('/')?;
        self.installations
            .iter()
            .find(|p| p.id() == plugin)?
            .runtime
            .as_ref()?
            .panel(panel)
    }
}
impl Drop for Plugins {
    fn drop(&mut self) {
        for installation in &mut self.installations {
            if let Some(runtime) = installation.runtime.take() {
                runtime.bridge().stop("Plugin host closed.");
            }
        }
    }
}
pub fn init(cx: &mut App) {
    gpui_shell::init(cx);
    crate::panel::init(cx);
    cx.observe_global::<navidog_app_state::settings::Settings>(|cx| {
        let plugins = Plugins::global(cx);
        plugins.update(cx, |plugins, cx| {
            for installation in &plugins.installations {
                if let Some(runtime) = &installation.runtime {
                    runtime.refresh(cx);
                }
            }
        });
    })
    .detach();
    let root = navidog_model::paths::config_dir().join("plugins");
    let plugins = cx.new(|_| Plugins::new(root));
    cx.set_global(GlobalPlugins(plugins.clone()));
    let sessions = navidog_session::sessions::Sessions::global(cx);
    plugins.update(cx, |_, cx| {
        cx.subscribe(&sessions, |_, sessions, _, cx| {
            let ids = sessions.read(cx).open_ids().cloned().collect();
            cx.global_mut::<Contributions>().open_connections = ids;
            cx.refresh_windows();
        })
        .detach()
    });
    plugins.update(cx, |p, cx| {
        p.scan(cx);
        {
            p.watch = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(1))
                        .await;
                    let _ = this.update(cx, |plugins, cx| plugins.audit_runtime_changes(cx));
                }
            }));
        }
    });
}

impl Plugins {
    pub fn start_enabled(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids: Vec<_> = self
            .installations
            .iter()
            .filter(|p| self.approval(p.id()).enabled)
            .map(|p| p.id().to_string())
            .collect();
        for id in ids {
            if let Err(error) = self.reload(&id, window, cx) {
                tracing::warn!(plugin = id, "{error}");
            }
        }
    }
}

impl Plugins {
    fn audit_runtime_changes(&mut self, cx: &mut Context<Self>) {
        if cfg!(debug_assertions) {
            let changed: Vec<_> = self
                .installations
                .iter()
                .filter(|p| {
                    p.runtime.is_some() && p.source_revision != source_revision(&p.directory)
                })
                .map(|p| p.id().to_string())
                .collect();
            for id in changed {
                if let Some(installation) = self.installations.iter_mut().find(|p| p.id() == id) {
                    installation.source_revision = source_revision(&installation.directory);
                }
                let approval = self.approval(&id);
                let directory = self
                    .installations
                    .iter()
                    .find(|p| p.id() == id)
                    .unwrap()
                    .directory
                    .clone();
                match InstalledPlugin::read(&directory) {
                    Ok(plugin) if plugin.requested().is_subset(&approval.requested) => {
                        if let Some(window) = cx.windows().first().copied() {
                            let owner = cx.entity().downgrade();
                            cx.defer(move |cx| {
                                let _ = window.update(cx, |_, window, cx| {
                                    let _ = owner.update(cx, |plugins, cx| {
                                        if let Err(error) = plugins.reload(&id, window, cx) {
                                            plugins.stop(&id, &error, cx);
                                        }
                                    });
                                });
                            });
                        }
                    }
                    _ => {
                        self.stop(
                            &id,
                            "Plugin changed and needs validation or permission approval.",
                            cx,
                        );
                        if let Some(plugin) = self.installations.iter_mut().find(|p| p.id() == id) {
                            plugin.failure =
                                Some("Plugin changed; review permissions and reload.".into());
                        }
                    }
                }
            }
        }
        let failures: Vec<_> = self
            .installations
            .iter()
            .filter_map(|installation| {
                let runtime = installation.runtime.as_ref()?;
                runtime
                    .failure(cx)
                    .or_else(|| {
                        (!runtime.bridge().is_active())
                            .then(|| "Plugin event transport terminated. Reload to restart.".into())
                    })
                    .map(|error| (installation.id().to_string(), error))
            })
            .collect();
        for (id, error) in failures {
            self.stop(&id, &error, cx);
            if let Some(plugin) = self.installations.iter_mut().find(|p| p.id() == id) {
                plugin.failure = Some(error);
            }
        }
    }
}

fn source_revision(root: &std::path::Path) -> String {
    fn visit(root: &std::path::Path, paths: &mut Vec<std::path::PathBuf>) {
        if paths.len() > 4096 {
            return;
        }
        if let Ok(entries) = std::fs::read_dir(root) {
            for entry in entries.flatten() {
                if entry.file_name().to_string_lossy().starts_with('.') {
                    continue;
                }
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    visit(&entry.path(), paths);
                } else {
                    paths.push(entry.path());
                }
            }
        }
    }
    let mut paths = Vec::new();
    visit(root, &mut paths);
    paths.sort();
    use sha2::Digest as _;
    let mut hash = sha2::Sha256::new();
    for path in paths {
        if let Ok(metadata) = std::fs::metadata(&path) {
            hash.update(path.to_string_lossy().as_bytes());
            hash.update(metadata.len().to_le_bytes());
            hash.update(format!("{:?}", metadata.modified()).as_bytes());
        }
    }
    format!("{:x}", hash.finalize())
}
