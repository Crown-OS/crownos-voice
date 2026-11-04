use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

const MISS_WINDOW: Duration = Duration::from_secs(1);
/// More misses than this in one window demote the stream.
const TOLERATED_MISSES: u32 = 5;

/// Decides, once per window, whether a stream's backend is too slow.
pub struct DeadlineMonitor {
    misses: Arc<AtomicU32>,
    window_start: Instant,
}

impl DeadlineMonitor {
    pub fn new(misses: Arc<AtomicU32>, now: Instant) -> Self {
        Self {
            misses,
            window_start: now,
        }
    }

    pub fn misses(&self) -> &AtomicU32 {
        &self.misses
    }

    pub fn is_overloaded(&mut self, now: Instant) -> bool {
        if now.duration_since(self.window_start) < MISS_WINDOW {
            return false;
        }
        self.window_start = now;
        self.misses.swap(0, Ordering::Relaxed) > TOLERATED_MISSES
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor_with(misses: u32) -> (DeadlineMonitor, Instant) {
        let start = Instant::now();
        (
            DeadlineMonitor::new(Arc::new(AtomicU32::new(misses)), start),
            start,
        )
    }

    #[test]
    fn tolerates_a_few_misses_per_window() {
        let (mut monitor, start) = monitor_with(TOLERATED_MISSES);
        assert!(!monitor.is_overloaded(start + MISS_WINDOW));
    }

    #[test]
    fn persistent_misses_overload_once_per_window() {
        let (mut monitor, start) = monitor_with(TOLERATED_MISSES + 1);
        assert!(
            !monitor.is_overloaded(start + MISS_WINDOW / 2),
            "the window is not over yet"
        );
        assert!(monitor.is_overloaded(start + MISS_WINDOW));
        assert!(
            !monitor.is_overloaded(start + MISS_WINDOW * 2),
            "the count restarts each window"
        );
    }
}
