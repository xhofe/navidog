use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, IntoElement, ParentElement as _, Render, Styled as _, Window,
};
use navidog_app_state::settings::Settings;
use navidog_ui::tokens;
use navidog_util::update::notes_preview;
use rust_i18n::t;

use super::download::*;
use super::install::*;
use super::manifest::*;

pub fn startup_check(window: &mut Window, cx: &mut App) {
    if is_smoke_test() || !Settings::global(cx).update_check_due(unix_now()) {
        return;
    }
    begin_check(false, window, cx);
}

/// Menu item: always checks, and reports up to date, no release, and errors.
pub fn manual_check(window: &mut Window, cx: &mut App) {
    begin_check(true, window, cx);
}

fn begin_check(manual: bool, window: &mut Window, cx: &mut App) {
    let include = Settings::global(cx).include_prerelease(is_nightly_build());
    // Keep this window. The menu callback often runs while `NSApp.mainWindow` is nil,
    // so looking up the active window again after the request would drop the result.
    let home = window.window_handle();
    cx.spawn(async move |cx| {
        let found = cx
            .background_spawn(async move { fetch_latest_release(include) })
            .await;
        cx.update(|cx| {
            let _ = home.update(cx, |_, window, cx| show_check(found, manual, window, cx));
        });
    })
    .detach();
}

fn show_check(
    found: Result<Availability, String>,
    manual: bool,
    window: &mut Window,
    cx: &mut App,
) {
    if let Some(at) = checked_at(&found, unix_now()) {
        Settings::update(cx, |s| s.update_checked_unix = Some(at));
    }
    match found {
        Ok(Availability::Available(info)) => open_dialog(info, window, cx),
        Ok(Availability::Current) if manual => {
            window.push_notification(t!("update.uptodate").to_string(), cx);
        }
        Ok(Availability::Unpublished) if manual => {
            window.push_notification(t!("update.unpublished").to_string(), cx);
        }
        Err(detail) if manual => {
            window.push_notification(t!("update.failed", detail = detail).to_string(), cx);
        }
        _ => {}
    }
}

fn open_dialog(info: UpdateInfo, window: &mut Window, cx: &mut App) {
    let view = cx.new(|_| UpdateDialog {
        info,
        progress: None,
        installing: false,
        phase: None,
        cancel: Arc::new(AtomicBool::new(false)),
    });
    let version = view.read(cx).info.version.clone();
    window.open_dialog(cx, move |dialog, window, _| {
        dialog
            .w(tokens::dialog::SMALL_WIDTH.to_pixels(window.rem_size()))
            .title(t!("update.available", version = version).to_string())
            .child(view.clone())
    });
}

struct UpdateDialog {
    info: UpdateInfo,
    /// `(downloaded, total)` while a download is in flight. `total` is 0 when
    /// the size is unknown. `None` shows the action buttons.
    progress: Option<(u64, u64)>,
    /// The file is on disk and install has started. Cancel no longer applies.
    installing: bool,
    /// The install step currently running. `None` until the installer reports one.
    phase: Option<InstallPhase>,
    cancel: Arc<AtomicBool>,
}

/// Applies download ticks to the dialog. Percentage-sized steps only, so a
/// 64 KiB chunk does not repaint on its own.
async fn watch_download(
    view: gpui_kit::WeakEntity<UpdateDialog>,
    rx: std::sync::mpsc::Receiver<(u64, u64)>,
    cx: &mut gpui_kit::AsyncWindowContext,
) {
    let mut shown: Option<(u64, u64)> = None;
    loop {
        let mut latest = None;
        let mut closed = false;
        loop {
            match rx.try_recv() {
                Ok(tick) => latest = Some(tick),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    closed = true;
                    break;
                }
            }
        }
        if let Some((done, total)) = latest
            && progress_changed(shown, done, total)
        {
            shown = Some((done, total));
            let _ = view.update_in(cx, |dialog, _, cx| {
                if dialog.installing {
                    return;
                }
                dialog.progress = Some((done, total));
                cx.notify();
            });
        }
        if closed {
            break;
        }
        cx.background_executor()
            .timer(Duration::from_millis(100))
            .await;
    }
}

fn download_progress_bar(done: u64, total: u64) -> impl IntoElement {
    let bar = if total == 0 {
        Progress::new("update-progress").w_full().loading(true)
    } else {
        Progress::new("update-progress")
            .w_full()
            .value(download_percent(done, total) as f32)
    };
    let caption = if total == 0 {
        t!("update.downloading").to_string()
    } else {
        t!(
            "update.downloading_progress",
            percent = download_percent(done, total),
            done = format_bytes(done),
            total = format_bytes(total)
        )
        .to_string()
    };
    v_flex().w_full().gap_2().child(bar).child(caption)
}

fn install_progress(phase: Option<InstallPhase>) -> impl IntoElement {
    let caption = match phase {
        Some(InstallPhase::Mounting) => t!("update.mounting"),
        Some(InstallPhase::Copying) => t!("update.copying"),
        Some(InstallPhase::Verifying) => t!("update.verifying"),
        Some(InstallPhase::Replacing) => t!("update.replacing"),
        Some(InstallPhase::Opening) => t!("update.opening"),
        None => t!("update.installing"),
    };
    v_flex()
        .w_full()
        .gap_2()
        .child(Progress::new("update-progress").w_full().loading(true))
        .child(caption.to_string())
}

/// Applies install steps to the dialog. The bar stays indeterminate.
async fn watch_install(
    view: gpui_kit::WeakEntity<UpdateDialog>,
    rx: std::sync::mpsc::Receiver<InstallPhase>,
    cx: &mut gpui_kit::AsyncWindowContext,
) {
    let mut shown = None;
    loop {
        let mut latest = None;
        let mut closed = false;
        loop {
            match rx.try_recv() {
                Ok(phase) => latest = Some(phase),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    closed = true;
                    break;
                }
            }
        }
        if let Some(phase) = latest
            && shown != Some(phase)
        {
            shown = Some(phase);
            let _ = view.update_in(cx, |dialog, _, cx| {
                dialog.installing = true;
                dialog.phase = Some(phase);
                cx.notify();
            });
        }
        if closed {
            break;
        }
        cx.background_executor()
            .timer(Duration::from_millis(100))
            .await;
    }
}

impl UpdateDialog {
    fn download(&mut self, window: &mut Window, cx: &mut gpui_kit::Context<Self>) {
        if self.progress.is_some() {
            return;
        }
        let Some(asset) = self.info.asset.clone() else {
            cx.open_url(&self.info.page_url);
            window.close_dialog(cx);
            return;
        };
        self.cancel.store(false, Ordering::Relaxed);
        self.installing = false;
        self.phase = None;
        // Show the bar on the click itself. The first byte waits on DNS and
        // TLS, and a still-enabled button looks like the click did nothing.
        self.progress = Some((0, asset.size));
        cx.notify();
        let page = self.info.page_url.clone();
        let cancel = Arc::clone(&self.cancel);
        let (tx, rx) = std::sync::mpsc::channel();
        cx.spawn_in(window, async move |this, cx| {
            let view = this.clone();
            let watch = cx.spawn(async move |cx| watch_download(view, rx, cx).await);
            let downloaded = cx
                .background_spawn(async move {
                    download_verified(&asset, &cancel, |done, total| {
                        let _ = tx.send((done, total));
                    })
                })
                .await;
            let result = match downloaded {
                Err(err) => Err(err),
                Ok(path) => {
                    let cancelled = this
                        .update_in(cx, |dialog, _, _| dialog.cancel.load(Ordering::Relaxed))
                        .unwrap_or(false);
                    if cancelled {
                        cleanup_download(&path);
                        Err(InstallError::Cancelled)
                    } else {
                        let (phase_tx, phase_rx) = std::sync::mpsc::channel();
                        let view = this.clone();
                        let watch_phase =
                            cx.spawn(async move |cx| watch_install(view, phase_rx, cx).await);
                        let _ = this.update_in(cx, |dialog, _, cx| {
                            dialog.installing = true;
                            dialog.phase = None;
                            cx.notify();
                        });
                        let installed = cx
                            .background_spawn(async move {
                                match install(&path, &|phase| {
                                    let _ = phase_tx.send(phase);
                                }) {
                                    Ok(delivery) => {
                                        #[cfg(target_os = "macos")]
                                        if matches!(delivery, Delivery::Replaced) {
                                            cleanup_download(&path);
                                        }
                                        Ok(delivery)
                                    }
                                    Err(detail) => {
                                        cleanup_download(&path);
                                        Err(InstallError::Other(detail))
                                    }
                                }
                            })
                            .await;
                        watch_phase.await;
                        installed
                    }
                }
            };
            watch.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.progress = None;
                this.installing = false;
                this.phase = None;
                match result {
                    #[cfg(target_os = "macos")]
                    Ok(Delivery::Replaced) => {
                        window.close_dialog(cx);
                        open_restart(window, cx);
                    }
                    Ok(Delivery::HandedToOs) => {
                        window.push_notification(t!("update.handed_off").to_string(), cx);
                        window.close_dialog(cx);
                    }
                    Err(InstallError::Cancelled) => {}
                    Err(InstallError::Checksum) => {
                        window.push_notification(t!("update.bad_checksum").to_string(), cx);
                        cx.open_url(&page);
                    }
                    Err(InstallError::Other(detail)) => {
                        window.push_notification(
                            t!("update.install_failed", detail = detail).to_string(),
                            cx,
                        );
                    }
                }
            });
        })
        .detach();
    }
}

impl Render for UpdateDialog {
    fn render(&mut self, _: &mut Window, cx: &mut gpui_kit::Context<Self>) -> impl IntoElement {
        let notes = notes_preview(&self.info.notes);
        let has_asset = self.info.asset.is_some();
        let page = self.info.page_url.clone();
        let weak = cx.entity().downgrade();
        let progress = self.progress;
        let installing = self.installing;
        let phase = self.phase;
        let cancel = Arc::clone(&self.cancel);
        v_flex()
            .gap_2()
            .w_full()
            .min_w_0()
            .child(t!("update.current", version = self.info.current.clone()).to_string())
            .when(!notes.is_empty(), |col| col.child(notes))
            .when(progress.is_none() && !has_asset && !installing, |col| {
                col.child(t!("update.no_asset").to_string())
            })
            .when(installing, |col| col.child(install_progress(phase)))
            .when(!installing, |col| {
                col.when_some(progress, |col, (done, total)| {
                    col.child(download_progress_bar(done, total)).child(
                        h_flex().justify_end().w_full().child(
                            Button::new("update-cancel")
                                .label(t!("dialog.cancel").to_string())
                                .on_click(move |_, _, _| {
                                    cancel.store(true, Ordering::Relaxed);
                                }),
                        ),
                    )
                })
            })
            .when(progress.is_none(), |col| {
                col.child(
                    h_flex()
                        .justify_end()
                        .gap_2()
                        .w_full()
                        .child(
                            Button::new("update-later")
                                .label(t!("update.later").to_string())
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("update-go")
                                .primary()
                                .label(
                                    if has_asset {
                                        t!("update.download")
                                    } else {
                                        t!("update.open_page")
                                    }
                                    .to_string(),
                                )
                                .on_click(move |_, window, cx| {
                                    if has_asset {
                                        let _ =
                                            weak.update(cx, |this, cx| this.download(window, cx));
                                    } else {
                                        cx.open_url(&page);
                                        window.close_dialog(cx);
                                    }
                                }),
                        ),
                )
            })
    }
}

#[cfg(target_os = "macos")]
fn open_restart(window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, |dialog, window, _| {
        dialog
            .w(tokens::dialog::SMALL_WIDTH.to_pixels(window.rem_size()))
            .title(t!("update.restart_body").to_string())
            .footer(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("restart-later")
                            .label(t!("update.restart_later").to_string())
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("restart-now")
                            .primary()
                            .label(t!("update.restart_now").to_string())
                            .on_click(|_, _, cx| {
                                #[cfg(target_os = "macos")]
                                relaunch();
                                cx.quit();
                            }),
                    ),
            )
    });
}
