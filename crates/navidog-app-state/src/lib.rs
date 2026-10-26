//! GPUI application state: settings, the connection store, the workspace selection, favorites,
//! virtual groups, snippets, cross-feature actions and object URIs. Pure data lives in
//! `navidog-model`; only the `Global`/`Entity` wrappers and the commands built on them are here.
//! Localized store errors live here so every feature crate can show them without depending on
//! one another.

rust_i18n::i18n!("locales", fallback = "en");
// The proc-macro reads locales itself; include them so Cargo also tracks future edits.
const _: &str = include_str!("../locales/app_state.yml");

// The Action derive macro emits `gpui::` paths
extern crate gpui_kit as gpui;

pub mod actions;
pub mod favorites;
pub mod registry;
pub mod settings;
pub mod snippets;
pub mod store;
pub mod uri;
pub mod vgroups;
pub mod workspace;

use gpui_kit::App;

/// Initialize global state: settings, connection store, registries, favorites, virtual groups and
/// the workspace selection.
pub fn init(cx: &mut App) {
    init_with_settings(cx, settings::Settings::load());
}

/// Like [`init`], but uses settings already read by the caller (the shell reads
/// them once for logging and must not read the file a second time).
pub fn init_with_settings(cx: &mut App, settings: settings::Settings) {
    let _ = _rust_i18n_backend();
    settings::init_with(cx, settings);
    registry::init(cx);
    store::init(cx);
    favorites::init(cx);
    vgroups::init(cx);
    snippets::init(cx);
    workspace::init(cx);
}
