use super::*;

use gpui_kit::component::button::Button;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::{
    IntoElement, ParentElement as _, PathPromptOptions, SharedString, Styled as _, Window,
};
use navidog_ui::tokens;
use rust_i18n::t;

impl ConnectionForm {
    fn browse(
        &mut self,
        target: Text,
        directories: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: !directories,
            directories,
            multiple: false,
            prompt: None,
        });
        cx.spawn_in(window, async move |_, cx| {
            if let Ok(Ok(Some(paths))) = rx.await
                && let Some(path) = paths.first()
            {
                let path = path.to_string_lossy().to_string();
                let _ = target.update_in(cx, |s, window, cx| s.set_value(path, window, cx));
            }
        })
        .detach();
    }

    pub(crate) fn path_field(
        &self,
        id: &'static str,
        input: &Text,
        wide: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let width = if wide {
            tokens::form::FIELD_WIDTH_WIDE
        } else {
            tokens::form::FIELD_WIDTH
        };
        let target = input.clone();
        h_flex()
            .gap_1()
            .child(
                Input::new(input)
                    .id(SharedString::from(format!("{id}-input")))
                    .w(width.rems()),
            )
            .child(
                Button::new(id)
                    .label(t!("conn.choose").to_string())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.browse(target.clone(), false, window, cx)
                    })),
            )
    }
}
