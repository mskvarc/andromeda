//! Throttling-window arithmetic for subscription notification slots.
//!
//! The persistence layer claims slots with a pure timestamp comparison; the
//! service layer converts the subscription's configured throttling interval
//! into the earliest `last_notification` that still permits a new slot.

use chrono::{DateTime, Duration, Utc};

/// Compute the earliest `last_notification` timestamp that still allows a new
/// notification slot to be claimed, given the subscription's throttling window.
///
/// Returns `None` when throttling is disabled (no ceiling: any prior
/// `last_notification` is acceptable).
///
/// Fractional-second throttling values are converted into nanoseconds before
/// building the [`Duration`], so sub-second throttling (e.g. `0.5`) stays
/// precise.
pub fn ceiling_for_throttle(now: DateTime<Utc>, throttling_secs: Option<f64>) -> Option<DateTime<Utc>> {
    let secs = throttling_secs?;
    if !secs.is_finite() || secs <= 0.0 {
        return None;
    }
    let nanos = (secs * 1_000_000_000.0).round() as i64;
    Some(now - Duration::nanoseconds(nanos))
}
