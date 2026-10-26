//! Rendering of the SQL text panel and its right-click menu.

use super::*;

use gpui_kit::component::input::{Copy, Cut, Editor, EditorState};
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{
    App, Entity, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _, div, px,
};
use rust_i18n::t;

impl QueryEditor {
    /// The theme's mono font (`font.mono_family`); zoom and the preferences change only its size.
    pub(crate) fn render_editor(&self, cx: &App) -> impl IntoElement {
        let state = self.editor.clone();
        div().id("query-editor-text").size_full().min_h_0().child(
            Editor::new(&self.editor)
                .bordered(false)
                .size_full()
                // raw-px: font size in points set by zoom or the preferences
                .text_size(px(self.font_pt(cx)))
                .context_menu(move |menu, _, cx| context_menu(menu, &state, cx)),
        )
    }
}

/// Right-click menu of the SQL editor (30 §3). Items about the selection are disabled without one.
fn context_menu(menu: NativeMenu, state: &Entity<EditorState>, cx: &App) -> NativeMenu {
    use crate::format::{
        Beautify, CommentLines, LowerCase, PasteFromStack, UncommentLines, UpperCase,
    };
    use gpui_kit::component::input::{Paste, SelectAll};
    let selected = state.read(cx).selected_text().to_string();
    let none = selected.is_empty();
    menu.menu_with_disabled(t!("query.cut").to_string(), none, Box::new(Cut))
        .menu_with_disabled(t!("query.copy").to_string(), none, Box::new(Copy))
        .menu(t!("query.paste").to_string(), Box::new(Paste))
        .menu(
            t!("query.paste_from_stack").to_string(),
            Box::new(PasteFromStack),
        )
        .menu(t!("query.select_all").to_string(), Box::new(SelectAll))
        .separator()
        .menu_with_disabled(
            t!("query.run_selected").to_string(),
            none,
            Box::new(RunSelected),
        )
        .menu(t!("query.run_current").to_string(), Box::new(RunCurrent))
        .menu_with_disabled(
            t!("query.explain_selected").to_string(),
            none,
            Box::new(Explain),
        )
        .separator()
        .menu(t!("query.beautify").to_string(), Box::new(Beautify))
        .submenu(
            t!("query.comment").to_string(),
            NativeMenu::new()
                .menu(
                    t!("query.comment_lines").to_string(),
                    Box::new(CommentLines),
                )
                .menu(
                    t!("query.uncomment_lines").to_string(),
                    Box::new(UncommentLines),
                ),
        )
        .submenu(
            t!("query.convert_case").to_string(),
            NativeMenu::new()
                .menu(t!("query.upper_case").to_string(), Box::new(UpperCase))
                .menu(t!("query.lower_case").to_string(), Box::new(LowerCase)),
        )
        .separator()
        .menu(
            t!("query.create_snippet").to_string(),
            Box::new(navidog_app_state::actions::CreateSnippet {
                code: selected.into(),
            }),
        )
}
