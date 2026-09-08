//! `Content-Range` header builder for NGSI-LD temporal pagination
//! (clause 6.3.10).
//!
//! This is response-construction logic, not a payload transform -- it sits
//! alongside [`andromeda_http_response::pagination`] which builds `Link`
//! headers from analogous pagination state.

use andromeda_http_headers::ngsild_content_range::NgsiLdContentRange;
use andromeda_ngsi_ld_lexicon::rfc3339::whole_seconds;
use andromeda_ngsi_ld_query::temporal::{last_n::LastNOrigin, window::TimeInterval};
use chrono::{DateTime, Utc};

/// Build a `Content-Range` header value for temporal pagination.
///
/// Returns `None` when no time range is available (empty result set).
/// The format is `date-time <start>-<end>/<size>` where size is `*` when the
/// total is unknown.
#[must_use]
pub fn build_content_range(
    interval: &TimeInterval,
    time_range: Option<(DateTime<Utc>, DateTime<Utc>)>,
    partition_total: Option<i64>,
    last_n_origin: LastNOrigin,
) -> Option<NgsiLdContentRange> {
    let (min_time, max_time) = time_range?;
    let lrt = whole_seconds(min_time);
    let mrt = whole_seconds(max_time);

    let (start, end, size) = if last_n_origin.is_user_specified() {
        let upper = match interval {
            TimeInterval::Before(instant) => whole_seconds(*instant),
            TimeInterval::Between { to, .. } => whole_seconds(*to),
            TimeInterval::After(_) | TimeInterval::Unbounded => mrt,
        };
        let size = partition_total.map_or_else(|| "*".to_string(), |n| n.to_string());
        (upper, lrt, size)
    } else {
        let lower = interval.reference().map_or(lrt, |instant| whole_seconds(*instant));
        (lower, mrt, "*".to_string())
    };

    let start = start.parse().ok()?;
    let end = end.parse().ok()?;
    Some(NgsiLdContentRange::new(start, end, size.parse().ok()))
}
