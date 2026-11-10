use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{Duration, Instant};

#[derive(Debug)]
struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

/// Token-bucket global bandwidth limiter supporting live `TrafficUsageMode` speed changes.
#[derive(Debug, Clone)]
pub struct BandwidthLimiter {
    limit_bps: Arc<AtomicU64>,
    strict_compliant: Arc<AtomicBool>,
    bucket: Arc<Mutex<Bucket>>,
}

impl BandwidthLimiter {
    pub fn new(limit_bps: u64) -> Self {
        Self {
            limit_bps: Arc::new(AtomicU64::new(limit_bps)),
            strict_compliant: Arc::new(AtomicBool::new(true)),
            bucket: Arc::new(Mutex::new(Bucket {
                tokens: limit_bps as f64,
                last_refill: Instant::now(),
            })),
        }
    }

    pub fn set_limit_bps(&self, limit_bps: u64) {
        self.limit_bps.store(limit_bps, Ordering::Relaxed);
    }

    pub fn limit_bps(&self) -> u64 {
        self.limit_bps.load(Ordering::Relaxed)
    }

    pub fn set_compliant_mode(&self, enabled: bool) {
        self.strict_compliant.store(enabled, Ordering::Relaxed);
    }

    pub fn compliant_mode(&self) -> bool {
        self.strict_compliant.load(Ordering::Relaxed)
    }

    /// Acquire permission for `bytes` transferred; returns immediately if unlimited (`0`).
    pub async fn acquire(&self, bytes: usize) {
        if bytes == 0 {
            return;
        }
        let mut remaining = bytes as f64;
        loop {
            let rate = self.limit_bps.load(Ordering::Relaxed);
            if rate == 0 || remaining <= 0.0 {
                return;
            }
            let strict = self.strict_compliant.load(Ordering::Relaxed);
            let wait_dur = {
                let mut g = self.bucket.lock().await;
                let now = Instant::now();
                let elapsed = now.duration_since(g.last_refill).as_secs_f64();
                let rate_f = rate as f64;
                let max_bucket = if strict { rate_f * 0.25 } else { rate_f };
                g.tokens = (g.tokens + elapsed * rate_f).min(max_bucket);
                g.last_refill = now;

                if g.tokens >= remaining {
                    g.tokens -= remaining;
                    return;
                }
                if g.tokens > 0.0 {
                    remaining -= g.tokens;
                    g.tokens = 0.0;
                }
                let max_sleep = if strict { 0.1 } else { 0.25 };
                Duration::from_secs_f64((remaining / rate_f).min(max_sleep).max(0.001))
            };
            tokio::time::sleep(wait_dur).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn limiter_enforces_rate_and_switches_compliant_mode() {
        let limiter = BandwidthLimiter::new(40_000);
        assert!(limiter.compliant_mode());
        limiter.set_compliant_mode(false);
        assert!(!limiter.compliant_mode());
        limiter.set_compliant_mode(true);
        assert!(limiter.compliant_mode());

        // Drain initial burst and verify subsequent acquisition takes noticeable time.
        limiter.acquire(20_000).await;
        let start = Instant::now();
        limiter.acquire(10_000).await;
        let elapsed = start.elapsed();
        assert!(elapsed >= Duration::from_millis(120), "elapsed was {:?}", elapsed);

        // Zero limit bypasses throttling immediately.
        limiter.set_limit_bps(0);
        let fast = Instant::now();
        limiter.acquire(1_000_000).await;
        assert!(fast.elapsed() < Duration::from_millis(50));
    }
}
