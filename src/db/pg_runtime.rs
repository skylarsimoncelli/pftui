use std::future::Future;
use std::sync::OnceLock;

use tokio::runtime::{Builder, Runtime};

fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to initialize shared Postgres runtime")
    })
}

pub fn block_on<F: Future>(future: F) -> F::Output {
    runtime().block_on(future)
}
