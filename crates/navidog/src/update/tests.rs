use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use navidog_util::update::hex_encode;
use navidog_util::update_manifest::Asset as ManifestAsset;
use sha2::{Digest as _, Sha256};

use super::download::*;
use super::http::*;
use super::install::*;
use super::manifest::*;

use std::path::Path;

fn asset(os: &str, arch: &str, kind: &str, name: &str) -> ManifestAsset {
    ManifestAsset {
        os: os.into(),
        arch: arch.into(),
        kind: kind.into(),
        name: name.into(),
        url: format!("https://example.test/{name}"),
        sha256: "abc".into(),
        size: 0,
    }
}

#[test]
fn picks_the_preferred_kind_for_the_machine() {
    let assets = vec![
        asset("linux", "x86_64", "deb", "a.deb"),
        asset("linux", "x86_64", "appimage", "a.AppImage.tar.gz"),
        asset("linux", "aarch64", "appimage", "b.AppImage.tar.gz"),
    ];
    assert_eq!(
        pick_asset_for(&assets, "linux", "x86_64").unwrap().name,
        "a.AppImage.tar.gz"
    );
    assert!(pick_asset_for(&assets, "macos", "aarch64").is_none());
    assert_eq!(
        pick_asset_for(&assets, "linux", "x86_64").unwrap().url,
        "https://example.test/a.AppImage.tar.gz"
    );
}

#[test]
fn release_files_go_through_the_mirror() {
    assert_eq!(
        manifest_url(),
        "https://gh.nn.ci/xhofe/navidog/releases/download/nightly/latest.json"
    );
    assert_eq!(
        manifest_url_stable(),
        "https://gh.nn.ci/xhofe/navidog/releases/latest/download/latest.json"
    );
    let github = "https://github.com/xhofe/navidog/releases/download/v0.2.0/Navidog-aarch64.dmg";
    let mirrored = "https://gh.nn.ci/xhofe/navidog/releases/download/v0.2.0/Navidog-aarch64.dmg";
    assert_eq!(mirror_release_file(github), mirrored);
    assert_eq!(mirror_release_file(mirrored), mirrored);
}

#[test]
fn one_list_serves_both_channels() {
    const BUILT: &str = "2026-10-16T00:00:00Z";
    assert!(matches!(
        pick_release(&[], false, BUILT),
        Listed::Unpublished
    ));
    let list: Vec<UpdateRelease> = serde_json::from_str(
            r#"[
                {"tag":"v8.0.0","version":"8.0.0","draft":true,"published_at":null,"body":null},
                {"tag":"nightly","version":"nightly","prerelease":true,"published_at":"2099-01-02T00:00:00Z","built_at":"2099-01-02T00:00:00Z","body":"nightly notes"},
                {"tag":"v9.0.0","version":"9.0.0","published_at":"2099-01-01T00:00:00Z","body":"nine"},
                {"tag":"v9.1.0-rc.1","version":"9.1.0-rc.1","prerelease":true,"published_at":"2098-01-01T00:00:00Z","body":"rc"},
                {"tag":"v0.0.0","version":"0.0.0","published_at":"2020-01-01T00:00:00Z","body":"old"}
            ]"#,
        )
        .unwrap();
    assert!(list[0].body.is_empty());
    assert!(list[0].published_at.is_empty());
    assert!(list[0].built_at.is_empty());
    assert!(matches!(
        pick_release(&list[..1], false, BUILT),
        Listed::Unpublished
    ));
    match pick_release(&list, false, BUILT) {
        Listed::Chosen(release) => assert_eq!(release.tag, "v9.0.0"),
        other => panic!("stable channel should offer v9.0.0, got {other:?}"),
    }
    match pick_release(&list, true, BUILT) {
        Listed::Chosen(release) => assert_eq!(release.tag, "nightly"),
        other => panic!("pre-release channel should offer nightly, got {other:?}"),
    }
    let same_nightly: Vec<UpdateRelease> = serde_json::from_str(&format!(
            r#"[{{"tag":"nightly","version":"nightly","prerelease":true,"published_at":"2099-01-02T00:00:00Z","built_at":"{BUILT}"}}]"#
        ))
        .unwrap();
    assert!(
        matches!(pick_release(&same_nightly, true, BUILT), Listed::Current),
        "the nightly that built this binary is not a new version"
    );
    let older: Vec<UpdateRelease> =
        serde_json::from_str(r#"[{"tag":"v0.0.0","version":"0.0.0"}]"#).unwrap();
    assert!(matches!(
        pick_release(&older, false, BUILT),
        Listed::Current
    ));
    let rc: Vec<UpdateRelease> = serde_json::from_str(
            r#"[{"tag":"v9.1.0-rc.1","version":"9.1.0-rc.1","prerelease":true,"published_at":"2099-01-01T00:00:00Z"}]"#,
        )
        .unwrap();
    assert!(matches!(
        pick_release(&rc, false, BUILT),
        Listed::Unpublished
    ));
    match pick_release(&rc, true, BUILT) {
        Listed::Chosen(release) => assert_eq!(release.tag, "v9.1.0-rc.1"),
        other => panic!("pre-release channel should offer the rc, got {other:?}"),
    }
    assert!(matches!(
        missing_latest(Some(404), "http status: 404"),
        Ok(Availability::Unpublished)
    ));
    assert!(missing_latest(None, "connection failed").is_err());
}

#[test]
fn an_old_latest_json_is_one_release() {
    let manifest: Manifest = serde_json::from_str(
            r#"{"version":"9.0.0","tag":"v9.0.0","notes":"https://github.com/xhofe/navidog/releases/tag/v9.0.0","assets":[]}"#,
        )
        .unwrap();
    let releases = listed_releases(manifest);
    assert_eq!(releases.len(), 1);
    assert_eq!(releases[0].tag, "v9.0.0");
    assert!(matches!(
        pick_release(&releases, false, "1970-01-01T00:00:00Z"),
        Listed::Chosen(_)
    ));
}

#[test]
fn a_running_app_bundle_is_the_nearest_app_ancestor() {
    let path = Path::new("/Applications/Navidog.app/Contents/MacOS/navidog");
    assert_eq!(
        bundle_from_exe(path).unwrap(),
        PathBuf::from("/Applications/Navidog.app")
    );
    assert!(bundle_from_exe(Path::new("/usr/local/bin/navidog")).is_none());
}

#[test]
fn a_failed_check_is_not_recorded() {
    assert_eq!(checked_at(&Ok(Availability::Current), 10), Some(10));
    assert_eq!(checked_at(&Err("offline".into()), 10), None);
}

#[test]
fn nightly_label_names_the_channel_and_the_build_time() {
    let label = version_label("0.0.1", "nightly", "2026-10-17T18:05:00Z");
    assert_eq!(label, "nightly (2026-10-17 18:05 UTC)");
    assert!(!label.contains("0.0.1"));
    assert_eq!(
        version_label("0.2.0", "stable", "2026-10-17T18:05:00Z"),
        "0.2.0"
    );
}

#[test]
fn progress_repaints_when_the_percent_changes() {
    assert!(progress_changed(None, 0, 100));
    assert!(!progress_changed(Some((0, 100)), 0, 100));
    assert!(!progress_changed(Some((1, 1000)), 9, 1000));
    assert!(progress_changed(Some((9, 1000)), 10, 1000));
    assert!(progress_changed(Some((0, 0)), 1, 0));
    assert!(!progress_changed(Some((1, 0)), 1, 0));
}

#[test]
fn download_name_stays_inside_the_update_directory() {
    assert_eq!(download_file_name("../../etc/passwd"), Some("passwd"));
    assert_eq!(download_file_name("navidog.dmg"), Some("navidog.dmg"));
    assert_eq!(download_file_name(".."), None);
    assert_eq!(download_file_name(""), None);
    assert_eq!(download_file_name("C:payload.msi"), None);
    assert_eq!(download_file_name("nul\t.msi"), None);
}

fn serve(body: &'static [u8]) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        let mut buf = [0u8; 2048];
        let _ = std::io::Read::read(&mut sock, &mut buf);
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = std::io::Write::write_all(&mut sock, header.as_bytes());
        let _ = std::io::Write::write_all(&mut sock, body);
    });
    format!("http://{addr}/pkg.dmg")
}

#[test]
fn cancelling_a_download_removes_the_partial_file() {
    let body: &'static [u8] = b"navidog-update-cancel";
    let url = serve(body);
    let name = format!("navidog-cancel-{}.dmg", std::process::id());
    let asset = UpdateAsset {
        url,
        sha256: hex_encode(&Sha256::digest(body)),
        name: name.clone(),
        size: body.len() as u64,
    };
    let cancel = AtomicBool::new(false);
    let err = download_verified(&asset, &cancel, |done, _| {
        if done == 0 {
            cancel.store(true, Ordering::Relaxed);
        }
    });
    assert_eq!(err, Err(InstallError::Cancelled));
    let path = std::env::temp_dir().join("navidog-update").join(&name);
    assert!(!path.exists(), "{}", path.display());
}

#[test]
fn download_verified_checks_the_checksum_and_the_file_name() {
    let empty = UpdateAsset {
        url: "http://127.0.0.1/missing".into(),
        sha256: "  ".into(),
        name: "navidog.dmg".into(),
        size: 0,
    };
    assert_eq!(
        download_verified(&empty, &AtomicBool::new(false), |_, _| {}),
        Err(InstallError::Checksum)
    );

    let body: &'static [u8] = b"navidog-update";
    let good = hex_encode(&Sha256::digest(body));
    let url = serve(body);
    let wrong = UpdateAsset {
        url: url.clone(),
        sha256: "deadbeef".into(),
        name: format!("../../navidog-bad-{}.dmg", std::process::id()),
        size: 0,
    };
    assert_eq!(
        download_verified(&wrong, &AtomicBool::new(false), |_, _| {}),
        Err(InstallError::Checksum)
    );
    let bad_name = download_file_name(&wrong.name).unwrap();
    assert!(
        !std::env::temp_dir()
            .join("navidog-update")
            .join(bad_name)
            .exists()
    );

    let url = serve(body);
    let name = format!("../../navidog-ok-{}.dmg", std::process::id());
    let right = UpdateAsset {
        url,
        sha256: good,
        name: name.clone(),
        size: 0,
    };
    let mut ticks = Vec::new();
    let path = download_verified(&right, &AtomicBool::new(false), |done, total| {
        ticks.push((done, total));
    })
    .unwrap();
    assert!(
        ticks.windows(2).all(|pair| pair[0].0 <= pair[1].0),
        "{ticks:?}"
    );
    assert_eq!(ticks.first().copied(), Some((0, body.len() as u64)));
    assert_eq!(
        ticks.last().copied(),
        Some((body.len() as u64, body.len() as u64))
    );
    let file_name = download_file_name(&name).unwrap();
    assert_eq!(
        path,
        std::env::temp_dir().join("navidog-update").join(file_name)
    );
    assert!(!path.components().any(|c| c.as_os_str() == ".."));
    assert_eq!(std::fs::read(&path).unwrap(), body);
    cleanup_download(&path);
}

#[test]
fn relaunch_waits_until_this_process_exits() {
    let cmd = relaunch_command(42, Path::new("/Applications/Navidog.app"));
    let script = cmd
        .get_args()
        .nth(1)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(script.contains("kill -0"));
    assert!(script.contains("exec open"));
    assert!(!script.contains("-n"));
    let env: Vec<_> = cmd.get_envs().collect();
    assert!(env.iter().any(|(k, v)| *k == "PID" && v.unwrap() == "42"));
    assert!(
        env.iter()
            .any(|(k, v)| { *k == "APP" && v.unwrap() == "/Applications/Navidog.app" })
    );
}

#[test]
fn a_failed_swap_restores_the_previous_app() {
    let dir = std::env::temp_dir().join(format!("navidog-swap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let app = dir.join("Navidog.app");
    let staged = dir.join("Navidog.app.new");
    let aside = dir.join("aside.app");
    std::fs::create_dir(&app).unwrap();
    std::fs::write(app.join("marker"), b"old").unwrap();
    let err = swap_app(&app, &staged, &aside).unwrap_err();
    assert!(err.contains("could not copy"), "{err}");
    assert_eq!(std::fs::read(app.join("marker")).unwrap(), b"old");
    assert!(!aside.exists());

    std::fs::create_dir_all(&aside).unwrap();
    std::fs::write(aside.join("marker"), b"old").unwrap();
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(app.join("marker"), b"partial").unwrap();
    restore_previous_app(&app, &aside).unwrap();
    assert_eq!(std::fs::read(app.join("marker")).unwrap(), b"old");
    let _ = std::fs::remove_dir_all(&dir);
}
