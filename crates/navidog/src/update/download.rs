use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use navidog_util::update::hex_encode;
use sha2::{Digest, Sha256};

use super::http::*;
use super::manifest::*;

pub(super) fn download_dir() -> Result<PathBuf, InstallError> {
    let dir = std::env::temp_dir().join("navidog-update");
    match std::fs::symlink_metadata(&dir) {
        Ok(meta) => {
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return Err(InstallError::Other(
                    "the update path is not a private directory".into(),
                ));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if meta.permissions().mode() & 0o077 != 0 {
                    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                        .map_err(|e| InstallError::Other(e.to_string()))?;
                }
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                let mut builder = std::fs::DirBuilder::new();
                builder.mode(0o700);
                match builder.create(&dir) {
                    Ok(()) => {}
                    Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                        // Another navigation or test created it between the
                        // metadata check and this call; re-check it.
                        return download_dir();
                    }
                    Err(err) => return Err(InstallError::Other(err.to_string())),
                }
            }
            #[cfg(not(unix))]
            match std::fs::create_dir_all(&dir) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    return download_dir();
                }
                Err(err) => return Err(InstallError::Other(err.to_string())),
            }
        }
        Err(err) => return Err(InstallError::Other(err.to_string())),
    }
    Ok(dir)
}

/// Downloads `asset` into the update directory and checks its SHA-256.
///
/// `on_progress(downloaded, total)` runs as bytes arrive. `total` is the
/// response `Content-Length` when it is present, otherwise `asset.size`, and
/// `0` when neither is known.
///
/// ponytail: `cancel` is noticed between chunks. The connect and each blocking
/// read run to completion; aborting an in-flight read means replacing this ureq loop.
pub fn download_verified(
    asset: &UpdateAsset,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<PathBuf, InstallError> {
    if asset.sha256.trim().is_empty() {
        return Err(InstallError::Checksum);
    }
    let Some(name) = download_file_name(&asset.name) else {
        return Err(InstallError::Other("download name is empty".into()));
    };
    let dir = download_dir()?;
    let path = dir.join(name);
    // A stale file (or a symlink planted by another process) must not be
    // followed or truncated; the private directory makes the window small.
    if std::fs::symlink_metadata(&path).is_ok() {
        std::fs::remove_file(&path).map_err(|e| InstallError::Other(e.to_string()))?;
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(300)))
        .build()
        .into();
    let mut response = agent
        .get(&asset.url)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| InstallError::Other(e.to_string()))?;
    let total = response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(asset.size);
    on_progress(0, total);
    let mut reader = response.body_mut().as_reader();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| InstallError::Other(e.to_string()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut downloaded = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            drop(file);
            let _ = std::fs::remove_file(&path);
            return Err(InstallError::Cancelled);
        }
        let n = reader
            .read(&mut buf)
            .map_err(|e| InstallError::Other(e.to_string()))?;
        if n == 0 {
            break;
        }
        downloaded += n as u64;
        if downloaded > MAX_DOWNLOAD {
            let _ = std::fs::remove_file(&path);
            return Err(InstallError::Other("download is larger than 512 MB".into()));
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])
            .map_err(|e| InstallError::Other(e.to_string()))?;
        on_progress(downloaded, total);
    }
    if cancel.load(Ordering::Relaxed) {
        drop(file);
        let _ = std::fs::remove_file(&path);
        return Err(InstallError::Cancelled);
    }
    file.sync_all()
        .map_err(|e| InstallError::Other(e.to_string()))?;
    let got = hex_encode(&hasher.finalize());
    if !got.eq_ignore_ascii_case(asset.sha256.trim()) {
        let _ = std::fs::remove_file(&path);
        return Err(InstallError::Checksum);
    }
    Ok(path)
}

/// `0` when the total is unknown. Otherwise a percentage in `0..=100`.
pub(super) fn download_percent(done: u64, total: u64) -> u64 {
    done.saturating_mul(100)
        .checked_div(total)
        .unwrap_or(0)
        .min(100)
}

/// Repaint when the percentage changes. An unknown total has no percentage, so
/// any new byte count repaints the indeterminate bar's caption.
pub(super) fn progress_changed(shown: Option<(u64, u64)>, done: u64, total: u64) -> bool {
    let Some((prev_done, prev_total)) = shown else {
        return true;
    };
    if total == 0 {
        done != prev_done
    } else {
        download_percent(done, total) != download_percent(prev_done, prev_total)
    }
}

pub(super) fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    let n = bytes as f64;
    let (value, unit) = if n < KIB {
        return format!("{bytes} B");
    } else if n < KIB * KIB {
        (n / KIB, "KB")
    } else if n < KIB * KIB * KIB {
        (n / (KIB * KIB), "MB")
    } else {
        (n / (KIB * KIB * KIB), "GB")
    };
    format!("{value:.1} {unit}")
}

/// When a check reached GitHub. A transport error leaves this `None`, so the
/// next launch can try again instead of waiting two days.
pub(super) fn checked_at(found: &Result<Availability, String>, now: i64) -> Option<i64> {
    found.is_ok().then_some(now)
}

pub(super) fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
