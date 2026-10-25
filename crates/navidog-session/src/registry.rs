//! Registered drivers, shared by built-in drivers and plugin JS drivers (M5).

use std::sync::Arc;

use gpui_kit::{App, Global};
use navidog_core::registry::Registry;
use navidog_db::Driver;
use navidog_db::dialect::Dialect;

#[derive(Default)]
pub struct DriverRegistry(pub Registry<Arc<dyn Driver>>);

impl Global for DriverRegistry {}

impl DriverRegistry {
    pub fn get(cx: &App, id: &str) -> Option<Arc<dyn Driver>> {
        cx.global::<Self>().0.get(id).cloned()
    }

    pub fn icon(cx: &App, id: &str) -> &'static str {
        Self::get(cx, id)
            .map(|driver| driver.info().icon)
            .unwrap_or("icons/db/mysql.svg")
    }

    pub fn dialect(cx: &App, id: &str) -> Option<Dialect> {
        Self::get(cx, id).map(|driver| driver.info().dialect)
    }

    pub fn name(cx: &App, id: &str) -> gpui_kit::SharedString {
        Self::get(cx, id)
            .map(|driver| driver.info().name.clone().into())
            .unwrap_or_else(|| id.to_string().into())
    }

    pub fn all(cx: &App) -> Vec<Arc<dyn Driver>> {
        cx.global::<Self>()
            .0
            .iter()
            .map(|(_, _, driver)| driver.clone())
            .collect()
    }
}
