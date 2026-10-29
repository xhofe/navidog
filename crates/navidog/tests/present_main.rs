//! Dock click and Window > Show Main Window open the main window again after the last one closes.

use gpui_kit::{AppContext as _, QuitMode, TestAppContext};

#[gpui_kit::test]
fn present_main_window_reopens_after_the_last_window_closes(cx: &mut TestAppContext) {
    let home = std::env::temp_dir().join(format!("navidog-present-main-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    // SAFETY: this test binary has a single test; nothing else reads the environment concurrently.
    unsafe { std::env::set_var("NAVIDOG_HOME", &home) };

    cx.update(|cx| {
        // Linux and Windows quit when the last window closes. Keep the process so the
        // reopen path can run on every platform.
        cx.set_quit_mode(QuitMode::Explicit);
        navidog::init(cx);
        assert!(cx.windows().is_empty());
        navidog::present_main_window(cx);
        assert_eq!(cx.windows().len(), 1);
        navidog::present_main_window(cx);
        assert_eq!(
            cx.windows().len(),
            1,
            "an open window is activated, not duplicated"
        );
    });

    let handle = cx.update(|cx| cx.windows()[0]);
    cx.update_window(handle, |_, window, _| window.remove_window())
        .unwrap();
    cx.update(|cx| {
        assert!(
            cx.windows().is_empty(),
            "closing the last window leaves the process with nothing to activate"
        );
        navidog::present_main_window(cx);
        assert_eq!(cx.windows().len(), 1);
    });

    let _ = std::fs::remove_dir_all(&home);
}
