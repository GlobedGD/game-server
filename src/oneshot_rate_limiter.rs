use std::time::{Duration, Instant};

pub struct OneshotRateLimiter<const MsDelay: u64> {
    last_time: Instant,
}

impl<const MsDelay: u64> Default for OneshotRateLimiter<MsDelay> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const MsDelay: u64> OneshotRateLimiter<MsDelay> {
    pub fn new() -> Self {
        Self {
            last_time: Instant::now() - Duration::from_millis(MsDelay),
        }
    }

    pub fn try_acquire(&mut self) -> bool {
        let now = Instant::now();
        if now.duration_since(self.last_time) >= Duration::from_millis(MsDelay) {
            self.last_time = now;
            true
        } else {
            false
        }
    }
}
