//! SQL dialects and one dialect-aware scanner shared by every SQL consumer.
//!
//! Statement splitting, `[$name]` parameter scanning, the format menu, the table viewer's
//! filter parser and the table designer all read the same [`span`] list, so one script cannot be
//! read two different ways. No UI and no drivers here.

pub mod dialect;
pub mod keywords;
pub mod span;

pub use dialect::Dialect;
