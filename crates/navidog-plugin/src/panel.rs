//! Dock identity survives plugin deactivation; the retained panel renders a placeholder.
use crate::Plugins;
use gpui_kit::component::dock::{Panel, PanelEvent, PanelId, TabGroup};
use gpui_kit::{
    App, Context, EventEmitter, FocusHandle, Focusable, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, WeakEntity, Window, div,
};
use navidog_ui::{palette, skin::truncated, tabs};
use rust_i18n::t;

pub struct PluginPanel {
    id: String,
    title: SharedString,
    focus: FocusHandle,
}
impl PluginPanel {
    pub fn new(id: String, title: SharedString, cx: &mut Context<Self>) -> Self {
        cx.observe(&Plugins::global(cx), |_, _, cx| cx.notify())
            .detach();
        Self {
            id,
            title,
            focus: cx.focus_handle().tab_stop(true),
        }
    }
}
impl EventEmitter<PanelEvent> for PluginPanel {}
impl Focusable for PluginPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl gpui_kit::base::dock::Panel for PluginPanel {
    fn panel_name(&self) -> &'static str {
        "PluginPanel"
    }
    fn dump(&self, _: &App) -> gpui_kit::component::dock::PanelState {
        gpui_kit::component::dock::PanelState {
            panel_name: "PluginPanel".into(),
            children: vec![],
            info: gpui_kit::component::dock::PanelInfo::panel(
                serde_json::json!({"id":self.id,"title":self.title.to_string()}),
            ),
        }
    }
    fn on_added_to(&mut self, group: WeakEntity<TabGroup>, _: &mut Window, cx: &mut Context<Self>) {
        tabs::added(PanelId::from(cx.entity_id()), group, cx);
    }
    fn set_active(&mut self, active: bool, _: &mut Window, cx: &mut Context<Self>) {
        tabs::set_active(PanelId::from(cx.entity_id()), active, cx);
    }
    fn on_removed(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        tabs::removed(PanelId::from(cx.entity_id()), cx);
    }
}
impl Panel for PluginPanel {
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tabs::title(
            PanelId::from(cx.entity_id()),
            None,
            self.title.clone(),
            None,
            false,
        )
    }
}
impl Render for PluginPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = Plugins::global(cx).read(cx).panel(&self.id);
        let p = palette(cx);
        let mut view = div()
            .size_full()
            .min_w_0()
            .bg(p.control_bg)
            .text_color(p.label);
        if let Some(content) = content {
            view = view.child(content);
        } else {
            view = view.child(truncated(
                "plugin-unavailable",
                t!("plugin.unavailable").to_string(),
            ));
        }
        view
    }
}

pub fn init(cx: &mut App) {
    use gpui_kit::AppContext as _;
    gpui_kit::component::dock::register_panel(cx, "PluginPanel", |context, _, cx| {
        let value = match context.info() {
            gpui_kit::component::dock::PanelInfo::Panel(value) => value.clone(),
            _ => serde_json::Value::Null,
        };
        let id = value["id"].as_str().unwrap_or("unavailable").to_string();
        let title = value["title"]
            .as_str()
            .unwrap_or("Plugin")
            .to_string()
            .into();
        gpui_kit::component::dock::panel_handle(cx.new(|cx| PluginPanel::new(id, title, cx)))
    });
}
