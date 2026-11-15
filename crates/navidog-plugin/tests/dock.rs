//! A saved plugin panel can be restored while disabled and recover in the same place.
use gpui_kit::component::dock::{DockArea, DockLayout, DockSkin, panel_handle};
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, TestAppContext,
    Window, WindowOptions,
};
use navidog_plugin::{Plugins, panel::PluginPanel};
struct Dock {
    area: Entity<DockArea>,
    _skin: std::rc::Rc<DockSkin>,
}
impl Render for Dock {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui_kit::div().child(self.area.clone())
    }
}
#[gpui_kit::test]
fn dock_layout_restores_disabled_panel_identity(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    // SAFETY: isolated one-test integration binary before initialization.
    unsafe {
        std::env::set_var("NAVIDOG_HOME", directory.path());
    }
    cx.update(|cx| {
        gpui_kit::init(cx);
        navidog_app_state::init(cx);
        navidog_session::init(cx);
        navidog_ui::tabs::init(cx);
        navidog_plugin::init(cx);
    });
    let (first, view) = cx
        .update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| {
                    let (area, skin) = DockSkin::dock_area("plugin-dock", Some(1), window, cx);
                    let panel =
                        cx.new(|cx| PluginPanel::new("missing/help".into(), "Help".into(), cx));
                    area.update(cx, |area, cx| {
                        area.set_center(
                            DockLayout::tabs().panel_view(panel_handle(panel), cx),
                            window,
                            cx,
                        )
                    });
                    Dock { area, _skin: skin }
                })
            })
        })
        .unwrap();
    cx.update_window(first, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let saved = cx.read(|cx| view.read(cx).area.read(cx).dump(cx));
    let (second, restored) = cx
        .update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| {
                    let (area, skin) =
                        DockSkin::dock_area("restored-plugin-dock", Some(1), window, cx);
                    area.update(cx, |area, cx| area.load(saved.clone(), window, cx).unwrap());
                    Dock { area, _skin: skin }
                })
            })
        })
        .unwrap();
    cx.update_window(second, |_, window, cx| window.render_frame(cx))
        .unwrap();
    cx.read(|cx| {
        assert!(PluginPanel::open("missing/help", cx).is_some());
        assert_eq!(
            serde_json::to_value(restored.read(cx).area.read(cx).dump(cx)).unwrap(),
            serde_json::to_value(saved).unwrap()
        );
        assert!(Plugins::global(cx).read(cx).panel("missing/help").is_none());
    });
}
