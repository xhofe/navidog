//! Cross-feature plugin contributions and immutable invocation targets.

use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::{Action, App, EntityId, Global, SharedString, Window, WindowId};
use serde_json::Value;

use crate::registry::{Registry, Source};

#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = plugin, no_json)]
pub struct PluginCommand {
    pub id: SharedString,
}

#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct CommandContribution {
    pub title: SharedString,
    pub title_zh: Option<SharedString>,
    pub icon: Option<SharedString>,
    pub shortcut: Option<SharedString>,
    pub location: SharedString,
    pub when: Option<SharedString>,
}
impl CommandContribution {
    pub fn is_enabled(&self, window: WindowId, cx: &App) -> bool {
        match self.when.as_deref() {
            Some("grid") => cx
                .global::<ContextBridge>()
                .targets
                .get(&window)
                .is_some_and(|t| t.grid.is_some() && (t.alive)(cx)),
            Some("editor") => cx
                .global::<ContextBridge>()
                .targets
                .get(&window)
                .is_some_and(|t| t.editor.is_some() && (t.alive)(cx)),
            Some("object.selected") => matches!(
                crate::workspace::Workspace::global(cx).read(cx).selection(),
                crate::workspace::Selection::Object(_)
            ),
            Some("connection.open") => crate::workspace::Workspace::global(cx)
                .read(cx)
                .selection()
                .connection_id()
                .is_some_and(|id| cx.global::<Contributions>().open_connections.contains(id)),
            Some(_) => false,
            None => true,
        }
    }
    pub fn title(&self) -> SharedString {
        if &*rust_i18n::locale() == "zh-CN" {
            self.title_zh.clone().unwrap_or_else(|| self.title.clone())
        } else {
            self.title.clone()
        }
    }
}
#[derive(Default)]
pub struct Contributions {
    pub commands: Registry<CommandContribution>,
    pub formats: Registry<Value>,
    pub formatters: Registry<Value>,
    pub panels: Registry<Value>,
    pub open_connections: std::collections::HashSet<String>,
}
impl Global for Contributions {}
impl Contributions {
    pub fn remove(&mut self, source: &Source) {
        self.commands.unregister_source(source);
        self.formats.unregister_source(source);
        self.formatters.unregister_source(source);
        self.panels.unregister_source(source);
    }
    pub fn commands_at<'a>(
        &'a self,
        location: &'a str,
    ) -> impl Iterator<Item = (&'a SharedString, &'a CommandContribution)> {
        self.commands
            .iter()
            .filter(move |(_, _, c)| c.location == location)
            .map(|(id, _, c)| (id, c))
    }
}

type GridRead = Rc<dyn Fn(&App) -> Result<Value, String>>;
type EditorWrite = Rc<dyn Fn(String, &mut Window, &mut App) -> Result<(), String>>;
type TargetAlive = Rc<dyn Fn(&App) -> bool>;
#[derive(Clone)]
struct Target {
    panel: EntityId,
    alive: TargetAlive,
    grid: Option<GridRead>,
    editor: Option<EditorWrite>,
}
struct Invocation {
    plugin: String,
    generation: u64,
    window: WindowId,
    target: Option<Target>,
}
#[derive(Default)]
pub struct ContextBridge {
    targets: HashMap<WindowId, Target>,
    invocations: HashMap<String, Invocation>,
}
impl Global for ContextBridge {}
impl ContextBridge {
    pub fn set_grid(
        cx: &mut App,
        window: WindowId,
        panel: EntityId,
        alive: impl Fn(&App) -> bool + 'static,
        read: impl Fn(&App) -> Result<Value, String> + 'static,
    ) {
        cx.global_mut::<Self>().targets.insert(
            window,
            Target {
                panel,
                alive: Rc::new(alive),
                grid: Some(Rc::new(read)),
                editor: None,
            },
        );
    }
    pub fn set_editor(
        cx: &mut App,
        window: WindowId,
        panel: EntityId,
        alive: impl Fn(&App) -> bool + 'static,
        write: impl Fn(String, &mut Window, &mut App) -> Result<(), String> + 'static,
    ) {
        cx.global_mut::<Self>().targets.insert(
            window,
            Target {
                panel,
                alive: Rc::new(alive),
                grid: None,
                editor: Some(Rc::new(write)),
            },
        );
    }
    pub fn capture(cx: &mut App, window: WindowId, plugin: &str, generation: u64) -> String {
        let bridge = cx.global_mut::<Self>();
        if bridge.invocations.len() >= 4096 {
            bridge.invocations.clear();
        }
        let token = uuid::Uuid::new_v4().to_string();
        bridge.invocations.insert(
            token.clone(),
            Invocation {
                plugin: plugin.into(),
                generation,
                window,
                target: bridge.targets.get(&window).cloned(),
            },
        );
        token
    }
    pub fn invalidate(cx: &mut App, plugin: &str) {
        cx.global_mut::<Self>()
            .invocations
            .retain(|_, v| v.plugin != plugin);
    }
    pub fn retire(cx: &mut App, panel: EntityId) {
        let bridge = cx.global_mut::<Self>();
        bridge.targets.retain(|_, v| v.panel != panel);
        bridge
            .invocations
            .retain(|_, v| v.target.as_ref().is_none_or(|t| t.panel != panel));
    }
    fn invocation<'a>(
        cx: &'a App,
        token: &str,
        plugin: &str,
        generation: u64,
    ) -> Result<&'a Invocation, String> {
        let invocation = cx
            .global::<Self>()
            .invocations
            .get(token)
            .ok_or("Plugin context has expired.")?;
        if invocation.plugin != plugin
            || invocation.generation != generation
            || invocation.target.as_ref().is_some_and(|t| !(t.alive)(cx))
        {
            return Err("Plugin context has expired.".into());
        }
        Ok(invocation)
    }
    pub fn window(
        cx: &App,
        token: &str,
        plugin: &str,
        generation: u64,
    ) -> Result<WindowId, String> {
        Ok(Self::invocation(cx, token, plugin, generation)?.window)
    }
    pub fn grid(cx: &App, token: &str, plugin: &str, generation: u64) -> Result<Value, String> {
        let invocation = Self::invocation(cx, token, plugin, generation)?;
        let read = invocation
            .target
            .as_ref()
            .and_then(|t| t.grid.as_ref())
            .ok_or("Context has no result grid.")?;
        read(cx)
    }
    pub fn insert(
        cx: &mut App,
        window: &mut Window,
        token: &str,
        plugin: &str,
        generation: u64,
        text: String,
    ) -> Result<(), String> {
        let invocation = Self::invocation(cx, token, plugin, generation)?;
        if invocation.window != window.window_handle().window_id() {
            return Err("Context belongs to another window.".into());
        }
        let write = invocation
            .target
            .as_ref()
            .and_then(|t| t.editor.clone())
            .ok_or("Context has no query editor.")?;
        write(text, window, cx)
    }
}

pub fn init(cx: &mut App) {
    cx.set_global(Contributions::default());
    cx.set_global(ContextBridge::default());
}

/// A contribution uses the same Action on every native context menu.
pub fn context_menu(
    mut menu: gpui_kit::component::native_menu::NativeMenu,
    location: &str,
    cx: &App,
) -> gpui_kit::component::native_menu::NativeMenu {
    if let Some(contributions) = cx.try_global::<Contributions>() {
        for (id, command) in contributions.commands_at(location) {
            let enabled = cx
                .active_window()
                .is_some_and(|window| command.is_enabled(window.window_id(), cx));
            let action = Box::new(PluginCommand { id: id.clone() });
            menu = if let Some(icon) = &command.icon {
                menu.menu_with_icon_disabled(
                    command.title().to_string(),
                    gpui_kit::component::Icon::default().path(icon.clone()),
                    !enabled,
                    action,
                )
            } else {
                menu.menu_with_disabled(command.title().to_string(), !enabled, action)
            };
        }
    }
    menu
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{AppContext as _, TestAppContext, WindowOptions};
    #[gpui_kit::test]
    fn context_keeps_original_window_and_rejects_closed_targets_and_reload(
        cx: &mut TestAppContext,
    ) {
        cx.update(init);
        let (first, target) = cx
            .update(|cx| {
                gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Empty))
            })
            .unwrap();
        let (second, _) = cx
            .update(|cx| {
                gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Empty))
            })
            .unwrap();
        let weak = target.downgrade();
        let token = cx.update(|cx| {
            ContextBridge::set_grid(
                cx,
                first.window_id(),
                target.entity_id(),
                move |_| weak.upgrade().is_some(),
                |_| Ok(serde_json::json!({"rows":[["original"]]})),
            );
            ContextBridge::capture(cx, first.window_id(), "plugin", 1)
        });
        cx.update(|cx| {
            ContextBridge::set_grid(
                cx,
                second.window_id(),
                target.entity_id(),
                |_| true,
                |_| Ok(serde_json::json!({"rows":[["other"]]})),
            )
        });
        cx.read(|cx| {
            assert_eq!(
                ContextBridge::grid(cx, &token, "plugin", 1).unwrap()["rows"][0][0],
                "original"
            )
        });
        cx.update(|cx| ContextBridge::retire(cx, target.entity_id()));
        cx.read(|cx| assert!(ContextBridge::grid(cx, &token, "plugin", 1).is_err()));
        let token = cx.update(|cx| ContextBridge::capture(cx, second.window_id(), "plugin", 2));
        cx.update(|cx| ContextBridge::invalidate(cx, "plugin"));
        cx.read(|cx| assert!(ContextBridge::window(cx, &token, "plugin", 2).is_err()));
    }
    struct Empty;
    impl gpui_kit::Render for Empty {
        fn render(
            &mut self,
            _: &mut Window,
            _: &mut gpui_kit::Context<Self>,
        ) -> impl gpui_kit::IntoElement {
            gpui_kit::div()
        }
    }
}
