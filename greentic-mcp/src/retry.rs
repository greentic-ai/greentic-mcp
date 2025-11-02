use std::time::Duration;

use rand::distr::{Distribution, Uniform};

/// Compute an exponential backoff delay with jitter.
///
/// `attempt` is zero-based. Jitter is applied in the range [0.5, 1.5] of the computed base delay.
pub fn backoff(base: Duration, attempt: u32) -> Duration {
    let multiplier = 1u128.checked_shl(attempt.min(16)).unwrap_or(1u128 << 16);
    let millis = base.as_millis().max(1);
    let scaled = millis.saturating_mul(multiplier);
    let max = scaled.min(u64::MAX as u128) as u64;
    let uniform = Uniform::new_inclusive(0.5f64, 1.5f64).expect("valid jitter bounds");
    let mut rng = rand::rng();
    let jitter = uniform.sample(&mut rng);
    let jittered = (max as f64 * jitter).round().clamp(1.0, u64::MAX as f64);
    Duration::from_millis(jittered as u64)
}
