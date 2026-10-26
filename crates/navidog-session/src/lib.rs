//! Runtime state of open connections, the metadata cache and the completion
//! outline. These need GPUI; the drivers themselves live in `navidog-db`.

pub mod outline;
mod registry;
pub mod scope_cache;
pub mod sessions;

pub use registry::DriverRegistry;

use gpui_kit::App;
use std::sync::Arc;

use navidog_core::registry::Source;
use navidog_db::{Driver, mysql, postgres, sqlite};

pub fn init(cx: &mut App) {
    let mut registry = DriverRegistry::default();
    let builtin: [Arc<dyn Driver>; 4] = [
        Arc::new(mysql::MySqlDriver::mysql()),
        Arc::new(mysql::MySqlDriver::mariadb()),
        Arc::new(postgres::PostgresDriver::default()),
        Arc::new(sqlite::SqliteDriver::default()),
    ];
    for driver in builtin {
        let id = driver.info().id.clone();
        registry
            .0
            .register(id, Source::Builtin, driver)
            .expect("built-in driver IDs are unique");
    }
    cx.set_global(registry);
    sessions::init(cx);
    outline::init(cx);
}
