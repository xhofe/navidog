//! Committing a batch of table changes. This is database protocol, not grid
//! UI, so it lives beside the drivers and is shared by the table viewer and
//! editable query results.

use std::sync::Arc;

use navidog_util::starts_with_ignore_ascii_case;

use crate::{DbError, DbResult, Relational, ResultSet};
use navidog_sql::dialect::Dialect;

/// Change statements and how to commit them.
#[derive(Clone)]
pub struct Commit {
    pub statements: Vec<String>,
    pub begin: &'static str,
    pub dialect: Arc<dyn Dialect>,
}

impl Commit {
    pub fn new(dialect: Arc<dyn Dialect>, statements: Vec<String>) -> Self {
        Self {
            statements,
            begin: dialect.begin_transaction(),
            dialect,
        }
    }
}

/// Applies the changes; either all take effect or none. Without an open
/// transaction they run in one that is rolled back on failure; inside the
/// user's transaction a savepoint does that, so a failed batch does not leave
/// half its statements in the transaction. When `end` is true, the already
/// open transaction is committed.
pub async fn apply(
    rel: &mut dyn Relational,
    commit: Commit,
    in_tx: bool,
    end: bool,
) -> DbResult<()> {
    const SAVEPOINT: &str = "SAVEPOINT navidog_apply";
    let (start, undo) = if in_tx {
        (SAVEPOINT, "ROLLBACK TO SAVEPOINT navidog_apply")
    } else {
        (commit.begin, "ROLLBACK")
    };
    rel.execute(start).await?;
    for statement in &commit.statements {
        let result = rel.execute(statement).await;
        if let Err(error) =
            result.and_then(|sets| matched_a_row(statement, &sets, commit.dialect.as_ref()))
        {
            if let Err(rollback) = rel.execute(undo).await {
                tracing::error!(%rollback, undo, "could not undo the failed batch");
            }
            return Err(error);
        }
    }
    if in_tx {
        rel.execute("RELEASE SAVEPOINT navidog_apply").await?;
    }
    if (!in_tx || end)
        && let Err(error) = rel.execute("COMMIT").await
    {
        if !in_tx {
            // We opened this transaction, so a failed COMMIT must not leave
            // it active (SQLite keeps the write lock after SQLITE_BUSY).
            if let Err(rollback) = rel.execute("ROLLBACK").await {
                tracing::error!(%rollback, "could not roll back the failed COMMIT");
            }
        }
        return Err(error);
    }
    Ok(())
}

/// An UPDATE or DELETE that matched no row means the grid's old values no
/// longer find it (changed elsewhere, or a value only the server knew). MySQL
/// counts changed rather than matched rows for UPDATE, so only its DELETE is
/// checked.
pub fn matched_a_row(sql: &str, sets: &[ResultSet], dialect: &dyn Dialect) -> DbResult<()> {
    let delete = starts_with_ignore_ascii_case(sql, "DELETE");
    let update =
        starts_with_ignore_ascii_case(sql, "UPDATE") && !dialect.features().counts_changed_rows;
    if (delete || update) && sets.last().and_then(|s| s.affected_rows) == Some(0) {
        return Err(DbError::coded(
            "navidog.row_not_found",
            format!("The statement matched no row: {sql}"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scripted {
        log: Vec<String>,
        fail_commit: bool,
    }

    impl Relational for Scripted {
        fn dialect(&self) -> std::sync::Arc<dyn Dialect> {
            std::sync::Arc::new(navidog_sqlite::SqliteDialect)
        }

        fn execute<'a>(
            &'a mut self,
            sql: &'a str,
        ) -> futures::future::BoxFuture<'a, DbResult<Vec<ResultSet>>> {
            self.log.push(sql.to_string());
            let fail = self.fail_commit && sql == "COMMIT";
            Box::pin(async move {
                if fail {
                    Err(DbError::msg("injected COMMIT failure"))
                } else {
                    Ok(vec![ResultSet {
                        affected_rows: Some(1),
                        ..ResultSet::default()
                    }])
                }
            })
        }

        crate::unsupported_catalog!();
    }

    #[test]
    fn a_failed_commit_rolls_back_a_transaction_we_opened() {
        let mut rel = Scripted {
            log: Vec::new(),
            fail_commit: true,
        };
        let error = futures::executor::block_on(apply(
            &mut rel,
            Commit::new(
                std::sync::Arc::new(navidog_sqlite::SqliteDialect),
                Vec::new(),
            ),
            false,
            false,
        ))
        .unwrap_err();
        assert_eq!(error.message, "injected COMMIT failure");
        assert_eq!(rel.log, ["BEGIN", "COMMIT", "ROLLBACK"]);
    }

    #[test]
    fn a_failed_commit_does_not_roll_back_the_users_transaction() {
        let mut rel = Scripted {
            log: Vec::new(),
            fail_commit: true,
        };
        let _ = futures::executor::block_on(apply(
            &mut rel,
            Commit::new(
                std::sync::Arc::new(navidog_sqlite::SqliteDialect),
                Vec::new(),
            ),
            true,
            true,
        ));
        assert_eq!(
            rel.log,
            [
                "SAVEPOINT navidog_apply",
                "RELEASE SAVEPOINT navidog_apply",
                "COMMIT"
            ]
        );
    }

    #[test]
    fn a_write_that_matched_no_row_fails() {
        let affected = |n| {
            vec![ResultSet {
                affected_rows: Some(n),
                ..ResultSet::default()
            }]
        };
        let update = "UPDATE \"t\" SET \"name\" = 'b' WHERE \"id\" IS NULL";
        let delete = "DELETE FROM `t` WHERE `id` = 1 LIMIT 1";
        assert!(matched_a_row(update, &affected(0), &navidog_sqlite::SqliteDialect).is_err());
        assert!(matched_a_row(update, &affected(1), &navidog_postgres::PostgresDialect).is_ok());
        assert!(matched_a_row(delete, &affected(0), &navidog_mysql::MysqlDialect).is_err());
        // MySQL counts changed rows: an UPDATE to the same value is 0 and still fine
        assert!(matched_a_row(update, &affected(0), &navidog_mysql::MysqlDialect).is_ok());
        assert!(
            matched_a_row(
                "INSERT INTO t VALUES (1)",
                &affected(0),
                &navidog_sqlite::SqliteDialect
            )
            .is_ok()
        );
    }
}
