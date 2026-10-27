rust_i18n::i18n!("locales", fallback = "en");
const _: &str = include_str!("../locales/sqlite.yml");

mod dialect;
mod driver;
mod metadata;
mod words;

pub use dialect::SqliteDialect;
pub use driver::SqliteDriver;
