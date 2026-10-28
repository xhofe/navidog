use std::ops::Range;
use std::rc::Rc;

use gpui_kit::component::highlighter::SyntaxHighlighter;
use gpui_kit::component::input::{
    EditorState, FoldRange, HighlightStyleResolver, InputEdit, InputHighlighter,
    InputHighlighterFactory, Rope,
};
use gpui_kit::component::scroll::ScrollbarMode;
use gpui_kit::component::{Theme, ThemeMode, ThemeRegistry};
use gpui_kit::{App, Context, HighlightStyle, SharedString, Window};

const NAVICAT_THEME: &str = include_str!("../themes/navicat.json");
pub const LIGHT: &str = "Navicat Light";
pub const DARK: &str = "Navicat Dark";
pub const DEFAULT_LIGHT: &str = "Default Light";
pub const DEFAULT_DARK: &str = "Default Dark";

/// Navidog (Navicat colors) or gpui-kit's built-in Default (shadcn).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeFamily {
    Navidog,
    Default,
}

/// Registers the Navicat themes as the default light and dark themes. Call after `gpui_kit::init`.
pub fn init(cx: &mut App) {
    ThemeRegistry::global_mut(cx)
        .load_themes_from_str(NAVICAT_THEME)
        .expect("bundled navicat.json must parse");
    let registry = ThemeRegistry::global(cx);
    let light = registry
        .themes()
        .get(LIGHT)
        .cloned()
        .expect("missing light theme");
    let dark = registry
        .themes()
        .get(DARK)
        .cloned()
        .expect("missing dark theme");
    let theme = Theme::global_mut(cx);
    theme.light_theme = light;
    theme.dark_theme = dark;
    Theme::change(ThemeMode::Light, None, cx);
    // Show the bar when the pointer is over the scroll area.
    Theme::set_scrollbar_mode(ScrollbarMode::Hover, cx);
    // The SQL grammar captures a column as `@field`. The theme has no such key,
    // so the name stayed the body color. `property` is a real key.
    paint_sql_columns();
}

/// Point the SQL column capture at `syntax.property`.
///
/// A leading `(identifier)` capture paints identifier nodes that are not
/// columns. Later captures (table `@type`, function, alias) still win.
/// A clause fragment never builds those nodes; [`ClauseHighlighter`] paints
/// the words the grammar drops.
fn paint_sql_columns() {
    use gpui_kit::component::highlighter::LanguageRegistry;
    let Some(mut sql) = LanguageRegistry::singleton().language("sql") else {
        return;
    };
    let mut highlights = sql.highlights.replace("@field", "@property");
    const BARE: &str = "(identifier) @property\n";
    if !highlights.contains(BARE) {
        highlights.insert_str(0, BARE);
    }
    if highlights == sql.highlights.as_ref() {
        return;
    }
    sql.highlights = highlights.into();
    LanguageRegistry::singleton().register("sql", &sql);
}

/// SQL highlighting for a one-line clause.
///
/// tree-sitter-sequel only builds a `field` node inside a statement, and its
/// error recovery does not even keep the identifier. Words that the syntax
/// styles left uncolored become `property`. Keywords, strings, comments and
/// names that already have a color stay as they are.
///
/// ponytail: each edit builds a new highlighter. A clause is one row, so a
/// full reparse is cheaper than threading tree-sitter's `InputEdit` through.
/// Forward the editor's edit if these boxes ever hold a whole script.
struct ClauseHighlighter {
    inner: SyntaxHighlighter,
    text: String,
}

impl ClauseHighlighter {
    fn new() -> Self {
        Self {
            inner: SyntaxHighlighter::new("sql"),
            text: String::new(),
        }
    }

    fn sync(&mut self, text: &Rope) {
        let next = text.to_string();
        if next == self.text {
            return;
        }
        self.inner = SyntaxHighlighter::new("sql");
        self.inner.update(None, text, None);
        self.text = next;
    }
}

impl InputHighlighter for ClauseHighlighter {
    fn language(&self) -> SharedString {
        "sql".into()
    }

    fn update(
        &mut self,
        _edit: Option<InputEdit>,
        text: &Rope,
        _folding: bool,
        _window: &mut Window,
        _cx: &mut Context<EditorState>,
    ) {
        self.sync(text);
    }

    fn styles(
        &self,
        range: &Range<usize>,
        resolver: &dyn HighlightStyleResolver,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        let styles = self.inner.styles(range, resolver);
        let Some(property) = resolver.style("property") else {
            return styles;
        };
        paint_uncolored_words(&self.text, styles, property)
    }

    fn fold_ranges(&self, _: &Rope) -> Vec<FoldRange> {
        Vec::new()
    }
}

/// Install [`ClauseHighlighter`] on a one-line SQL editor. Callers set the
/// language to `sql` first. A different language would get no highlighter,
/// because this replaces the editor's factory.
pub(crate) fn use_clause_highlighter(state: &mut EditorState, cx: &mut Context<EditorState>) {
    let factory: InputHighlighterFactory = Rc::new(|language| {
        language
            .eq_ignore_ascii_case("sql")
            .then(|| Box::new(ClauseHighlighter::new()) as Box<dyn InputHighlighter>)
    });
    state.set_highlighter_factory(factory, cx);
}

/// Words the SQL grammar did not color. Quoted identifiers count; keywords do
/// not, so a dropped `AND` stays the body color instead of a field.
fn bare_ident_ranges(text: &str) -> Vec<Range<usize>> {
    navidog_sql::span::spans(text, navidog_sql::span::Lex::GENERIC)
        .into_iter()
        .filter(|span| match span.kind {
            navidog_sql::span::SpanKind::QuotedIdent => true,
            navidog_sql::span::SpanKind::Word => !navidog_sql::keywords::COMMON
                .iter()
                .any(|keyword| keyword.eq_ignore_ascii_case(span.text(text))),
            _ => false,
        })
        .map(|span| span.range)
        .collect()
}

fn paint_uncolored_words(
    text: &str,
    styles: Vec<(Range<usize>, HighlightStyle)>,
    property: HighlightStyle,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let words = bare_ident_ranges(text);
    let mut out = Vec::with_capacity(styles.len());
    for (range, style) in styles {
        if style.color.is_some() || range.is_empty() {
            out.push((range, style));
            continue;
        }
        let mut cursor = range.start;
        for word in &words {
            let start = word.start.max(range.start);
            let end = word.end.min(range.end);
            if start >= end {
                continue;
            }
            if cursor < start {
                out.push((cursor..start, style));
            }
            out.push((start..end, property));
            cursor = end;
        }
        if cursor < range.end {
            out.push((cursor..range.end, style));
        }
    }
    out
}

/// Points the light and dark slots at one family. Call [`apply_appearance`] afterwards so
/// `Theme::change` loads the slot for the current mode.
pub fn apply_family(family: ThemeFamily, cx: &mut App) {
    let (light_name, dark_name) = match family {
        ThemeFamily::Navidog => (LIGHT, DARK),
        ThemeFamily::Default => (DEFAULT_LIGHT, DEFAULT_DARK),
    };
    let registry = ThemeRegistry::global(cx);
    let Some(light) = registry.themes().get(light_name).cloned() else {
        tracing::error!("missing theme {light_name}");
        return;
    };
    let Some(dark) = registry.themes().get(dark_name).cloned() else {
        tracing::error!("missing theme {dark_name}");
        return;
    };
    let theme = Theme::global_mut(cx);
    theme.light_theme = light;
    theme.dark_theme = dark;
}

/// Keeps the rem base at 13pt. The Default theme omits `font.size`, and gpui's built-in size is 16.
pub fn pin_layout_font(cx: &mut App) {
    Theme::update(cx, |theme| {
        theme.font_size = gpui_kit::px(13.); // raw-px: layout rem base
        theme.mono_font_size = gpui_kit::px(12.); // raw-px: mono size from navicat.json
    });
}

/// Applies the appearance: `None` follows the system, `Some(true)` forces dark, `Some(false)` forces light.
pub fn apply_appearance(dark: Option<bool>, window: Option<&mut Window>, cx: &mut App) {
    match dark {
        None => Theme::sync_system_appearance(window, cx),
        Some(true) => Theme::change(ThemeMode::Dark, None, cx),
        Some(false) => Theme::change(ThemeMode::Light, None, cx),
    }
}

/// Syntax color keys the Editor preferences can override (F-PREF-004).
pub const SYNTAX_KEYS: [&str; 7] = [
    "text",
    "keyword",
    "property",
    "comment",
    "string",
    "number",
    "background",
];

/// Puts the user's syntax colors (`#rrggbb` by key) over the active theme's editor colors.
/// Theme switches reload the theme, so call this after every appearance change.
pub fn apply_syntax_colors(colors: &std::collections::BTreeMap<String, String>, cx: &mut App) {
    if colors.is_empty() {
        return;
    }
    let mut theme = (*Theme::global(cx).highlight_theme).clone();
    for (key, hex) in colors {
        let Some(color) = parse_hex(hex) else {
            continue;
        };
        let style = || serde_json::from_value(serde_json::json!({ "color": hex })).ok();
        let syntax = &mut theme.style.syntax;
        match key.as_str() {
            "text" => theme.style.editor_foreground = Some(color),
            "background" => theme.style.editor_background = Some(color),
            "keyword" => syntax.keyword = style(),
            "property" => syntax.property = style(),
            "comment" => {
                syntax.comment = style();
                syntax.comment_doc = style();
            }
            "string" => syntax.string = style(),
            "number" => syntax.number = style(),
            _ => {}
        }
    }
    Theme::global_mut(cx).highlight_theme = std::sync::Arc::new(theme);
    cx.refresh_windows();
}

/// The color of a syntax key in the active theme, for the color pickers.
pub fn syntax_color(key: &str, cx: &App) -> Option<gpui_kit::Hsla> {
    let style = &Theme::global(cx).highlight_theme.style;
    let pick = |s: Option<gpui_kit::component::highlighter::ThemeStyle>| {
        s.and_then(|s| gpui_kit::HighlightStyle::from(s).color)
    };
    match key {
        "text" => style.editor_foreground,
        "background" => style.editor_background,
        "keyword" => pick(style.syntax.keyword),
        "property" => pick(style.syntax.property),
        "comment" => pick(style.syntax.comment),
        "string" => pick(style.syntax.string),
        "number" => pick(style.syntax.number),
        _ => None,
    }
}

pub fn parse_hex(hex: &str) -> Option<gpui_kit::Hsla> {
    let digits = hex.trim().trim_start_matches('#');
    if digits.len() != 6 {
        return None;
    }
    let bytes = hex::decode(digits).ok()?;
    // raw-px: a color the user picked
    Some(gpui_kit::rgb(u32::from_be_bytes([0, bytes[0], bytes[1], bytes[2]])).into())
}

pub fn to_hex(color: gpui_kit::Hsla) -> String {
    let c = gpui_kit::Rgba::from(color);
    let byte = |f: f32| (f.clamp(0., 1.) * 255.).round() as u8;
    format!("#{}", hex::encode([byte(c.r), byte(c.g), byte(c.b)]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::{dark_palette, light_palette, palette};
    use gpui_kit::component::ThemeSet;

    #[test]
    fn theme_parses_with_both_modes_on_13pt_base() {
        let set: ThemeSet = serde_json::from_str(NAVICAT_THEME).unwrap();
        let names: Vec<_> = set.themes.iter().map(|t| t.name.to_string()).collect();
        assert_eq!(names, [LIGHT, DARK]);
        assert!(set.themes.iter().all(|t| t.font_size == Some(13.)));
        assert!(set.themes[1].mode.is_dark());
    }

    #[test]
    fn hex_round_trips() {
        let c = parse_hex("#0433ff").unwrap();
        assert_eq!(to_hex(c), "#0433ff");
        assert!(parse_hex("blue").is_none());
    }

    #[gpui_kit::test]
    fn a_missing_theme_leaves_the_current_one(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            // Navicat themes are not loaded. This used to panic at startup.
            apply_family(ThemeFamily::Navidog, cx);
        });
    }

    /// The user's colors replace the theme's editor colors.
    #[gpui_kit::test]
    fn syntax_colors_override_the_theme(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            init(cx);
            let colors = std::collections::BTreeMap::from([
                ("keyword".to_string(), "#ff0000".to_string()),
                ("property".to_string(), "#9a4d00".to_string()),
                ("background".to_string(), "#000000".to_string()),
            ]);
            apply_syntax_colors(&colors, cx);
            assert_eq!(
                syntax_color("keyword", cx).map(to_hex).as_deref(),
                Some("#ff0000")
            );
            assert_eq!(
                syntax_color("property", cx).map(to_hex).as_deref(),
                Some("#9a4d00")
            );
            assert_eq!(
                syntax_color("background", cx).map(to_hex).as_deref(),
                Some("#000000")
            );
        });
    }

    /// A theme without `highlight` keeps the previous mode's editor colors (white DDL in dark mode).
    #[gpui_kit::test]
    fn editor_colors_follow_appearance(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            init(cx);
        });
        cx.run_until_parked();
        for (mode, palette) in [
            (ThemeMode::Dark, dark_palette()),
            (ThemeMode::Light, light_palette()),
        ] {
            cx.update(|cx| Theme::change(mode, None, cx));
            let style = cx.update(|cx| Theme::global(cx).highlight_theme.style.clone());
            assert_eq!(
                style.editor_background,
                Some(palette.control_bg),
                "{mode:?}"
            );
            assert!(style.syntax.keyword.is_some(), "{mode:?}");
        }
    }

    /// A column reference takes the property color. A clause fragment has no
    /// `field` node; the one-line highlighter paints that bare word. A table
    /// name keeps `type`, and a string or operator stays uncolored.
    #[gpui_kit::test]
    fn a_column_name_uses_the_property_color(cx: &mut gpui_kit::TestAppContext) {
        use gpui_kit::component::input::{InputHighlighter, Rope};
        cx.update(|cx| {
            gpui_kit::init(cx);
            init(cx);
            let theme = Theme::global(cx).highlight_theme.clone();
            let color_at = |sql: &str, needle: &str| {
                let mut highlighter = ClauseHighlighter::new();
                let rope = Rope::from(sql);
                highlighter.sync(&rope);
                let at = sql.find(needle).unwrap();
                InputHighlighter::styles(&highlighter, &(0..rope.len()), theme.as_ref())
                    .into_iter()
                    .find(|(range, _)| range.start <= at && at < range.end)
                    .and_then(|(_, style)| style.color)
            };
            let property =
                gpui_kit::HighlightStyle::from(theme.style.syntax.property.unwrap()).color;
            let keyword = gpui_kit::HighlightStyle::from(theme.style.syntax.keyword.unwrap()).color;
            let ty = gpui_kit::HighlightStyle::from(theme.style.syntax.type_.unwrap()).color;
            assert_eq!(color_at("select id from t", "id"), property);
            assert_eq!(color_at("id = 1 AND first_name", "id"), property);
            assert_eq!(color_at("id = 1 AND first_name", "first_name"), property);
            assert_eq!(color_at("id = 1 AND first_name", "AND"), keyword);
            assert_ne!(color_at("id = 1 AND first_name", "="), property);
            assert_ne!(color_at("id = 1 AND first_name", "1"), property);
            assert_ne!(color_at("id = 'abc'", "abc"), property);
            assert_eq!(color_at("SELECT id FROM actor", "actor"), ty);
            assert_ne!(color_at("SELECT id FROM actor", "actor"), property);
        });
    }

    /// Default uses gpui-kit's shadcn themes; Navidog restores the measured palette. Font stays 13pt.
    #[gpui_kit::test]
    fn switching_family_swaps_the_theme_and_palette(cx: &mut gpui_kit::TestAppContext) {
        use gpui_kit::component::ActiveTheme as _;
        cx.update(|cx| {
            gpui_kit::init(cx);
            init(cx);
        });
        cx.run_until_parked();
        for (family, mode, name) in [
            (ThemeFamily::Default, ThemeMode::Light, DEFAULT_LIGHT),
            (ThemeFamily::Default, ThemeMode::Dark, DEFAULT_DARK),
            (ThemeFamily::Navidog, ThemeMode::Light, LIGHT),
            (ThemeFamily::Navidog, ThemeMode::Dark, DARK),
        ] {
            cx.update(|cx| {
                apply_family(family, cx);
                Theme::change(mode, None, cx);
                if family == ThemeFamily::Default {
                    // gpui's built-in size, which Default does not override
                    Theme::update(cx, |theme| theme.font_size = gpui_kit::px(16.)); // raw-px: gpui built-in size
                    pin_layout_font(cx);
                }
            });
            cx.update(|cx| {
                assert_eq!(Theme::global(cx).theme_name().as_ref(), name);
                let colors = palette(cx);
                if family == ThemeFamily::Navidog {
                    let expected = if mode.is_dark() {
                        dark_palette()
                    } else {
                        light_palette()
                    };
                    assert_eq!(colors, expected, "{name}");
                } else {
                    assert_eq!(colors.window_bg, cx.theme().background, "{name}");
                    assert_eq!(colors.selection, cx.theme().list_active, "{name}");
                    assert_eq!(colors.selection_text, cx.theme().foreground, "{name}");
                    assert_eq!(colors.row_selected, cx.theme().list_active, "{name}");
                    assert_eq!(cx.theme().font_size, gpui_kit::px(13.), "{name}"); // raw-px: layout rem base
                    assert_eq!(cx.theme().mono_font_size, gpui_kit::px(12.), "{name}"); // raw-px: mono size from navicat.json
                }
            });
        }
    }
}
