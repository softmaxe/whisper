//! Shared Tokio runtime and HTTP client for every network service.
//!
//! The UI runs on GPUI's executor, so network work is spawned here and the
//! returned [`tokio::task::JoinHandle`] is awaited from any executor (it is a
//! plain `Future` that does not need a Tokio context to be polled).

use std::future::Future;
use std::sync::{Arc, LazyLock};

use tokio::runtime::{Builder, Runtime};

static RUNTIME: LazyLock<Runtime> = LazyLock::new(build_runtime);
static BLOCKING_SLOTS: LazyLock<Arc<tokio::sync::Semaphore>> =
    LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(2)));

type StorageJob = Box<dyn FnOnce() + Send>;

static STORAGE: LazyLock<async_channel::Sender<StorageJob>> = LazyLock::new(|| {
    let (tx, rx) = async_channel::unbounded::<StorageJob>();
    std::thread::Builder::new()
        .name("whisper-storage".into())
        .spawn(move || {
            while let Ok(job) = rx.recv_blocking() {
                job();
            }
        })
        .expect("storage worker");
    tx
});

/// Runs CPU work without occupying a network worker. Started jobs retain their
/// permit even if the caller stops awaiting them.
pub async fn blocking<F, T>(work: F) -> Result<T, tokio::task::JoinError>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let permit = BLOCKING_SLOTS
        .clone()
        .acquire_owned()
        .await
        .expect("CPU slots");
    RUNTIME
        .spawn_blocking(move || {
            let _permit = permit;
            work()
        })
        .await
}

/// Serializes accepted storage mutations, including deletion and retention,
/// without making the UI wait on the database connection or filesystem.
pub fn storage<F, T>(work: F) -> impl Future<Output = T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = futures::channel::oneshot::channel();
    let job: StorageJob = Box::new(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work));
        let _ = tx.send(result);
    });
    // Accept every operation at submission, preserving order even when the
    // worker is busy. A bounded mailbox deferred overflow into caller futures,
    // which could be polled out of order or dropped before the write was queued.
    STORAGE.try_send(job).expect("storage worker available");
    async move {
        match rx.await.expect("storage job completed") {
            Ok(value) => value,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }
}

/// Shutdown is the only place allowed to wait synchronously. Storage jobs never
/// require UI callbacks, so draining here cannot deadlock on the UI executor.
/// GPUI's asynchronous quit handlers otherwise have a 200 ms timeout.
pub fn drain_storage() {
    futures::executor::block_on(storage(|| ()));
}

static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
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
    fn accepted_storage_writes_survive_dropped_receivers_and_drain_in_order() {
        let values = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let (release, blocked) = std::sync::mpsc::channel();
        drop(storage(move || blocked.recv().unwrap()));
        for index in 0..64 {
            let values = values.clone();
            // These futures deliberately never get polled: acceptance must
            // happen at submission, not when a UI task first runs.
            drop(storage(move || values.lock().push(index)));
        }
        release.send(()).unwrap();
        drain_storage();
        assert_eq!(*values.lock(), (0..64).collect::<Vec<_>>());
    }

    #[test]
    fn storage_worker_survives_a_failed_job() {
        let failed = storage(|| panic!("synthetic storage failure"));
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                futures::executor::block_on(failed)
            }))
            .is_err()
        );
        assert_eq!(futures::executor::block_on(storage(|| 42)), 42);
    }

    #[test]
    fn cpu_work_is_bounded_and_does_not_block_network_progress() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let tasks: Vec<_> = (0..8)
            .map(|_| {
                let (active, peak) = (active.clone(), peak.clone());
                spawn(async move {
                    blocking(move || {
                        let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(count, Ordering::SeqCst);
                        std::thread::sleep(std::time::Duration::from_millis(2));
                        active.fetch_sub(1, Ordering::SeqCst);
                    })
                    .await
                    .unwrap();
                })
            })
            .collect();
        futures::executor::block_on(async {
            assert_eq!(spawn(async { 42 }).await.unwrap(), 42);
            for task in tasks {
                task.await.unwrap();
            }
        });
        assert!(peak.load(Ordering::SeqCst) <= 2);
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn spawned_tasks_complete_from_a_foreign_executor() {
        let handle = spawn(async { 41 + 1 });
        let value = futures::executor::block_on(handle).ok();
        assert_eq!(value, Some(42));
    }
}
