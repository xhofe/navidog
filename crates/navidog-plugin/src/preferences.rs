//! The plugin page owns selection and approval drafts; Plugins owns committed lifecycle state.

use std::collections::BTreeSet;

use gpui_kit::component::{
    Disableable as _, Selectable as _, WindowExt as _, button::Button, checkbox::Checkbox, h_flex,
    v_flex,
};
use gpui_kit::{
    AppContext as _, Context, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Window,
};
use navidog_ui::{palette, skin::truncated as truncate_text, tokens};
use rust_i18n::t;

use crate::{Plugins, settings::Approval};

pub struct PluginPreferences {
    selected: Option<String>,
    error: Option<String>,
}
impl PluginPreferences {
    pub fn new(cx: &mut Context<Self>) -> Self {
        cx.observe(&Plugins::global(cx), |_, _, cx| cx.notify())
            .detach();
        Self {
            selected: None,
            error: None,
        }
    }
    fn approval(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let plugins = Plugins::global(cx);
        let plugins_read = plugins.read(cx);
        let Some(plugin) = plugins_read
            .installations()
            .iter()
            .find(|p| p.id() == id)
            .and_then(|p| p.plugin().ok())
        else {
            return;
        };
        let requested = plugin.requested();
        let saved = plugins_read.approval(&id);
        let granted = if saved.fingerprint.is_empty() {
            requested.clone()
        } else {
            saved.granted.clone()
        };
        let fingerprint = plugin.fingerprint().to_owned();
        let view = cx.new(|_| ApprovalView {
            id,
            requested,
            granted,
            fingerprint,
            limits: saved.limits,
            error: None,
        });
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title(t!("plugin.permissions").to_string())
                .child(view.clone())
        });
    }
}
impl Render for PluginPreferences {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let plugins = Plugins::global(cx);
        let plugins = plugins.read(cx);
        let mut rows = Vec::new();
        for installation in plugins.installations() {
            let id = installation.id().to_owned();
            let name = installation
                .plugin()
                .map(|p| p.shell().name())
                .unwrap_or(installation.id())
                .to_owned();
            let version = installation
                .plugin()
                .map(|p| p.shell().version())
                .unwrap_or("")
                .to_owned();
            let status = if installation.is_enabled() {
                t!("plugin.enabled")
            } else if installation.failure().is_some() {
                t!("plugin.failed")
            } else {
                t!("plugin.disabled")
            };
            let selected = self.selected.as_deref() == Some(&id);
            rows.push(
                Button::new(SharedString::from(format!("plugin-select-{id}")))
                    .label(format!("{name} {version} · {status}"))
                    .selected(selected)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected = Some(id.clone());
                        this.error = None;
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        let mut detail = v_flex()
            .min_w_0()
            .flex_1()
            .gap(tokens::form::ROW_GAP.rems());
        if let Some(installation) = self
            .selected
            .as_ref()
            .and_then(|id| plugins.installations().iter().find(|p| p.id() == id))
        {
            let id = installation.id().to_owned();
            detail = detail.child(truncated(id.clone())).child(truncated(
                installation.directory().to_string_lossy().to_string(),
            ));
            if let Some(failure) = installation.failure() {
                detail = detail.child(truncated(failure.to_owned()));
            }
            let enabled = installation.is_enabled();
            let valid = installation.plugin().is_ok();
            let toggle = id.clone();
            let approve = id.clone();
            let reload = id;
            detail = detail.child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("plugin-toggle")
                            .label(
                                if enabled {
                                    t!("plugin.disable")
                                } else {
                                    t!("plugin.enable")
                                }
                                .to_string(),
                            )
                            .disabled(!valid)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if enabled {
                                    let result = Plugins::global(cx)
                                        .update(cx, |p, cx| p.disable(&toggle, cx));
                                    this.error = result.err();
                                    cx.notify();
                                } else {
                                    this.approval(toggle.clone(), window, cx);
                                }
                            })),
                    )
                    .child(
                        Button::new("plugin-authorize")
                            .label(t!("plugin.permissions_action").to_string())
                            .disabled(!valid)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.approval(approve.clone(), window, cx)
                            })),
                    )
                    .child(
                        Button::new("plugin-reload")
                            .label(t!("plugin.reload").to_string())
                            .disabled(!enabled)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.error = Plugins::global(cx)
                                    .update(cx, |p, cx| p.reload(&reload, window, cx))
                                    .err();
                                cx.notify();
                            })),
                    ),
            );
        } else {
            detail = detail.child(t!("plugin.select").to_string());
        }
        if let Some(id) = self.selected.clone() {
            let limits = plugins.approval(&id).limits;
            detail = detail.child(
                Button::new("plugin-limits")
                    .label(t!("plugin.limits").to_string())
                    .on_click(cx.listener(move |_, _, window, cx| {
                        let view = cx.new(|cx| {
                            crate::limits_view::LimitsView::new(
                                id.clone(),
                                limits.clone(),
                                window,
                                cx,
                            )
                        });
                        window.open_dialog(cx, move |dialog, _, _| {
                            dialog
                                .title(t!("plugin.limits_title").to_string())
                                .child(view.clone())
                        });
                    })),
            );
        }
        if let Some(error) = self.error.as_deref().or_else(|| plugins.failure()) {
            detail = detail.child(truncated(error.to_owned()));
        }
        let empty = rows.is_empty();
        v_flex()
            .size_full()
            .gap_3()
            .text_color(p.label)
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("plugin-scan")
                            .label(t!("plugin.scan").to_string())
                            .on_click(|_, _, cx| {
                                Plugins::global(cx).update(cx, |p, cx| p.scan(cx))
                            }),
                    )
                    .child(truncated(plugins.root().to_string_lossy().to_string())),
            )
            .child(if empty {
                v_flex()
                    .child(t!("plugin.empty").to_string())
                    .into_any_element()
            } else {
                h_flex()
                    .items_stretch()
                    .flex_1()
                    .min_h_0()
                    .gap_3()
                    .child(
                        v_flex()
                            .w(tokens::preferences::NAV_WIDTH.rems())
                            .gap_1()
                            .children(rows),
                    )
                    .child(detail)
                    .into_any_element()
            })
    }
}
struct ApprovalView {
    id: String,
    requested: BTreeSet<String>,
    granted: BTreeSet<String>,
    fingerprint: String,
    limits: crate::settings::Limits,
    error: Option<String>,
}
impl Render for ApprovalView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let grants: Vec<_> = self.requested.iter().cloned().collect();
        let mut content = v_flex().gap_2().min_w_0().child(truncated(self.id.clone()));
        for grant in grants {
            let checked = self.granted.contains(&grant);
            let label = grant.clone();
            content = content.child(
                Checkbox::new(SharedString::from(format!("grant-{grant}")))
                    .label(label)
                    .checked(checked)
                    .on_change(cx.listener(move |this, next, _, cx| {
                        if *next {
                            this.granted.insert(grant.clone());
                        } else {
                            this.granted.remove(&grant);
                        }
                        cx.notify();
                    })),
            );
        }
        if let Some(error) = &self.error {
            content = content.child(truncated(error.to_owned()));
        }
        content.child(
            Button::new("plugin-confirm")
                .label(t!("plugin.confirm").to_string())
                .on_click(cx.listener(|this, _, window, cx| {
                    let approval = Approval::confirm(
                        this.requested.clone(),
                        this.granted.clone(),
                        this.fingerprint.clone(),
                        this.limits.clone(),
                    );
                    let result = approval.and_then(|approval| {
                        Plugins::global(cx)
                            .update(cx, |p, cx| p.enable(&this.id, approval, window, cx))
                    });
                    match result {
                        Ok(()) => window.close_dialog(cx),
                        Err(error) => {
                            this.error = Some(error);
                            cx.notify();
                        }
                    }
                })),
        )
    }
}

pub fn title() -> SharedString {
    t!("plugin.title").into()
}

fn truncated(text: impl Into<SharedString>) -> impl IntoElement {
    truncate_text("plugin-value", text)
}
