use navidog_db::DbError;

use futures::FutureExt as _;
use futures::future::BoxFuture;
use std::sync::Arc;
use tokio::sync::Mutex;

use navidog_db::{CancelHandle, DbResult, Session, sessions};

#[test]
fn error_record_supports_external_literals_and_exhaustive_destructuring() {
    let error = DbError {
        code: Some("08006".into()),
        message: "connection lost".into(),
        disconnected: true,
        outcome_unknown: true,
    };
    assert!(error.is_session_lost());
    assert!(error.is_outcome_unknown());
    assert_eq!(error.to_string(), "08006 - connection lost");
    let DbError {
        code,
        message,
        disconnected,
        outcome_unknown,
    } = error;
    assert_eq!(code.as_deref(), Some("08006"));
    assert_eq!(message, "connection lost");
    assert!(disconnected);
    assert!(outcome_unknown);
}

struct NonRelationalSession;

impl Session for NonRelationalSession {
    fn server_version(&self) -> &str {
        "test"
    }

    fn ping(&mut self) -> BoxFuture<'_, DbResult<()>> {
        async { Ok(()) }.boxed()
    }

    fn cancel_handle(&self) -> Option<CancelHandle> {
        None
    }

    fn session_id(&self) -> Option<u64> {
        None
    }

    fn close(self: Box<Self>) -> BoxFuture<'static, DbResult<()>> {
        async { Ok(()) }.boxed()
    }
}

#[test]
fn non_relational_session_error_has_a_stable_code() {
    navidog_db::runtime::runtime().block_on(async {
        let session: sessions::SharedSession =
            Arc::new(Mutex::new(Some(Box::new(NonRelationalSession))));
        let error = sessions::run_on::<()>(session, |_| {
            async { panic!("a non-relational session must not invoke the operation") }.boxed()
        })
        .await
        .unwrap()
        .unwrap_err();
        assert_eq!(error.code.as_deref(), Some("navidog.not_relational"));
        assert_eq!(
            error.message,
            "This connection is not a relational database."
        );
        assert!(!error.is_session_lost());
        assert!(!error.is_outcome_unknown());
    });
}
