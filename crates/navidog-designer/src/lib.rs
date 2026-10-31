//! Table design panels; schema planning belongs to navidog-db.
rust_i18n::i18n!("locales", fallback = "en");

mod object;
mod panel;
pub use object::ObjectDesigner;
pub use panel::TableDesigner;

pub fn init(cx: &mut gpui_kit::App) {
    panel::init(cx);
    object::init(cx);
}
