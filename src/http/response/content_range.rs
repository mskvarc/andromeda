//! `Content-Range` header builder for NGSI-LD temporal pagination
//! (clause 6.3.10).
//!
//! This is response-construction logic, not a payload transform -- it sits
//! alongside [`crate::http::response::pagination`] which builds `Link`
//! headers from analogous pagination state.

use crate::{
    domain::temporal::TimeRel,
    query::compiled::{CompiledTemporalQuery, LastNOrigin},
    service::temporal::format_datetime,
};
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
        let upper = match compiled.timerel {
            Some(TimeRel::Before) => compiled.time_at.map_or_else(|| mrt.clone(), format_datetime),
            Some(TimeRel::Between) => compiled.end_time_at.map_or_else(|| mrt.clone(), format_datetime),
            Some(TimeRel::After) => mrt,
            None => mrt,
        };
        let size = partition_total.map_or_else(|| "*".to_string(), |n| n.to_string());
        (upper, lrt, size)
    } else {
        let lower = match compiled.timerel {
            Some(_) => compiled.time_at.map(format_datetime).unwrap_or(lrt),
            None => lrt,
        };
        (lower, mrt, "*".to_string())
    };

    Some(format!("date-time {}-{}/{}", start, end, size))
}
