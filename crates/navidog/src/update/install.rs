//! Applying a downloaded update: mount/copy on macOS, run the installer on
//! Windows, and hand the AppImage to the desktop elsewhere. UI lives in the
//! parent module.

use std::path::{Path, PathBuf};

pub enum Delivery {
    /// The running `.app` was replaced. Restart to use it.
    #[cfg(target_os = "macos")]
    Replaced,
    /// The installer was handed to the operating system.
    HandedToOs,
}

pub(super) fn cleanup_download(path: &Path) {
    let _ = std::fs::remove_file(path);
    if let Some(dir) = path.parent()
        && dir.file_name().is_some_and(|name| name == "navidog-update")
    {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(any(test, target_os = "macos"))]
pub fn bundle_from_exe(path: &Path) -> Option<PathBuf> {
    let mut cursor = path.to_path_buf();
    loop {
        if cursor.extension().is_some_and(|ext| ext == "app") {
            return Some(cursor);
        }
        if !cursor.pop() {
            return None;
        }
    }
}

pub fn install(path: &Path) -> Result<Delivery, String> {
    #[cfg(target_os = "macos")]
    {
        install_macos(path)
    }
    #[cfg(target_os = "windows")]
    {
        install_windows(path)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        install_other(path)
    }
}

#[cfg(target_os = "macos")]
pub(super) fn install_macos(dmg: &Path) -> Result<Delivery, String> {
    if let Ok(exe) = std::env::current_exe()
        && let Some(app) = bundle_from_exe(&exe)
        && app
            .parent()
            .is_some_and(|dir| std::fs::write(dir.join(".navidog-update-probe"), b"").is_ok())
    {
        let _ = app.parent().map(|dir| {
            let _ = std::fs::remove_file(dir.join(".navidog-update-probe"));
        });
        if replace_bundle(&app, dmg).is_ok() {
            return Ok(Delivery::Replaced);
        }
    }
    std::process::Command::new("open")
        .arg(dmg)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(Delivery::HandedToOs)
}

/// `codesign` integrity check. Release CI can pin the expected Team ID in
/// `NAVIDOG_TEAM_ID`; without it an ad-hoc signature still has to be valid.
#[cfg(target_os = "macos")]
pub(super) fn verify_bundle(app: &Path) -> bool {
    let verified = std::process::Command::new("codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(app)
        .status()
        .is_ok_and(|status| status.success());
    if !verified {
        return false;
    }
    let Some(expected) = option_env!("NAVIDOG_TEAM_ID") else {
        return true;
    };
    let output = std::process::Command::new("codesign")
        .args(["-dv", "--verbose=4"])
        .arg(app)
        .output();
    output.is_ok_and(|out| {
        String::from_utf8_lossy(&out.stderr)
            .lines()
            .any(|line| line.trim() == format!("TeamIdentifier={expected}"))
    })
}

#[cfg(target_os = "macos")]
pub(super) fn replace_bundle(app: &Path, dmg: &Path) -> Result<(), String> {
    let mount = std::env::temp_dir().join(format!("navidog-dmg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&mount);
    std::fs::create_dir_all(&mount).map_err(|e| e.to_string())?;
    let attached = std::process::Command::new("hdiutil")
        .args(["attach", "-nobrowse", "-readonly", "-mountpoint"])
        .arg(&mount)
        .arg(dmg)
        .status()
        .map_err(|e| e.to_string())?;
    if !attached.success() {
        return Err("could not mount the disk image".into());
    }
    let src = std::fs::read_dir(&mount)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "app"));
    let Some(src) = src else {
        let _ = std::process::Command::new("hdiutil")
            .args(["detach"])
            .arg(&mount)
            .status();
        return Err("the disk image has no app".into());
    };
    let staged = std::env::temp_dir().join(format!("navidog-new-{}.app", std::process::id()));
    let _ = std::fs::remove_dir_all(&staged);
    let copied = std::process::Command::new("ditto")
        .arg(&src)
        .arg(&staged)
        .status();
    let _ = std::process::Command::new("hdiutil")
        .args(["detach"])
        .arg(&mount)
        .status();
    match copied {
        Ok(status) if status.success() && verify_bundle(&staged) => {}
        Ok(status) if status.success() => {
            let _ = std::fs::remove_dir_all(&staged);
            return Err("the downloaded app is not signed correctly".into());
        }
        _ => {
            let _ = std::fs::remove_dir_all(&staged);
            return Err("could not copy the new app into place".into());
        }
    }
    let aside = std::env::temp_dir().join(format!("navidog-old-{}.app", std::process::id()));
    let _ = std::fs::remove_dir_all(&aside);
    swap_app(app, &staged, &aside)
}

/// Moves `staged` onto `app`, keeping `aside` until that succeeds. A partial `app`
/// left by a failed copy is removed before the previous bundle is moved back.
#[cfg(any(test, target_os = "macos"))]
pub(super) fn swap_app(app: &Path, staged: &Path, aside: &Path) -> Result<(), String> {
    if let Err(e) = std::fs::rename(app, aside) {
        let _ = std::fs::remove_dir_all(staged);
        return Err(e.to_string());
    }
    if let Err(e) = std::fs::rename(staged, app) {
        restore_previous_app(app, aside)?;
        return Err(format!("could not copy the new app into place: {e}"));
    }
    if let Err(e) = std::fs::remove_dir_all(aside) {
        tracing::error!("could not remove the previous app: {e}");
    }
    Ok(())
}

#[cfg(any(test, target_os = "macos"))]
pub(super) fn restore_previous_app(app: &Path, aside: &Path) -> Result<(), String> {
    if app.exists() {
        std::fs::remove_dir_all(app).map_err(|e| {
            tracing::error!("could not remove the partial app: {e}");
            e.to_string()
        })?;
    }
    std::fs::rename(aside, app).map_err(|e| {
        tracing::error!("could not restore the previous app: {e}");
        format!("could not restore the previous app: {e}")
    })
}

/// Shell command that starts the new bundle after this process has exited.
/// `open` without waiting would only activate the process that is still running.
#[cfg(any(test, target_os = "macos"))]
pub(super) fn relaunch_command(pid: u32, app: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new("sh");
    cmd.arg("-c")
        .arg(
            "nohup sh -c 'while kill -0 \"$PID\" 2>/dev/null; do sleep 0.2; done; exec open \"$APP\"' >/dev/null 2>&1 &",
        )
        .env("PID", pid.to_string())
        .env("APP", app)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    cmd
}

#[cfg(target_os = "macos")]
pub fn relaunch() {
    if let Ok(exe) = std::env::current_exe()
        && let Some(app) = bundle_from_exe(&exe)
    {
        let _ = relaunch_command(std::process::id(), &app).spawn();
    }
}

#[cfg(target_os = "windows")]
pub(super) fn install_windows(path: &Path) -> Result<Delivery, String> {
    // One argument so cmd does not split a path that contains spaces.
    std::process::Command::new("cmd")
        .arg("/C")
        .arg(format!("start \"\" \"{}\"", path.display()))
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(Delivery::HandedToOs)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(super) fn install_other(path: &Path) -> Result<Delivery, String> {
    let open = if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".AppImage.tar.gz"))
    {
        extract_appimage(path)?
    } else {
        path.to_path_buf()
    };
    std::process::Command::new("xdg-open")
        .arg(&open)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(Delivery::HandedToOs)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(super) fn extract_appimage(archive: &Path) -> Result<PathBuf, String> {
    let dir = archive.with_extension("").with_extension("");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let status = std::process::Command::new("tar")
        .args(["-xzf"])
        .arg(archive)
        .arg("-C")
        .arg(&dir)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("could not unpack the AppImage".into());
    }
    let appimage = std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "AppImage"))
        .ok_or_else(|| "the archive has no AppImage".to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&appimage, std::fs::Permissions::from_mode(0o755));
    }
    Ok(appimage)
}
