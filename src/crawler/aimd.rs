//! # AIMD Adaptive Politeness & Congestion Controller
//!
//! Additive-Increase/Multiplicative-Decrease congestion control algorithm
//! adapting request delays and worker task concurrency based on origin response
//! latencies, error rates, and HTTP 429 rate limits.
//!
//! ## Mathematical Model (RFC 9309 & TCP Reno Inspired)
//!
//! ```text
//!                               ┌────────────────────────┐
//!                               │ Sample Window (W = 50) │
//!                               └───────────┬────────────┘
//!                                           │
//!                     Compute: Error Rate (E) & p95 TTFB
//!                                           │
//!             ┌─────────────────────────────┴─────────────────────────────┐
//!             ▼                                                           ▼
//!    Degradation Trigger                                           Healthy Recovery
//!    (E > 8% OR p95 > 1.5x baseline OR 429)                       (E == 0% AND p95 < 500ms)
//!             │                                                           │
//!             ▼                                                           ▼
//!  Multiplicative Backoff:                                       Additive Recovery:
//!  delay = min(max(delay, 100) * 2.0, 10,000ms)                  delay = max(delay - 25ms, floor)
//!  concurrency = max(floor(c * 0.5), 1)                          concurrency = min(c + 1, c_max)
//! ```

use std::collections::VecDeque;

/// A single recorded request sample in the rolling sample window.
#[derive(Debug, Clone, Copy)]
struct RequestSample {
    ttfb_ms: u32,
    is_error: bool,
}

/// Adaptive congestion controller tuning crawl delays and concurrency in real time.
#[derive(Debug, Clone)]
pub struct AimdController {
    /// Maximum allowed concurrency.
    c_max: usize,
    /// Minimum allowed delay floor in milliseconds (e.g. from robots.txt Crawl-Delay).
    delay_floor_ms: u64,
    /// Maximum delay ceiling in milliseconds (10,000 ms).
    max_delay_ms: u64,
    /// Additive recovery step in milliseconds (25 ms).
    additive_step_ms: u64,
    /// Multiplicative backoff factor (2.0).
    backoff_factor: f64,
    /// Error rate ceiling threshold (0.08 = 8%).
    error_threshold: f64,
    /// Latency multiplier over baseline (1.5).
    latency_multiplier: f64,
    /// Rolling window size (50 requests).
    window_size: usize,

    /// Current worker concurrency.
    current_concurrency: usize,
    /// Current politeness delay in milliseconds.
    current_delay_ms: u64,
    /// Rolling sample window.
    samples: VecDeque<RequestSample>,
    /// Moving baseline TTFB in milliseconds.
    baseline_ttfb_ms: u32,
}

impl AimdController {
    /// Creates a new `AimdController` with maximum concurrency and delay floor.
    ///
    /// # Arguments
    ///
    /// * `c_max` - Maximum number of concurrent tasks allowed.
    /// * `delay_floor_ms` - Minimum delay floor (0ms if unspecified, or higher if robots.txt dictates).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use blacksparrow::crawler::aimd::AimdController;
    ///
    /// let controller = AimdController::new(10, 0);
    /// assert_eq!(controller.current_concurrency(), 10);
    /// assert_eq!(controller.current_delay_ms(), 0);
    /// ```
    pub fn new(c_max: usize, delay_floor_ms: u64) -> Self {
        let initial_concurrency = c_max.max(1);
        Self {
            c_max: initial_concurrency,
            delay_floor_ms,
            max_delay_ms: 10_000,
            additive_step_ms: 25,
            backoff_factor: 2.0,
            error_threshold: 0.08,
            latency_multiplier: 1.5,
            window_size: 50,
            current_concurrency: initial_concurrency,
            current_delay_ms: delay_floor_ms,
            samples: VecDeque::with_capacity(50),
            baseline_ttfb_ms: 200,
        }
    }

    /// Records a successful HTTP request with its time-to-first-byte (TTFB) latency.
    ///
    /// If the rolling window is healthy (error rate == 0% and p95 latency is stable),
    /// the controller additively recovers concurrency and reduces politeness delay.
    pub fn record_success(&mut self, ttfb_ms: u32) {
        if self.samples.len() >= self.window_size {
            self.samples.pop_front();
        }
        self.samples.push_back(RequestSample {
            ttfb_ms,
            is_error: false,
        });

        // Update baseline moving average
        if self.baseline_ttfb_ms == 0 {
            self.baseline_ttfb_ms = ttfb_ms;
        } else {
            self.baseline_ttfb_ms =
                ((self.baseline_ttfb_ms as u64 * 9 + ttfb_ms as u64) / 10) as u32;
        }

        // Check if healthy recovery should occur
        let err_rate = self.error_rate();
        let p95 = self.p95_ttfb();

        if err_rate == 0.0
            && p95 < 500
            && (p95 as f64) <= (self.baseline_ttfb_ms as f64 * self.latency_multiplier)
        {
            // Additive decrease of delay down to delay_floor
            self.current_delay_ms = self
                .current_delay_ms
                .saturating_sub(self.additive_step_ms)
                .max(self.delay_floor_ms);

            // Additive increase of concurrency up to c_max
            self.current_concurrency = (self.current_concurrency + 1).min(self.c_max);
        }
    }

    /// Records a failed or rate-limited HTTP request.
    ///
    /// Triggers multiplicative backoff immediately if the response is an explicit
    /// 429 Too Many Requests, or if the rolling error rate exceeds 8%.
    pub fn record_failure(&mut self, status: Option<u16>, is_rate_limited: bool) {
        if self.samples.len() >= self.window_size {
            self.samples.pop_front();
        }
        self.samples.push_back(RequestSample {
            ttfb_ms: 0,
            is_error: true,
        });

        let is_server_distress = status.map(|s| (500..=504).contains(&s)).unwrap_or(false);

        if is_rate_limited || is_server_distress || self.error_rate() > self.error_threshold {
            self.trigger_multiplicative_backoff();
        }
    }

    /// Executes multiplicative backoff: doubles delay and halves concurrency.
    fn trigger_multiplicative_backoff(&mut self) {
        let new_delay = if self.current_delay_ms == 0 {
            100
        } else {
            ((self.current_delay_ms as f64) * self.backoff_factor) as u64
        };
        self.current_delay_ms = new_delay.min(self.max_delay_ms);

        let new_concurrency =
            ((self.current_concurrency as f64) / self.backoff_factor).floor() as usize;
        self.current_concurrency = new_concurrency.max(1);
    }

    /// Returns the current dynamic delay in milliseconds.
    pub fn current_delay_ms(&self) -> u64 {
        self.current_delay_ms
    }

    /// Returns the current permitted task concurrency.
    pub fn current_concurrency(&self) -> usize {
        self.current_concurrency
    }

    /// Calculates the 95th percentile TTFB across the current sample window.
    pub fn p95_ttfb(&self) -> u32 {
        if self.samples.is_empty() {
            return 0;
        }

        let mut latencies: Vec<u32> = self
            .samples
            .iter()
            .filter(|s| !s.is_error)
            .map(|s| s.ttfb_ms)
            .collect();

        if latencies.is_empty() {
            return 0;
        }

        latencies.sort_unstable();
        let idx = ((latencies.len() as f64) * 0.95).ceil() as usize;
        let clamped_idx = idx.saturating_sub(1).min(latencies.len() - 1);
        latencies[clamped_idx]
    }

    /// Calculates the failure rate [0.0, 1.0] across the current sample window.
    pub fn error_rate(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let err_count = self.samples.iter().filter(|s| s.is_error).count();
        err_count as f64 / self.samples.len() as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aimd_initial_state() {
        let c = AimdController::new(8, 50);
        assert_eq!(c.current_concurrency(), 8);
        assert_eq!(c.current_delay_ms(), 50);
        assert_eq!(c.error_rate(), 0.0);
    }

    #[test]
    fn test_aimd_multiplicative_backoff_and_recovery() {
        let mut c = AimdController::new(10, 0);

        // Immediate 429 backoff
        c.record_failure(Some(429), true);
        assert_eq!(c.current_concurrency(), 5);
        assert_eq!(c.current_delay_ms(), 100);

        // Second backoff
        c.record_failure(Some(503), false);
        assert_eq!(c.current_concurrency(), 2);
        assert_eq!(c.current_delay_ms(), 200);

        // Additive recovery over 55 healthy responses (flushing 50-sample window)
        for _ in 0..55 {
            c.record_success(80);
        }
        assert!(c.current_delay_ms() < 200);
        assert!(c.current_concurrency() > 2);
    }
}
