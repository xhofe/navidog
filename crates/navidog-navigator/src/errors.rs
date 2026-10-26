//! Localized recovery guidance around unmodified database diagnostics.

use navidog_db::DbError;

pub(crate) fn database_message(error: &DbError) -> String {
    navidog_ui::db_error_text(
        error.code.as_deref(),
        &error.message,
        error.is_session_lost(),
        error.is_outcome_unknown(),
    )
}
