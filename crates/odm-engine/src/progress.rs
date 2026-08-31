use std::time::Instant;

#[derive(Debug, Clone, Copy, Default)]
pub struct Progress {
    pub downloaded_bytes: u64,
    /// `None` when the server didn't report a `Content-Length`.
    pub total_bytes: Option<u64>,
    pub bytes_per_sec: f64,
    pub active_chunks: usize,
}

impl Progress {
    pub fn percentage(&self) -> Option<f64> {
        self.total_bytes.map(|total| {
            if total == 0 {
                100.0
            } else {
                (self.downloaded_bytes as f64 / total as f64) * 100.0
            }
        })
    }
}

/// Tracks downloaded-bytes-over-time to compute a smoothed instantaneous speed.
pub(crate) struct SpeedTracker {
    last_sample_at: Instant,
    last_bytes: u64,
    smoothed_bps: f64,
}

impl SpeedTracker {
    pub fn new() -> Self {
        Self {
            last_sample_at: Instant::now(),
            last_bytes: 0,
            smoothed_bps: 0.0,
        }
    }

    pub fn sample(&mut self, total_downloaded: u64) -> f64 {
        let elapsed = self.last_sample_at.elapsed().as_secs_f64();
        if elapsed <= 0.0 {
            return self.smoothed_bps;
        }
        let delta = total_downloaded.saturating_sub(self.last_bytes) as f64;
        let instant_bps = delta / elapsed;
        // Exponential moving average to smooth out bursty chunk completions.
        self.smoothed_bps = if self.smoothed_bps == 0.0 {
            instant_bps
        } else {
            0.3 * instant_bps + 0.7 * self.smoothed_bps
        };
        self.last_sample_at = Instant::now();
        self.last_bytes = total_downloaded;
        self.smoothed_bps
    }
}
