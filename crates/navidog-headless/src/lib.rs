//! Headless entry point: `navidog-model` and `navidog-db` on their own, with no GPUI dependency.
//!
//! The integration tests in this crate compile and run against those two crates only, so the
//! driver stack cannot quietly grow a window/GPU dependency (A-02). The shell, the navigator and
//! every other pane live in crates that depend on `navidog-app-state` instead.

pub use navidog_db as db;
pub use navidog_model as model;
