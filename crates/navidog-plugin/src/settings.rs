//! Persisted approvals are separate from discovered, untrusted manifests.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Host limits complement the VM's fixed watchdog and stack limit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct Limits {
    pub timeout_seconds: f64,
    pub events: usize,
    pub requests: usize,
    pub batch_rows: usize,
    pub batch_bytes: usize,
    pub result_rows: usize,
    pub result_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            timeout_seconds: 30.,
            events: 256,
            requests: 64,
            batch_rows: 1_000,
            batch_bytes: 1_048_576,
            result_rows: 100_000,
            result_bytes: 67_108_864,
        }
    }
}

impl Limits {
    pub fn validate(&self) -> Result<(), String> {
        if !self.timeout_seconds.is_finite()
            || self.timeout_seconds <= 0.
            || self.timeout_seconds > std::time::Duration::MAX.as_secs_f64()
        {
            return Err("Request timeout must be a finite positive duration.".into());
        }
        if self.requests == 0 || self.requests > 1024 {
            return Err("In-flight requests must be between 1 and 1,024.".into());
        }
        if [
            self.events,
            self.batch_rows,
            self.batch_bytes,
            self.result_rows,
            self.result_bytes,
        ]
        .contains(&0)
            || self.result_rows < self.batch_rows
            || self.result_bytes < self.batch_bytes
        {
            return Err(
                "Limits must be positive and cumulative limits must cover one batch.".into(),
            );
        }
        Ok(())
    }
}

/// The fingerprint covers requested grants; selected grants may be a subset.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct Approval {
    pub enabled: bool,
    pub fingerprint: String,
    pub requested: BTreeSet<String>,
    pub granted: BTreeSet<String>,
    pub limits: Limits,
}

impl Approval {
    /// Expansions require confirmation even when the expanded item was previously unchecked.
    pub fn reconcile(&mut self, requested: BTreeSet<String>, fingerprint: String) -> bool {
        let expanded = !requested.is_subset(&self.requested);
        self.granted.retain(|grant| requested.contains(grant));
        if expanded {
            self.enabled = false;
        } else {
            self.requested = requested;
            self.fingerprint = fingerprint;
        }
        expanded
    }

    pub fn confirm(
        requested: BTreeSet<String>,
        granted: BTreeSet<String>,
        fingerprint: String,
        limits: Limits,
    ) -> Result<Self, String> {
        limits.validate()?;
        if !granted.is_subset(&requested) {
            return Err("Cannot grant a capability absent from the validated manifest.".into());
        }
        Ok(Self {
            enabled: true,
            fingerprint,
            requested,
            granted,
            limits,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginsFile {
    version: u32,
    plugins: BTreeMap<String, Approval>,
}

impl Default for PluginsFile {
    fn default() -> Self {
        Self {
            version: 1,
            plugins: BTreeMap::new(),
        }
    }
}

impl PluginsFile {
    pub fn load(path: &Path) -> Result<Self, String> {
        match std::fs::read(path) {
            Ok(bytes) => {
                let file: Self = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                if file.version != 1 {
                    return Err("Unsupported plugins.json version.".into());
                }
                for approval in file.plugins.values() {
                    approval.limits.validate()?;
                }
                Ok(file)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn approval(&self, id: &str) -> Option<&Approval> {
        self.plugins.get(id)
    }

    /// Commit a candidate before replacing in-memory state.
    pub fn save_approval(
        &mut self,
        path: &Path,
        id: String,
        approval: Approval,
    ) -> Result<(), String> {
        approval.limits.validate()?;
        let mut candidate = self.clone();
        candidate.plugins.insert(id, approval);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let bytes = serde_json::to_vec_pretty(&candidate).map_err(|e| e.to_string())?;
        navidog_model::paths::write_atomic(path, &bytes).map_err(|e| e.to_string())?;
        *self = candidate;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn grants(values: &[&str]) -> BTreeSet<String> {
        values.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn partial_grants_survive_code_updates_but_expansion_disables() {
        let mut a = Approval::confirm(
            grants(&["a", "b"]),
            grants(&["a"]),
            "one".into(),
            Limits::default(),
        )
        .unwrap();
        assert!(!a.reconcile(grants(&["a", "b"]), "one".into()));
        assert!(a.enabled);
        assert!(a.reconcile(grants(&["a", "b", "c"]), "two".into()));
        assert!(!a.enabled);
        assert_eq!(a.granted, grants(&["a"]));
        assert!(
            Approval::confirm(grants(&["a"]), grants(&["b"]), "".into(), Limits::default())
                .is_err()
        );
    }

    #[test]
    fn finite_limits_and_transactional_save() {
        for timeout in [0., -1., f64::NAN, f64::INFINITY] {
            let l = Limits {
                timeout_seconds: timeout,
                ..Limits::default()
            };
            assert!(l.validate().is_err());
        }
        let l = Limits {
            requests: 1025,
            ..Limits::default()
        };
        assert!(l.validate().is_err());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plugins.json");
        let mut file = PluginsFile::default();
        file.save_approval(&path, "p".into(), Approval::default())
            .unwrap();
        assert!(
            !PluginsFile::load(&path)
                .unwrap()
                .approval("p")
                .unwrap()
                .enabled
        );
        assert!(
            file.save_approval(dir.path(), "q".into(), Approval::default())
                .is_err()
        );
        assert!(file.approval("q").is_none());
    }
}
