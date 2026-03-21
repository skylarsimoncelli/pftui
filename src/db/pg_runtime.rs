use std::future::Future;
use std::sync::OnceLock;

use tokio::runtime::{Builder, Handle, Runtime};

fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to initialize shared Postgres runtime")
    })
}

/// Run a future to completion on the shared Postgres runtime.
///
/// If we are already inside a tokio runtime (e.g., the mobile API server's
/// axum handlers), we use `block_in_place` + the current runtime's handle
/// to avoid the "cannot start a runtime from within a runtime" panic.
/// Otherwise, we use the dedicated pg_runtime.
pub fn block_on<F: Future>(future: F) -> F::Output {
    if let Ok(handle) = Handle::try_current() {
        // We're inside a tokio runtime already. Use block_in_place to allow
        // blocking on this worker thread without deadlocking the runtime.
        tokio::task::block_in_place(|| handle.block_on(future))
    } else {
        // No runtime on this thread — use our dedicated one.
        runtime().block_on(future)
    }
}
