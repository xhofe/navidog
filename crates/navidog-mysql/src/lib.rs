//! MySQL and MariaDB: the protocol driver, the SQL dialect, catalog queries and keywords.
//! Both drivers share one dialect.
mod dialect;
mod driver;
mod metadata;
mod words;

pub use dialect::MysqlDialect;
pub use driver::MySqlDriver;
