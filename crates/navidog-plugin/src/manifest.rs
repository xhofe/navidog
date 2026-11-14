//! Discovery validates inert manifests and never evaluates plugin source.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use gpui_shell::plugin::PluginManifest;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

const PERMISSIONS: &[&str] = &[
    "connections.read",
    "query.execute",
    "grid.read",
    "editor.write",
];
const LOCATIONS: &[&str] = &[
    "tools.menu",
    "main.toolbar",
    "objects.context",
    "grid.context",
    "editor.context",
];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Command {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub title_zh: Option<String>,
    pub menu: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub shortcut: Option<String>,
    #[serde(default)]
    pub when: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Format {
    pub id: String,
    pub name: String,
    pub extensions: Vec<String>,
    #[serde(default)]
    pub import: bool,
    #[serde(default)]
    pub export: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Panel {
    pub id: String,
    pub title: String,
    pub entry: String,
    pub placement: String,
    pub size: f32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Extensions {
    pub api: u32,
    #[serde(default)]
    pub permissions: BTreeSet<String>,
    #[serde(default)]
    pub commands: Vec<Command>,
    #[serde(default)]
    pub formats: Vec<Format>,
    #[serde(default)]
    pub cell_formatters: Vec<Value>,
    #[serde(default)]
    pub drivers: Vec<Value>,
    #[serde(default)]
    pub panels: Vec<Panel>,
}

impl Extensions {
    pub fn parse(source: &str) -> Result<Self, String> {
        let extensions: Self = serde_json::from_str(source).map_err(|e| e.to_string())?;
        if extensions.api != 1 {
            return Err("Unsupported navidog plugin API version.".into());
        }
        for permission in &extensions.permissions {
            if !PERMISSIONS.contains(&permission.as_str()) {
                return Err(format!("Unknown permission: {permission}"));
            }
        }
        let mut ids = BTreeSet::new();
        let mut id = |id: &str| -> Result<(), String> {
            if id.is_empty()
                || !id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
            {
                return Err(format!("Invalid contribution id: {id}"));
            }
            if !ids.insert(id.to_owned()) {
                return Err(format!("Duplicate contribution id: {id}"));
            }
            Ok(())
        };
        for command in &extensions.commands {
            id(&command.id)?;
            if !LOCATIONS.contains(&command.menu.as_str()) {
                return Err(format!("Unknown command location: {}", command.menu));
            }
            if command.title.trim().is_empty() {
                return Err("A command needs a title.".into());
            }
            if command.when.as_deref().is_some_and(|c| {
                !["connection.open", "object.selected", "grid", "editor"].contains(&c)
            }) {
                return Err("Unknown command availability condition.".into());
            }
            if let Some(icon) = &command.icon {
                validate_entry(icon)?;
            }
        }
        for format in &extensions.formats {
            id(&format.id)?;
            if format.name.trim().is_empty()
                || (!format.import && !format.export)
                || format.extensions.is_empty()
                || format
                    .extensions
                    .iter()
                    .any(|e| e.is_empty() || !e.bytes().all(|c| c.is_ascii_alphanumeric()))
            {
                return Err(
                    "A format needs a name, extensions and an import or export handler.".into(),
                );
            }
        }
        for panel in &extensions.panels {
            id(&panel.id)?;
            validate_entry(&panel.entry)?;
            if !["left", "right", "bottom", "center"].contains(&panel.placement.as_str())
                || !panel.size.is_finite()
                || panel.size <= 0.
                || panel.title.trim().is_empty()
            {
                return Err("Invalid panel placement, size or title.".into());
            }
        }
        for driver in &extensions.drivers {
            if let Some(icon) = driver.get("icon").and_then(Value::as_str) {
                validate_entry(icon)?;
            }
            serde_json::from_value::<crate::driver::DriverDescriptor>(driver.clone())
                .map_err(|e| e.to_string())?
                .validate()?;
        }
        for formatter in &extensions.cell_formatters {
            serde_json::from_value::<navidog_model::cell_formatter::Formatter>(formatter.clone())
                .map_err(|e| e.to_string())?
                .validate()?;
        }
        for descriptor in extensions.cell_formatters.iter().chain(&extensions.drivers) {
            id(descriptor
                .get("id")
                .and_then(Value::as_str)
                .ok_or("Contribution needs an id.")?)?;
        }
        Ok(extensions)
    }
}

fn validate_entry(entry: &str) -> Result<(), String> {
    let path = Path::new(entry);
    if entry.is_empty()
        || entry.contains('\\')
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(format!(
            "Entry must be relative and remain inside the plugin: {entry}"
        ));
    }
    Ok(())
}

/// A capability is a selectable complete leaf grant. HTTP rules remain indivisible records.
#[derive(Clone, Debug)]
pub struct InstalledPlugin {
    directory: PathBuf,
    shell: PluginManifest,
    extensions: Extensions,
    shell_json: Value,
    requested: BTreeMap<String, (Vec<String>, Value)>,
    fingerprint: String,
}

impl InstalledPlugin {
    pub fn read(directory: &Path) -> Result<Self, String> {
        let shell = PluginManifest::read(directory).map_err(|e| e.to_string())?;
        let shell_json: Value = serde_json::from_slice(
            &std::fs::read(directory.join("gpui-shell.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let extensions = Extensions::parse(
            &std::fs::read_to_string(directory.join("navidog.json")).map_err(|e| e.to_string())?,
        )?;
        let mut requested = BTreeMap::new();
        if let Some(capabilities) = shell_json.get("capabilities") {
            collect_grants(capabilities, &mut Vec::new(), &mut requested);
        }
        for (name, dependency) in shell.dependencies() {
            let value = serde_json::json!({"git":dependency.git(),"branch":dependency.branch(),"tag":dependency.tag(),"entry":dependency.entry()});
            requested.insert(
                format!("shell.dependency.{name}={value}"),
                (vec!["dependency".into()], value),
            );
        }
        let mut all: BTreeSet<_> = requested.keys().cloned().collect();
        all.extend(extensions.permissions.iter().cloned());
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&all).map_err(|e| e.to_string())?)
        );
        Ok(Self {
            directory: directory.to_path_buf(),
            shell,
            extensions,
            shell_json,
            requested,
            fingerprint,
        })
    }
    pub fn directory(&self) -> &Path {
        &self.directory
    }
    pub fn shell(&self) -> &PluginManifest {
        &self.shell
    }
    pub fn extensions(&self) -> &Extensions {
        &self.extensions
    }
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
    pub fn requested(&self) -> BTreeSet<String> {
        self.requested
            .keys()
            .chain(self.extensions.permissions.iter())
            .cloned()
            .collect()
    }
    /// Reparse selected Shell grant leaves through the official parser before resolving anchors.
    pub fn capabilities(
        &self,
        granted: &BTreeSet<String>,
        data_dir: &Path,
    ) -> Result<gpui_shell::Capabilities, String> {
        if !granted.is_subset(&self.requested()) {
            return Err("Approval includes an unrequested capability.".into());
        }
        let mut capabilities = Value::Object(Default::default());
        for key in granted {
            if let Some((path, value)) = self.requested.get(key) {
                if path.first().is_some_and(|p| p == "dependency") {
                    continue;
                }
                insert_grant(&mut capabilities, path, value);
            }
        }
        if self.shell.dependencies().keys().any(|name| {
            !granted
                .iter()
                .any(|grant| grant.starts_with(&format!("shell.dependency.{name}=")))
        }) {
            return Err("Declared dependencies must be approved before loading.".into());
        }
        let mut manifest = self.shell_json.clone();
        manifest["capabilities"] = capabilities;
        let parsed = PluginManifest::parse(&manifest.to_string()).map_err(|e| e.to_string())?;
        Ok(parsed.capabilities(&self.directory, data_dir))
    }
}

fn collect_grants(
    value: &Value,
    path: &mut Vec<String>,
    out: &mut BTreeMap<String, (Vec<String>, Value)>,
) {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                path.push(key.clone());
                collect_grants(value, path, out);
                path.pop();
            }
        }
        Value::Array(values) => {
            for value in values {
                let key = format!("shell.{}={value}", path.join("."));
                out.insert(key, (path.clone(), Value::Array(vec![value.clone()])));
            }
        }
        Value::Bool(false) | Value::Null => {}
        _ => {
            out.insert(
                format!("shell.{}={value}", path.join(".")),
                (path.clone(), value.clone()),
            );
        }
    }
}

fn insert_grant(target: &mut Value, path: &[String], value: &Value) {
    if let Some((key, rest)) = path.split_first() {
        if target.get(key).is_none() {
            target[key] = Value::Object(Default::default());
        }
        insert_grant(&mut target[key], rest, value);
    } else if let (Value::Array(existing), Value::Array(values)) = (&mut *target, value) {
        existing.extend(values.iter().cloned());
    } else {
        *target = value.clone();
    }
}

/// Invalid entries stay visible with their directory and failure reason.
pub fn discover(root: &Path) -> Vec<(PathBuf, Result<InstalledPlugin, String>)> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let plugin = InstalledPlugin::read(&path);
            (path, plugin)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_locations_permissions_ids_and_entries() {
        for source in [
            r#"{"api":2}"#,
            r#"{"api":1,"permissions":["root"]}"#,
            r#"{"api":1,"commands":[{"id":"a","title":"A","menu":"unknown"}]}"#,
            r#"{"api":1,"panels":[{"id":"p","title":"P","entry":"../p.js","placement":"right","size":280}]}"#,
            r#"{"api":1,"typo":true}"#,
        ] {
            assert!(Extensions::parse(source).is_err(), "{source}");
        }
        for location in LOCATIONS {
            assert!(
                Extensions::parse(&format!(
                    r#"{{"api":1,"commands":[{{"id":"a","title":"A","menu":"{location}"}}]}}"#
                ))
                .is_ok()
            );
        }
    }
    #[test]
    fn discovery_is_inert_and_selected_grants_are_independent() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = dir.path().join("com.example.test");
        std::fs::create_dir(&plugin).unwrap();
        std::fs::write(plugin.join("gpui-shell.json"), r#"{"id":"com.example.test","name":"Test","entry":"main.js","capabilities":{"clipboard":{"read":true,"write":true}}}"#).unwrap();
        std::fs::write(
            plugin.join("navidog.json"),
            r#"{"api":1,"permissions":["grid.read"]}"#,
        )
        .unwrap();
        // No source file is needed to discover and approve an installation.
        let installed = InstalledPlugin::read(&plugin).unwrap();
        let granted = BTreeSet::from(["shell.clipboard.read=true".into()]);
        let caps = installed.capabilities(&granted, dir.path()).unwrap();
        assert!(caps.is_clipboard_readable());
        assert!(!caps.is_clipboard_writable());
        assert_eq!(discover(dir.path()).len(), 1);
        assert_eq!(installed.requested().len(), 3);
    }
}
