//! Drivers need tokio, while GPUI's executor is not a tokio runtime, so driver I/O runs here.
//! The returned `JoinHandle` can be awaited directly from GPUI `cx.spawn`.

use std::future::Future;
use std::sync::LazyLock;

use tokio::runtime::{Builder, Runtime};
use tokio::task::JoinHandle;

static RUNTIME: LazyLock<Runtime> = LazyLock::new(|| {
    Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("navidog-db")
        .enable_all()
        .build()
        .expect("cannot start the database runtime")
});

pub fn runtime() -> &'static Runtime {
    &RUNTIME
}

pub fn spawn<T: Send + 'static>(fut: impl Future<Output = T> + Send + 'static) -> JoinHandle<T> {
    RUNTIME.spawn(fut)
}
