//! Shared Tokio runtime and HTTP client for every network service.
//!
//! The UI runs on GPUI's executor, so network work is spawned here and the
//! returned [`tokio::task::JoinHandle`] is awaited from any executor (it is a
//! plain `Future` that does not need a Tokio context to be polled).

use std::future::Future;

use once_cell::sync::Lazy;
use tokio::runtime::{Builder, Runtime};

static RUNTIME: Lazy<Runtime> = Lazy::new(build_runtime);

static HTTP_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    // Self-hosted servers live on loopback, the LAN or a tailnet; a system or
    // environment proxy would only get in the way of reaching them.
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap_or_else(|err| {
            log::error!("HTTP client setup failed, using defaults: {err}");
            reqwest::Client::default()
        })
});

fn build_runtime() -> Runtime {
    let multi = Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("whisper-net")
        .enable_all()
        .build();
    match multi {
        Ok(rt) => rt,
        Err(err) => {
            log::error!("multi-thread runtime unavailable ({err}); trying a single worker");
            match Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(err) => {
                    // Without a runtime no network service can work at all.
                    log::error!("Tokio runtime could not start: {err}");
                    std::process::abort();
                }
            }
        }
    }
}

/// The shared HTTP client (connection pool reused across requests).
pub fn http_client() -> &'static reqwest::Client {
    &HTTP_CLIENT
}

/// Spawns a future on the shared runtime. The handle can be awaited from any
/// executor; dropping it detaches the task (use `abort()` to cancel it).
pub fn spawn<F>(future: F) -> tokio::task::JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    RUNTIME.spawn(future)
}

/// Runs a future to completion on the shared runtime, blocking the caller.
/// Must not be called from inside the runtime's own worker threads.
#[cfg(test)]
pub fn block_on<F: Future>(future: F) -> F::Output {
    RUNTIME.block_on(future)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawned_tasks_complete_from_a_foreign_executor() {
        let handle = spawn(async { 41 + 1 });
        let value = futures::executor::block_on(handle).ok();
        assert_eq!(value, Some(42));
    }
}
