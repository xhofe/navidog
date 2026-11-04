//! Limits remain a draft until validated persistence and runtime reload succeed.
use crate::{Plugins, settings::Limits};
use gpui_kit::component::{
    WindowExt as _,
    button::Button,
    input::{Input, InputState},
    v_flex,
};
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _, Window,
};
use navidog_ui::skin::FormRow;
use rust_i18n::t;

pub struct LimitsView {
    id: String,
    inputs: Vec<Entity<InputState>>,
    error: Option<String>,
}
impl LimitsView {
    pub fn new(id: String, limits: Limits, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let values = [
            limits.timeout_seconds.to_string(),
            limits.events.to_string(),
            limits.requests.to_string(),
            limits.batch_rows.to_string(),
            limits.batch_bytes.to_string(),
            limits.result_rows.to_string(),
            limits.result_bytes.to_string(),
        ];
        Self {
            id,
            inputs: values
                .into_iter()
                .map(|value| cx.new(|cx| InputState::new(window, cx).default_value(value)))
                .collect(),
            error: None,
        }
    }
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        let values: Vec<_> = self
            .inputs
            .iter()
            .map(|input| input.read(cx).value().to_string())
            .collect();
        let positive = |ix: usize| {
            values[ix]
                .parse::<usize>()
                .map_err(|_| "Limits must be positive integers.".to_owned())
        };
        let limits = Limits {
            timeout_seconds: values[0].parse().map_err(|_| "Invalid request timeout.")?,
            events: positive(1)?,
            requests: positive(2)?,
            batch_rows: positive(3)?,
            batch_bytes: positive(4)?,
            result_rows: positive(5)?,
            result_bytes: positive(6)?,
        };
        limits.validate()?;
        let plugins = Plugins::global(cx);
        let mut approval = plugins.read(cx).approval(&self.id);
        approval.limits = limits;
        plugins.update(cx, |plugins, cx| {
            plugins.enable(&self.id, approval, window, cx)
        })
    }
}
impl Render for LimitsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let labels = [
            t!("plugin.timeout"),
            t!("plugin.event_limit"),
            t!("plugin.request_limit"),
            t!("plugin.batch_rows"),
            t!("plugin.batch_bytes"),
            t!("plugin.result_rows"),
            t!("plugin.result_bytes"),
        ];
        let mut content =
            v_flex()
                .gap_2()
                .children(labels.into_iter().zip(&self.inputs).enumerate().map(
                    |(ix, (label, input))| {
                        FormRow::new(
                            label.to_string(),
                            Input::new(input).id(format!("plugin-limit-{ix}")),
                        )
                    },
                ));
        if let Some(error) = &self.error {
            content = content.child(error.clone());
        }
        content.child(
            Button::new("plugin-save-limits")
                .label(t!("plugin.save_reload").to_string())
                .on_click(
                    cx.listener(|this, _, window, cx| match this.save(window, cx) {
                        Ok(()) => window.close_dialog(cx),
                        Err(error) => {
                            this.error = Some(error);
                            cx.notify();
                        }
                    }),
                ),
        )
    }
}
