//! Localized recovery guidance around unmodified database diagnostics.

use navidog_db::DbError;
use rust_i18n::t;

pub(crate) fn database_message(error: &DbError) -> String {
    message(
        &error.to_string(),
        error.is_session_lost(),
        error.is_outcome_unknown(),
        &rust_i18n::locale(),
    )
}

fn message(detail: &str, session_lost: bool, outcome_unknown: bool, locale: &str) -> String {
    if outcome_unknown {
        t!(
            "nav.database_outcome_unknown",
            locale = locale,
            detail = detail
        )
        .to_string()
    } else if session_lost {
        t!(
            "nav.database_session_lost",
            locale = locale,
            detail = detail
        )
        .to_string()
    } else {
        detail.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_guidance_is_localized_without_changing_the_driver_diagnostic() {
        let detail = "08006 - original driver diagnostic";
        for locale in ["en", "zh-CN"] {
            assert_eq!(message(detail, false, false, locale), detail);
            for unknown in [false, true] {
                let text = message(detail, true, unknown, locale);
                assert!(text.contains(detail));
                assert!(text.contains(if locale == "en" {
                    "Reopen"
                } else {
                    "重新打开"
                }));
                if unknown {
                    assert!(text.contains(if locale == "en" {
                        "outcome may be unknown"
                    } else {
                        "执行结果可能未知"
                    }));
                }
                if locale == "zh-CN" {
                    assert!(!text.contains("Reopen"));
                }
            }
        }
    }
}
