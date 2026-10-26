//! Query-owned recovery guidance around the original driver diagnostic.

use navidog_db::DbError;

pub(crate) fn error_message(error: &DbError) -> String {
    navidog_ui::db_error_text(
        error.code.as_deref(),
        &error.message,
        error.is_session_lost(),
        error.is_outcome_unknown(),
    )
}
