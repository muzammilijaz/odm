use rand::Rng;
use std::time::Duration;

/// Exponential backoff with full jitter, capped at 10s.
pub fn backoff_delay(attempt: u32) -> Duration {
    let base_ms = 200u64;
    let cap_ms = 10_000u64;
    let exp = base_ms.saturating_mul(1u64 << attempt.min(20)).min(cap_ms);
    let jittered = rand::thread_rng().gen_range(0..=exp.max(1));
    Duration::from_millis(jittered)
}

/// Transient errors worth retrying: transport/timeout failures and common
/// overload/redirect status codes. Mirrors `ExceptionHelper.IsMomentumError`.
pub fn is_retryable(err: &reqwest::Error) -> bool {
    if err.is_timeout() || err.is_connect() {
        return true;
    }
    if let Some(status) = err.status() {
        return matches!(
            status.as_u16(),
            408 | 425 | 429 | 500 | 502 | 503 | 504
        );
    }
    // Body-read errors (connection reset mid-stream etc.) surface without a
    // status code — treat as retryable transport faults.
    err.is_body() || err.is_request()
}
