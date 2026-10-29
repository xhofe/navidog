//! Dock click and Window > Show Main Window target the tracked main window, not
//! whichever window happens to be in front or in the middle of its own update.

use gpui_kit::{
    AppContext as _, Context, IntoElement, QuitMode, Render, TestAppContext, Window, WindowOptions,
    div,
};

/// Root view for the extra window these scenarios open next to the main one.
struct Blank;

impl Render for Blank {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

// One test: `navidog::init` extends the i18n backend, which a process may do once.
#[gpui_kit::test]
fn present_main_window_targets_the_main_window_next_to_other_windows(cx: &mut TestAppContext) {
    let home =
        std::env::temp_dir().join(format!("navidog-present-main-other-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    // SAFETY: this test binary has a single test; nothing else reads the environment concurrently.
    unsafe { std::env::set_var("NAVIDOG_HOME", &home) };

    let main = cx.update(|cx| {
        // Linux and Windows quit when the last window closes. Keep the process so the
        // reopen path can run on every platform.
        cx.set_quit_mode(QuitMode::Explicit);
        navidog::init(cx);
        navidog::present_main_window(cx);
        cx.windows()[0]
    });

    // Preferences, a popped-out tab or the connection manager: any window that is
    // not the main one.
    let other = cx.update(|cx| {
        let (handle, _) =
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Blank)).unwrap();
        handle
    });

    // The main window closes while the other window is in front. A Dock click has
    // to open a new main window next to it instead of only raising "other".
    cx.update_window(other, |_, window, _| window.activate_window())
        .unwrap();
    cx.update_window(main, |_, window, _| window.remove_window())
        .unwrap();
    let main = cx.update(|cx| {
        assert_eq!(cx.windows(), vec![other]);
        navidog::present_main_window(cx);
        assert_eq!(
            cx.windows().len(),
            2,
            "the main window is reopened next to the window that was in front"
        );
        cx.windows().into_iter().find(|w| *w != other).unwrap()
    });

    // Window > Show Main Window from another key window raises the main window and
    // reuses it.
    cx.update_window(other, |_, window, cx| {
        window.activate_window();
        navidog::present_main_window(cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert_eq!(cx.windows().len(), 2, "no second main window");
        assert_eq!(cx.active_window(), Some(main));
    });

    // The same command from the main window itself: gpui runs menu commands inside
    // the key window's update, where that window's slot is empty until it returns.
    cx.update_window(main, |_, window, cx| {
        window.activate_window();
        navidog::present_main_window(cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert_eq!(cx.windows().len(), 2, "no second main window");
        assert_eq!(
            cx.active_window(),
            Some(main),
            "the main window is activated once its update returns"
        );
    });

    let _ = std::fs::remove_dir_all(&home);
}
