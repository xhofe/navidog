//! Committing a batch of table changes. This is database protocol, not grid
//! UI, so it lives beside the drivers and is shared by the table viewer and
//! editable query results.

use navidog_util::starts_with_ignore_ascii_case;

use crate::dialect::Dialect;
use crate::{DbError, DbResult, Relational, ResultSet};

/// Change statements and how to commit them.
#[derive(Clone)]
pub struct Commit {
    pub statements: Vec<String>,
    pub begin: &'static str,
    pub dialect: Dialect,
}

impl Commit {
    pub fn new(dialect: Dialect, statements: Vec<String>) -> Self {
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
        if let Err(error) = result.and_then(|sets| matched_a_row(statement, &sets, commit.dialect))
        {
            let _ = rel.execute(undo).await;
            return Err(error);
        }
    }
    if in_tx {
        rel.execute("RELEASE SAVEPOINT navidog_apply").await?;
    }
    if !in_tx || end {
        rel.execute("COMMIT").await?;
    }
    Ok(())
}

/// An UPDATE or DELETE that matched no row means the grid's old values no
/// longer find it (changed elsewhere, or a value only the server knew). MySQL
/// counts changed rather than matched rows for UPDATE, so only its DELETE is
/// checked.
pub fn matched_a_row(sql: &str, sets: &[ResultSet], dialect: Dialect) -> DbResult<()> {
    let delete = starts_with_ignore_ascii_case(sql, "DELETE");
    let update = starts_with_ignore_ascii_case(sql, "UPDATE") && dialect != Dialect::MySql;
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
        assert!(matched_a_row(update, &affected(0), Dialect::Sqlite).is_err());
        assert!(matched_a_row(update, &affected(1), Dialect::Postgres).is_ok());
        assert!(matched_a_row(delete, &affected(0), Dialect::MySql).is_err());
        // MySQL counts changed rows: an UPDATE to the same value is 0 and still fine
        assert!(matched_a_row(update, &affected(0), Dialect::MySql).is_ok());
        assert!(matched_a_row("INSERT INTO t VALUES (1)", &affected(0), Dialect::Sqlite).is_ok());
    }
}
