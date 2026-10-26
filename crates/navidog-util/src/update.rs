//! Version and asset names for the in-app update check. No UI and no network.

use semver::Version;

/// `Some(version)` when `raw` is a semver strictly newer than `current`.
pub fn newer_version(raw: &str, current: &str) -> Option<String> {
    let latest = Version::parse(raw.trim().trim_start_matches('v')).ok()?;
    let current = Version::parse(current).ok()?;
    (latest > current).then(|| latest.to_string())
}

/// A nightly is newer when its `built_at` is strictly later than this build.
/// The same timestamp is the build that is running. A missing or unparseable time is not newer.
pub fn nightly_is_newer(release_built_at: &str, built_at: &str) -> bool {
    let (Ok(release), Ok(built)) = (
        chrono::DateTime::parse_from_rfc3339(release_built_at.trim()),
        chrono::DateTime::parse_from_rfc3339(built_at.trim()),
    ) else {
        return false;
    };
    release > built
}

/// `(os, arch, kind)` of a file the publish workflow uploads.
pub fn asset_identity(name: &str) -> Option<(&'static str, String, &'static str)> {
    if let Some(arch) = name
        .strip_prefix("Navidog-")
        .and_then(|rest| rest.strip_suffix(".dmg"))
    {
        return Some(("macos", arch.to_string(), "dmg"));
    }
    let rest = name.strip_prefix("navidog-")?;
    if let Some(arch) = rest.strip_suffix(".AppImage.tar.gz") {
        return Some(("linux", arch.to_string(), "appimage"));
    }
    let (os, file) = rest.split_once('-')?;
    let os = match os {
        "macos" => "macos",
        "windows" => "windows",
        "linux" => "linux",
        _ => return None,
    };
    const KINDS: [(&str, &str); 6] = [
        (".tar.gz", "tarball"),
        (".msi.zip", "msizip"),
        (".msi", "msi"),
        (".zip", "zip"),
        (".deb", "deb"),
        (".rpm", "rpm"),
    ];
    KINDS.iter().find_map(|(suffix, kind)| {
        file.strip_suffix(suffix)
            .map(|arch| (os, arch.to_string(), *kind))
    })
}

pub fn hex_encode(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

/// Release notes shown in the update dialog, cut on a char boundary.
pub fn notes_preview(notes: &str) -> String {
    let notes = notes.trim();
    if notes.chars().count() <= 800 {
        return notes.to_string();
    }
    let mut end = 800;
    while !notes.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &notes[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_version_compares_semver() {
        assert_eq!(newer_version("v0.2.0", "0.1.0").as_deref(), Some("0.2.0"));
        assert!(newer_version("0.1.0", "0.1.0").is_none());
        assert!(newer_version("0.0.9", "0.1.0").is_none());
        assert!(newer_version("nightly", "0.1.0").is_none());
    }

    #[test]
    fn nightly_is_newer_only_when_the_build_time_is_later() {
        assert!(!nightly_is_newer(
            "2026-10-16T00:00:00Z",
            "2026-10-16T00:00:00Z"
        ));
        assert!(nightly_is_newer(
            "2026-10-16T00:00:01Z",
            "2026-10-16T00:00:00Z"
        ));
        assert!(!nightly_is_newer(
            "2026-10-15T23:59:59Z",
            "2026-10-16T00:00:00Z"
        ));
        assert!(!nightly_is_newer("", "2026-10-16T00:00:00Z"));
        assert!(!nightly_is_newer("not-a-date", "2026-10-16T00:00:00Z"));
    }

    #[test]
    fn asset_names_map_to_os_arch_and_kind() {
        assert_eq!(
            asset_identity("Navidog-aarch64.dmg"),
            Some(("macos", "aarch64".into(), "dmg"))
        );
        assert_eq!(
            asset_identity("navidog-aarch64.AppImage.tar.gz"),
            Some(("linux", "aarch64".into(), "appimage"))
        );
        assert_eq!(
            asset_identity("navidog-windows-x86_64.msi"),
            Some(("windows", "x86_64".into(), "msi"))
        );
        assert_eq!(
            asset_identity("navidog-windows-x86_64.msi.zip"),
            Some(("windows", "x86_64".into(), "msizip"))
        );
        assert_eq!(
            asset_identity("navidog-linux-x86_64.tar.gz"),
            Some(("linux", "x86_64".into(), "tarball"))
        );
        assert!(asset_identity("SHA256SUMS").is_none());
    }
}
