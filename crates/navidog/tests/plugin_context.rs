//! Delayed plugin edits stay on the query editor that originally issued the command.
use gpui_kit::component::dock::BasePanel as _;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, TestAppContext, WindowOptions};
use navidog_app_state::plugin::ContextBridge;
use navidog_query::{QueryEditor, QuerySource, Target};
#[gpui_kit::test]
fn delayed_plugin_insert_keeps_its_original_window_and_editor(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    // SAFETY: this integration test binary has one test before app startup.
    unsafe {
        std::env::set_var("NAVIDOG_HOME", directory.path());
    }
    cx.update(navidog::init);
    let (first, first_editor) = cx
        .update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                QueryEditor::new(
                    QuerySource::Untitled(1),
                    Target::default(),
                    "first".into(),
                    window,
                    cx,
                )
            })
        })
        .unwrap();
    cx.update_window(first, |_, window, cx| {
        window.render_frame(cx);
        first_editor.update(cx, |editor, cx| editor.set_active(true, window, cx));
    })
    .unwrap();
    let token = cx.update(|cx| ContextBridge::capture(cx, first.window_id(), "plugin", 1));
    let (second, second_editor) = cx
        .update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                QueryEditor::new(
                    QuerySource::Untitled(2),
                    Target::default(),
                    "second".into(),
                    window,
                    cx,
                )
            })
        })
        .unwrap();
    cx.update_window(second, |_, window, cx| {
        window.render_frame(cx);
        second_editor.update(cx, |editor, cx| editor.set_active(true, window, cx));
    })
    .unwrap();
    cx.update_window(first, |_, window, cx| {
        ContextBridge::insert(cx, window, &token, "plugin", 1, "-- plugin".into()).unwrap()
    })
    .unwrap();
    cx.read(|cx| {
        assert!(first_editor.read(cx).text(cx).contains("-- plugin"));
        assert_eq!(second_editor.read(cx).text(cx), "second");
    });
    cx.update_window(first, |_, window, cx| {
        first_editor.update(cx, |editor, cx| editor.on_removed(window, cx))
    })
    .unwrap();
    cx.read(|cx| assert!(ContextBridge::window(cx, &token, "plugin", 1).is_err()));
}
