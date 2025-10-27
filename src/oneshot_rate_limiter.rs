use std::time::{Duration, Instant};

pub struct OneshotRateLimiter<const MS_DELAY: u64> {
    last_time: Instant,
}

impl<const MS_DELAY: u64> Default for OneshotRateLimiter<MS_DELAY> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const MS_DELAY: u64> OneshotRateLimiter<MS_DELAY> {
    pub fn new() -> Self {
        Self {
            last_time: Instant::now() - Duration::from_millis(MS_DELAY),
        }
    }

    pub fn try_acquire(&mut self) -> bool {
        let now = Instant::now();
        if now.duration_since(self.last_time) >= Duration::from_millis(MS_DELAY) {
            self.last_time = now;
            true
        } else {
            false
        }
    }
}
