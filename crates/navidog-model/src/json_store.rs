//! One JSON file with the same corruption policy everywhere: keep a `.bak` of
//! an unreadable file before falling back to defaults, and write atomically.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::paths;

pub struct JsonStore {
    path: PathBuf,
}

impl JsonStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Loads the file, or the type's default when it is missing. An unreadable
    /// file is copied to `<name>.bak` before defaults replace it.
    pub fn load<T: DeserializeOwned + Default>(&self) -> T {
        self.load_optional().unwrap_or_default()
    }

    /// Like [`load`](Self::load), but a missing file stays `None`.
    pub fn load_optional<T: DeserializeOwned>(&self) -> Option<T> {
        let bytes = std::fs::read(&self.path).ok()?;
        match serde_json::from_slice(&bytes) {
            Ok(value) => Some(value),
            Err(err) => {
                tracing::error!(
                    "{} cannot be parsed, backing it up: {err}",
                    self.path.display()
                );
                let backup = self.path.with_extension("json.bak");
                if let Err(err) = std::fs::copy(&self.path, &backup) {
                    tracing::error!("could not write {}: {err}", backup.display());
                }
                None
            }
        }
    }

    pub fn save<T: Serialize>(&self, value: &T) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(value)
            .map_err(|err| std::io::Error::other(err.to_string()))?;
        paths::write_atomic(&self.path, &bytes)
    }

    /// Saves and logs a failure; used by stores whose callers cannot surface
    /// an error. Prefer [`save`](Self::save) when the caller can.
    pub fn save_lossy<T: Serialize>(&self, value: &T) {
        if let Err(err) = self.save(value) {
            tracing::error!("failed to save {}: {err}", self.path.display());
        }
    }
}
