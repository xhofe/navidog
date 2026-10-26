use super::*;

use std::time::Duration;

use gpui_kit::Window;
use navidog_util::update::{newer_version, nightly_is_newer};
use navidog_util::update_manifest::Asset as ManifestAsset;
use serde::Deserialize;

use super::http::{listed_releases, read_updates};

pub(super) fn manifest_url() -> String {
    format!("{RELEASE_MIRROR}/{REPO}/releases/download/nightly/latest.json")
}

/// Used when the nightly release does not exist yet.
pub(super) fn manifest_url_stable() -> String {
    format!("{RELEASE_MIRROR}/{REPO}/releases/latest/download/latest.json")
}

pub(super) const NIGHTLY_TAG: &str = "nightly";
pub(super) const MAX_DOWNLOAD: u64 = 512 * 1024 * 1024;
pub(super) const USER_AGENT: &str = concat!("navidog/", env!("CARGO_PKG_VERSION"));

pub const BUILD_TIMESTAMP: &str = env!("NAVIDOG_BUILD_TIMESTAMP");
pub const BUILD_CHANNEL: &str = env!("NAVIDOG_BUILD_CHANNEL");

pub fn is_nightly_build() -> bool {
    BUILD_CHANNEL == "nightly"
}

pub fn is_smoke_test() -> bool {
    std::env::var("NAVIDOG_SMOKE_TEST").is_ok_and(|v| v == "1")
}

pub fn smoke_gate_is_window() -> bool {
    std::env::var("NAVIDOG_SMOKE_GATE").is_ok_and(|v| v == "window")
}

/// Exits 2 if no frame (or the window gate) succeeds within 30 seconds.
pub fn arm_smoke_watchdog() {
    if !is_smoke_test() {
        return;
    }
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(30));
        eprintln!("NAVIDOG_SMOKE_TIMEOUT: no frame painted within 30s");
        std::process::exit(2);
    });
}

/// Hooks the open window: exit 0 on the first frame, or after the window has
/// stayed up for 5 seconds when `NAVIDOG_SMOKE_GATE=window`.
pub fn arm_smoke_window(window: &mut Window) {
    if !is_smoke_test() {
        return;
    }
    println!("NAVIDOG_SMOKE_WINDOW");
    if smoke_gate_is_window() {
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(5));
            println!("NAVIDOG_SMOKE_OK (window gate)");
            std::process::exit(0);
        });
    }
    window.on_next_frame(|_, _| {
        println!("NAVIDOG_SMOKE_OK");
        std::process::exit(0);
    });
}

#[derive(Clone, Debug)]
pub struct UpdateAsset {
    pub url: String,
    pub sha256: String,
    pub name: String,
    /// Advertised size from `latest.json`. `0` when the manifest omitted it.
    pub size: u64,
}

#[derive(Clone, Debug)]
pub struct UpdateInfo {
    pub version: String,
    pub current: String,
    pub page_url: String,
    pub notes: String,
    pub asset: Option<UpdateAsset>,
}

pub(crate) enum Availability {
    Available(UpdateInfo),
    Current,
    Unpublished,
}

/// The release list either has nothing published, nothing newer, or one release to offer.
#[derive(Debug)]
pub(super) enum Listed<'a> {
    Unpublished,
    Current,
    Chosen(&'a UpdateRelease),
}

/// Why a download was not installed.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum InstallError {
    /// The checksum was missing or did not match.
    Checksum,
    /// The user cancelled before the installer was applied.
    Cancelled,
    Other(String),
}

/// `latest.json`. `releases` is the index; the other fields are this release.
/// An older file has no `releases` and is treated as that one release.
#[derive(Debug, Deserialize)]
pub(super) struct Manifest {
    #[serde(default)]
    pub(super) version: String,
    #[serde(default)]
    pub(super) tag: String,
    #[serde(default, deserialize_with = "empty_if_null")]
    pub(super) body: String,
    #[serde(default)]
    pub(super) assets: Vec<ManifestAsset>,
    #[serde(default)]
    pub(super) releases: Vec<UpdateRelease>,
}

/// One entry of `latest.json`'s `releases`. `body` is null when there are no notes.
#[derive(Debug, Deserialize)]
pub(super) struct UpdateRelease {
    pub(super) tag: String,
    #[serde(default)]
    pub(super) version: String,
    #[serde(default)]
    pub(super) prerelease: bool,
    #[serde(default)]
    pub(super) draft: bool,
    #[serde(default, deserialize_with = "empty_if_null")]
    pub(super) published_at: String,
    /// Build clock baked into a nightly. Empty for every other release.
    #[serde(default, deserialize_with = "empty_if_null")]
    pub(super) built_at: String,
    #[serde(default, deserialize_with = "empty_if_null")]
    pub(super) body: String,
    #[serde(default)]
    pub(super) assets: Vec<ManifestAsset>,
}

pub(super) fn empty_if_null<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

/// The browser page for a tag. The file mirror does not serve this page.
pub(super) fn release_page(tag: &str) -> String {
    format!("https://github.com/{REPO}/releases/tag/{tag}")
}

/// Rewrite a GitHub release file for this repo onto the file mirror.
/// Another host, including an address that is already mirrored, stays as it is.
pub(super) fn mirror_release_file(url: &str) -> String {
    let github = format!("https://github.com/{REPO}/");
    match url.strip_prefix(github.as_str()) {
        Some(rest) => format!("{RELEASE_MIRROR}/{REPO}/{rest}"),
        None => url.to_string(),
    }
}

pub(super) fn pick_asset_for(
    assets: &[ManifestAsset],
    os: &str,
    arch: &str,
) -> Option<UpdateAsset> {
    let preferred = match os {
        "macos" => "dmg",
        "windows" => "msi",
        "linux" => "appimage",
        _ => return None,
    };
    let mut here = assets.iter().filter(|a| a.os == os && a.arch == arch);
    let chosen = here
        .clone()
        .find(|a| a.kind == preferred)
        .or_else(|| here.find(|_| true))?;
    Some(UpdateAsset {
        url: mirror_release_file(&chosen.url),
        sha256: chosen.sha256.clone(),
        name: chosen.name.clone(),
        size: chosen.size,
    })
}

/// The installer for this machine. `None` when the release has none, or the
/// checksum is empty: those are not installed. The dialog then offers the page.
pub(super) fn installer_for(release: &UpdateRelease) -> Option<UpdateAsset> {
    let Some(asset) = pick_asset_for(
        &release.assets,
        std::env::consts::OS,
        std::env::consts::ARCH,
    ) else {
        tracing::warn!(
            tag = %release.tag,
            os = std::env::consts::OS,
            arch = std::env::consts::ARCH,
            "update: no installer for this computer"
        );
        return None;
    };
    if asset.sha256.trim().is_empty() {
        tracing::warn!(tag = %release.tag, "update: installer has no checksum");
        return None;
    }
    Some(asset)
}

/// What About and the update dialog call this build.
///
/// A nightly shows the channel and the UTC build time baked by CI, which is the
/// same clock as the GitHub nightly notes. It does not show the Cargo.toml
/// version. A stable build shows the version compiled in, which a tag build
/// stamps from the tag before compiling.
pub(crate) fn current_label() -> String {
    version_label(env!("CARGO_PKG_VERSION"), BUILD_CHANNEL, BUILD_TIMESTAMP)
}

pub(crate) fn version_label(version: &str, channel: &str, built_at: &str) -> String {
    if channel == "nightly" {
        let date = built_at.get(..10).unwrap_or("");
        let time = built_at.get(11..16).unwrap_or("");
        format!("nightly ({date} {time} UTC)")
    } else {
        version.to_string()
    }
}

/// A 404 from both index URLs means nothing has been published.
/// Any other failure stays an error, with `detail` for the notification.
pub(super) fn missing_latest(status: Option<u16>, detail: &str) -> Result<Availability, String> {
    if status == Some(404) {
        Ok(Availability::Unpublished)
    } else {
        Err(detail.to_string())
    }
}

/// Newest published release that this build should offer.
///
/// Entries are ordered by `published_at`, newest first. Drafts do not count as
/// published. Without `include_prerelease`, pre-releases and `nightly` are skipped.
/// `built_at` is this binary's build clock; a nightly is newer only when its own
/// `built_at` is strictly later.
pub(super) fn pick_release<'a>(
    releases: &'a [UpdateRelease],
    include_prerelease: bool,
    built_at: &str,
) -> Listed<'a> {
    let mut order: Vec<usize> = (0..releases.len()).collect();
    order.sort_by(|&a, &b| releases[b].published_at.cmp(&releases[a].published_at));
    let mut published = false;
    for release in order.iter().map(|&index| &releases[index]) {
        if release.draft {
            continue;
        }
        if !include_prerelease && (release.prerelease || release.tag == NIGHTLY_TAG) {
            continue;
        }
        published = true;
        if release.tag == NIGHTLY_TAG {
            if nightly_is_newer(&release.built_at, built_at) {
                return Listed::Chosen(release);
            }
            continue;
        }
        let version = if release.version.is_empty() {
            release.tag.as_str()
        } else {
            release.version.as_str()
        };
        if newer_version(version, env!("CARGO_PKG_VERSION")).is_some() {
            return Listed::Chosen(release);
        }
    }
    if published {
        Listed::Current
    } else {
        Listed::Unpublished
    }
}

pub(super) fn offered_version(release: &UpdateRelease) -> String {
    if release.tag == NIGHTLY_TAG {
        let built = release.built_at.get(..10).unwrap_or(NIGHTLY_TAG);
        return format!("{NIGHTLY_TAG} {built}");
    }
    let raw = if release.version.is_empty() {
        release.tag.as_str()
    } else {
        release.version.as_str()
    };
    newer_version(raw, env!("CARGO_PKG_VERSION"))
        .unwrap_or_else(|| raw.trim_start_matches('v').to_string())
}

/// Blocking. Run it off the UI thread.
pub fn fetch_latest_release(include_prerelease: bool) -> Result<Availability, String> {
    let text = match read_updates() {
        Ok(text) => text,
        Err(err) => return missing_latest(err.status, &err.detail),
    };
    let manifest: Manifest = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let releases = listed_releases(manifest);
    let release = match pick_release(&releases, include_prerelease, BUILD_TIMESTAMP) {
        Listed::Unpublished => return Ok(Availability::Unpublished),
        Listed::Current => return Ok(Availability::Current),
        Listed::Chosen(release) => release,
    };
    Ok(Availability::Available(UpdateInfo {
        version: offered_version(release),
        current: current_label(),
        page_url: release_page(&release.tag),
        notes: release.body.trim().to_string(),
        asset: installer_for(release),
    }))
}
