use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Shared bandwidth limiter. The configured cap is divided live across
/// whatever number of chunks are currently active.
#[derive(Clone)]
pub struct Throttle {
    inner: Arc<Inner>,
}

struct Inner {
    max_bytes_per_sec: AtomicU64,
    active_chunks: AtomicU64,
    window: Mutex<Window>,
}

struct Window {
    started_at: Instant,
    bytes_this_window: u64,
}

impl Throttle {
    pub fn new(max_bytes_per_sec: u64) -> Self {
        Self {
            inner: Arc::new(Inner {
                max_bytes_per_sec: AtomicU64::new(max_bytes_per_sec),
                active_chunks: AtomicU64::new(0),
                window: Mutex::new(Window {
                    started_at: Instant::now(),
                    bytes_this_window: 0,
                }),
            }),
        }
    }

    pub fn set_limit(&self, max_bytes_per_sec: u64) {
        self.inner
            .max_bytes_per_sec
            .store(max_bytes_per_sec, Ordering::Relaxed);
    }

    pub fn register_chunk(&self) -> ChunkGuard {
        self.inner.active_chunks.fetch_add(1, Ordering::Relaxed);
        ChunkGuard {
            inner: self.inner.clone(),
        }
    }

    /// Sleeps as needed so the *global* cap, divided across currently-active
    /// chunks, is respected for this chunk's share of `bytes` just transferred.
    pub async fn throttle(&self, bytes: u64) {
        let limit = self.inner.max_bytes_per_sec.load(Ordering::Relaxed);
        if limit == 0 || bytes == 0 {
            return;
        }
        let active = self.inner.active_chunks.load(Ordering::Relaxed).max(1);
        let per_chunk_limit = (limit / active).max(1);

        let mut window = self.inner.window.lock().await;
        let elapsed = window.started_at.elapsed();
        if elapsed >= Duration::from_secs(1) {
            window.started_at = Instant::now();
            window.bytes_this_window = 0;
        }
        window.bytes_this_window += bytes;

        if window.bytes_this_window > per_chunk_limit {
            let overage = window.bytes_this_window - per_chunk_limit;
            let sleep_secs = overage as f64 / per_chunk_limit as f64;
            let sleep_dur = Duration::from_secs_f64(sleep_secs.min(2.0));
            drop(window);
            tokio::time::sleep(sleep_dur).await;
        }
    }
}

pub struct ChunkGuard {
    inner: Arc<Inner>,
}

impl Drop for ChunkGuard {
    fn drop(&mut self) {
        self.inner.active_chunks.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_limit_updates_live() {
        let t = Throttle::new(1000);
        t.set_limit(2000);
        assert_eq!(t.inner.max_bytes_per_sec.load(Ordering::Relaxed), 2000);
    }
}
