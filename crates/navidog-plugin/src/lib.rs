//! Permission-scoped plugin discovery, request transport and isolated Shell applications.
extern crate gpui_kit as gpui;

pub mod manifest;
pub mod protocol;
pub mod runtime;
pub mod settings;

pub mod host;

pub mod manager;
pub use manager::{Plugins, init};

pub mod preferences;
rust_i18n::i18n!("locales", fallback = "en");

pub mod driver;

pub mod panel;

pub mod format;

mod limits_view;
