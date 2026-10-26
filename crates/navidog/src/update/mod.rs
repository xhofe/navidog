//! In-app update check and install.
//!
//! Both channels read `latest.json` from [`RELEASE_MIRROR`]. The file's
//! `releases` list is every published release; the top-level fields stay this
//! release's own version and assets. The stable channel skips pre-releases and
//! the rolling `nightly` tag. Nightly is compared by `built_at`, the same clock
//! baked into the binary. A download
//! is installed only when its SHA-256 matches the manifest. A missing file or
//! an empty checksum is not installed.

mod install;

/// Release files. This mirror only serves the `xhofe` user.
const RELEASE_MIRROR: &str = "https://gh.nn.ci";
const REPO: &str = "xhofe/navidog";

mod download;
mod http;
mod manifest;
mod ui;

#[cfg(test)]
mod tests;

pub(crate) use manifest::current_label;
pub use manifest::{arm_smoke_watchdog, arm_smoke_window, is_nightly_build, is_smoke_test};
pub use ui::{manual_check, startup_check};
