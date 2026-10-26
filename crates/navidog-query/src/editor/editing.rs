//! Text, preferences, zoom, completion focus and results placement of the query editor.

use super::*;

use gpui_kit::component::input::TabSize;
use gpui_kit::{App, Context, Window};
use navidog_app_state::settings::{ResultsPosition, Settings};
use navidog_ui::tokens;

impl QueryEditor {
    /// Inserts at the caret (info pane identifiers and snippets). A snippet's `${…}` placeholders
    /// become tab stops.
    pub fn insert_text(
        &mut self,
        text: &str,
        snippet: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if snippet {
            self.insert_snippet(text, window, cx);
            return;
        }
        let text = text.to_string();
        self.editor.update(cx, |e, cx| {
            e.insert(text, window, cx);
            e.focus(window, cx);
        });
    }

    pub fn text(&self, cx: &App) -> String {
        self.editor.read(cx).value().to_string()
    }

    /// Replaces the text (one undo step) and marks the query modified.
    pub fn set_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let len = self.editor.read(cx).text().len();
        let text = text.to_string();
        self.editor.update(cx, |e, cx| {
            e.set_selected_range(0..len, cx);
            e.replace(text, window, cx);
        });
    }

    /// Line numbers, folding, tabs and highlighting follow the preferences.
    pub(crate) fn apply_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let s = Settings::global(cx).clone();
        let lang = language_for(&s, self.editor.read(cx).text().len());
        self.sql_highlight = lang == "sql";
        let wanted = (
            s.show_line_numbers,
            s.code_folding,
            s.tab_width,
            s.insert_spaces,
        );
        if self.applied == Some(wanted) && self.editor.read(cx).language_name() == lang {
            return;
        }
        self.applied = Some(wanted);
        self.editor.update(cx, |e, cx| {
            e.set_line_number(s.show_line_numbers, window, cx);
            e.set_folding(s.code_folding, window, cx);
            e.set_tab_size(
                TabSize {
                    tab_size: s.tab_width.clamp(1, 16) as usize,
                    hard_tabs: !s.insert_spaces,
                },
                cx,
            );
            if e.language_name() != lang {
                e.set_highlighter(lang, cx);
            }
        });
        if !s.bracket_highlight
            && let Some(d) = &self.brackets
        {
            d.clear(cx);
            self.bracket_caret = None;
        }
        cx.notify();
    }

    pub(crate) fn on_text_changed(&mut self, cx: &mut Context<Self>) {
        self.menu_moved = false;
        // Past the size limit the editor turns plain text and completion stops (F-PREF-004)
        let s = Settings::global(cx);
        let lang = language_for(s, self.editor.read(cx).text().len());
        self.sql_highlight = lang == "sql";
        if self.editor.read(cx).language_name() != lang {
            self.editor.update(cx, |e, cx| e.set_highlighter(lang, cx));
        }
        if !self.dirty {
            self.dirty = true;
            cx.notify();
        }
        // Errors marked by the last run go once the text changes
        self.editor.update(cx, |e, cx| {
            if let Some(d) = e.diagnostics_mut()
                && !d.is_empty()
            {
                d.clear();
                cx.notify();
            }
        });
    }

    /// The editor font size: zoom, else the preferences, else the token.
    pub(crate) fn font_pt(&self, cx: &App) -> f32 {
        let s = Settings::global(cx);
        self.zoom.unwrap_or(if s.use_default_editor_font {
            tokens::query::EDITOR_FONT_SIZE.value()
        } else {
            s.editor_font_pt
        })
    }

    pub(crate) fn zoom_by(&mut self, step: f32, cx: &mut Context<Self>) {
        let next = (self.font_pt(cx) + step).clamp(
            tokens::query::EDITOR_FONT_MIN,
            tokens::query::EDITOR_FONT_MAX,
        );
        self.zoom = Some(next);
        cx.notify();
    }

    pub(crate) fn toggle_word_wrap(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.word_wrap = !self.word_wrap;
        let wrap = self.word_wrap;
        self.editor
            .update(cx, |e, cx| e.set_soft_wrap(wrap, window, cx));
        cx.notify();
    }

    pub(crate) fn completion_open(&self, cx: &App) -> bool {
        self.editor.read(cx).completion_menu_state().open
    }

    /// With "auto select first item" off, Enter and Tab accept only after an arrow key.
    pub(crate) fn accepts_completion(&self, cx: &App) -> bool {
        Settings::global(cx).auto_select_first || self.menu_moved
    }

    pub(crate) fn editor_focused(&self, window: &Window, cx: &App) -> bool {
        self.editor.read(cx).focus_handle(cx).is_focused(window)
    }

    /// Menu items dispatch along the focused element; keep focus inside this editor.
    pub(crate) fn focus_for_menu(&self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.focus_handle.contains_focused(window, cx) {
            window.focus(&self.focus_handle, cx);
        }
    }

    pub(crate) fn has_selection(&self, cx: &App) -> bool {
        !self.editor.read(cx).selected_range().is_empty()
    }
}

fn set_position(position: ResultsPosition, cx: &mut App) {
    Settings::update(cx, |s| s.results_position = position);
    // Every open query editor follows; the menus show the check
    cx.refresh_windows();
}

impl QueryEditor {
    /// Result-position buttons call this directly. `dispatch_action` follows focus, and after the
    /// results page unmounts the SQL editor that focus is no longer in the tree.
    pub(crate) fn place_results(
        &mut self,
        position: ResultsPosition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let show_page = position == ResultsPosition::NewPage && self.results.has_run();
        self.results_page = show_page;
        set_position(position, cx);
        cx.notify();
        if !show_page {
            self.focus_sql(window, cx);
        }
    }

    pub(crate) fn back_to_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.results_page = false;
        cx.notify();
        self.focus_sql(window, cx);
    }

    fn focus_sql(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |e, cx| e.focus(window, cx));
    }
}
