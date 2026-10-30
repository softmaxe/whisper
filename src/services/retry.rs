//! Exponential-backoff retry.

use std::future::Future;
use std::time::Duration;

/// Code of the client-side LLM deadline error; never retried.
pub const LLM_REQUEST_TIMEOUT_CODE: &str = "LLM_REQUEST_TIMEOUT";

/// Backoff ladder; defaults are 3 retries, 1 s doubling up to 10 s.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RetryOptions {
    pub max_retries: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub backoff_multiplier: f64,
}

impl Default for RetryOptions {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(10),
            backoff_multiplier: 2.0,
        }
    }
}

/// Runs `f` until it succeeds, `should_retry` refuses, or retries run out;
/// returns the last error.
pub async fn with_retry<T, E, F, Fut>(
    mut f: F,
    options: &RetryOptions,
    should_retry: impl Fn(&E) -> bool,
) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    let mut delay = options.initial_delay;
    let mut attempt = 0;
    loop {
        match f().await {
            Ok(value) => return Ok(value),
            Err(err) => {
                if attempt >= options.max_retries || !should_retry(&err) {
                    return Err(err);
                }
                tokio::time::sleep(delay).await;
                delay = delay
                    .mul_f64(options.backoff_multiplier)
                    .min(options.max_delay);
                attempt += 1;
            }
        }
    }
}

/// Whether an API failure is worth retrying: never the LLM deadline; network
/// faults (no status) always; otherwise only 408, 429 and 5xx.
pub fn api_should_retry(status: Option<u16>, code: Option<&str>) -> bool {
    if code == Some(LLM_REQUEST_TIMEOUT_CODE) {
        return false;
    }
    match status {
        None => true,
        Some(status) => status == 408 || status == 429 || (500..600).contains(&status),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::test_server::run;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn retry_strategy_by_status_and_code() {
        assert!(api_should_retry(None, None), "network errors retry");
        assert!(!api_should_retry(None, Some(LLM_REQUEST_TIMEOUT_CODE)));
        assert!(!api_should_retry(Some(400), None));
        assert!(!api_should_retry(Some(404), None));
        assert!(api_should_retry(Some(408), None));
        assert!(api_should_retry(Some(429), None));
        assert!(api_should_retry(Some(500), None));
        assert!(api_should_retry(Some(503), None));
        assert!(!api_should_retry(Some(600), None));
        assert!(!api_should_retry(Some(499), None), "cancellation is final");
    }

    fn fast() -> RetryOptions {
        RetryOptions {
            initial_delay: Duration::from_millis(1),
            ..RetryOptions::default()
        }
    }

    #[test]
    fn stops_after_one_attempt_when_refused() {
        let attempts = AtomicU32::new(0);
        let result: Result<(), u16> = run(with_retry(
            || async {
                attempts.fetch_add(1, Ordering::SeqCst);
                Err(400)
            },
            &fast(),
            |s| api_should_retry(Some(*s), None),
        ));
        assert_eq!(result, Err(400));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn retries_5xx_up_to_max() {
        let attempts = AtomicU32::new(0);
        let options = RetryOptions {
            max_retries: 2,
            ..fast()
        };
        let result: Result<(), u16> = run(with_retry(
            || async {
                attempts.fetch_add(1, Ordering::SeqCst);
                Err(503)
            },
            &options,
            |s| api_should_retry(Some(*s), None),
        ));
        assert!(result.is_err());
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            3,
            "initial attempt plus two retries"
        );
    }

    #[test]
    fn succeeds_after_transient_failures_and_backs_off() {
        let attempts = AtomicU32::new(0);
        let options = RetryOptions {
            initial_delay: Duration::from_millis(20),
            max_delay: Duration::from_millis(30),
            ..RetryOptions::default()
        };
        let started = std::time::Instant::now();
        let result: Result<u32, u16> = run(with_retry(
            || async {
                let n = attempts.fetch_add(1, Ordering::SeqCst);
                if n < 3 { Err(502) } else { Ok(n) }
            },
            &options,
            |s| api_should_retry(Some(*s), None),
        ));
        assert_eq!(result, Ok(3));
        // 20 + 30 (capped) + 30 ms of backoff.
        assert!(started.elapsed() >= Duration::from_millis(80));
    }
}
