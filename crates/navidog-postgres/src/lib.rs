//! PostgreSQL: the protocol driver, the SQL dialect, catalog queries and keywords.
mod dialect;
mod driver;
mod metadata;
mod words;

pub use dialect::PostgresDialect;
pub use driver::PostgresDriver;
