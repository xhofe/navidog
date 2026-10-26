use std::path::{Path, PathBuf};

/// Config directory. `NAVIDOG_HOME` overrides it (tests and side-by-side installs).
pub fn config_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("NAVIDOG_HOME") {
        return PathBuf::from(dir);
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("navidog")
}

pub fn log_dir() -> PathBuf {
    if cfg!(target_os = "macos") && std::env::var_os("NAVIDOG_HOME").is_none() {
        return dirs::home_dir()
            .map(|home| home.join("Library/Logs/navidog"))
            .unwrap_or_else(|| config_dir().join("logs"));
    }
    config_dir().join("logs")
}

/// Expands `~` in a user-entered path. Other text is returned as written.
pub fn expand_home(path: &str) -> PathBuf {
    PathBuf::from(shellexpand::tilde(path).into_owned())
}

/// Per-connection settings and files. The id is encoded so a hand-edited or
/// imported `connections.json` cannot escape the profile folder with `..`.
pub fn profile_dir(connection_id: &str) -> PathBuf {
    profile_dir_in(&config_dir(), connection_id)
}

pub fn profile_dir_in(root: &Path, connection_id: &str) -> PathBuf {
    root.join("profiles")
        .join(crate::queries::encode_segment(connection_id))
}

/// Writes `bytes` next to `path`, syncs them, then renames over the target.
/// `tempfile` gives each write its own name and `persist` replaces the target
/// atomically, so two processes cannot share a half-written `.tmp` file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|err| err.error)?;
    // The rename itself is only durable once the directory entry is flushed.
    #[cfg(unix)]
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_and_leaves_no_temp_file() {
        let dir = std::env::temp_dir().join(format!(
            "navidog-atomic-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        let path = dir.join("settings.json");
        write_atomic(&path, b"first").unwrap();
        write_atomic(&path, b"second").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        let files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(files, [std::ffi::OsString::from("settings.json")]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn connection_ids_cannot_escape_the_profile_folder() {
        let root = std::env::temp_dir().join("navidog-paths-test");
        let safe = profile_dir_in(&root, "c-0123456789abcdef");
        assert!(safe.starts_with(root.join("profiles")));
        let escaped = profile_dir_in(&root, "../../outside");
        assert!(escaped.starts_with(root.join("profiles")));
        assert!(
            !escaped
                .components()
                .any(|c| c == std::path::Component::ParentDir)
        );
    }
}
