//! `Content-Range` header builder for NGSI-LD temporal pagination
//! (clause 6.3.10).
//!
//! This is response-construction logic, not a payload transform -- it sits
//! alongside [`crate::http::response::pagination`] which builds `Link`
//! headers from analogous pagination state.

use crate::service::temporal::format_datetime;
use andromeda_ngsi_ld_query::temporal::{compiled::CompiledTemporalQuery, last_n::LastNOrigin, window::TimeInterval};
use chrono::{DateTime, Utc};

type TimeRange = (DateTime<Utc>, DateTime<Utc>);

/// Build a `Content-Range` header value for temporal pagination.
///
/// Returns `None` when no time range is available (empty result set).
/// The format is `date-time <start>-<end>/<size>` where size is `*` when the
/// total is unknown.
pub fn build_content_range(
    compiled: &CompiledTemporalQuery,
    time_range: Option<TimeRange>,
    partition_total: Option<i64>,
    last_n_origin: LastNOrigin,
) -> Option<String> {
    let (min_time, max_time) = time_range?;
    let lrt = format_datetime(min_time);
    let mrt = format_datetime(max_time);

    let (start, end, size) = if last_n_origin.is_user_specified() {
        let upper = match compiled.window.interval {
            TimeInterval::Before(instant) => format_datetime(instant),
            TimeInterval::Between { to, .. } => format_datetime(to),
            TimeInterval::After(_) | TimeInterval::Unbounded => mrt,
        };
        let size = partition_total.map_or_else(|| "*".to_string(), |n| n.to_string());
        (upper, lrt, size)
    } else {
        let lower = compiled.window.interval.reference().map_or(lrt, |instant| format_datetime(*instant));
        (lower, mrt, "*".to_string())
    };

    Some(format!("date-time {}-{}/{}", start, end, size))
}
