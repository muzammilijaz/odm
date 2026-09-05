/// One byte-range segment of the file being downloaded.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Chunk {
    pub index: usize,
    pub start: u64,
    /// Inclusive end offset. Meaningless when `unbounded` is true.
    pub end: u64,
    /// Bytes already written for this chunk (relative to `start`), used for resume.
    pub position: u64,
    /// True when the total size wasn't known up front (no `Content-Length`) — the
    /// worker streams with no `Range` header until the connection closes, and
    /// completion is detected by EOF rather than reaching `end`.
    #[serde(default)]
    pub unbounded: bool,
}

impl Chunk {
    pub fn remaining_start(&self) -> u64 {
        self.start + self.position
    }

    pub fn is_complete(&self) -> bool {
        !self.unbounded && self.remaining_start() > self.end
    }

    pub fn len(&self) -> u64 {
        self.end - self.start + 1
    }
}

/// Split `total_size` bytes into up to `chunk_count` chunks, honoring `min_chunk_size`.
/// Degrades to a single chunk when the file is small or ranged requests aren't
/// supported.
pub fn plan_chunks(
    total_size: u64,
    chunk_count: usize,
    min_chunk_size: u64,
    supports_range: bool,
) -> Vec<Chunk> {
    if total_size == 0 || !supports_range || chunk_count <= 1 {
        return vec![Chunk {
            index: 0,
            start: 0,
            end: total_size.saturating_sub(1),
            position: 0,
            unbounded: false,
        }];
    }

    let max_chunks_by_size = (total_size / min_chunk_size.max(1)).max(1) as usize;
    let effective_count = chunk_count.min(max_chunks_by_size).max(1);

    let base_size = total_size / effective_count as u64;
    let remainder = total_size % effective_count as u64;

    let mut chunks = Vec::with_capacity(effective_count);
    let mut start = 0u64;
    for i in 0..effective_count {
        let extra = if (i as u64) < remainder { 1 } else { 0 };
        let size = base_size + extra;
        let end = start + size - 1;
        chunks.push(Chunk {
            index: i,
            start,
            end,
            position: 0,
            unbounded: false,
        });
        start = end + 1;
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_evenly_when_possible() {
        let chunks = plan_chunks(1000, 4, 1, true);
        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks[0].start, 0);
        assert_eq!(chunks.last().unwrap().end, 999);
        let total: u64 = chunks.iter().map(|c| c.len()).sum();
        assert_eq!(total, 1000);
    }

    #[test]
    fn falls_back_to_single_chunk_without_range_support() {
        let chunks = plan_chunks(1000, 4, 1, false);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].end, 999);
    }

    #[test]
    fn shrinks_chunk_count_for_small_files() {
        let chunks = plan_chunks(100, 8, 50, true);
        assert_eq!(chunks.len(), 2);
    }

    #[test]
    fn zero_size_is_single_empty_chunk() {
        let chunks = plan_chunks(0, 8, 1, true);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].end, 0);
        assert_eq!(chunks[0].start, 0);
    }
}
