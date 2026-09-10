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
///
/// Clause 6.3.10 fixes both the direction and the reported size. With `lastN`
/// the page runs backwards and `size` "shall be equal to the requested
/// `lastN`" -- the client's own number, not how many instances were found.
/// Without `lastN` the page runs forwards and `size` "shall be equal to `*`",
/// and `range-start` is the request's `timeAt` only for `timerel=after` and
/// `timerel=between`; for `timerel=before` it is the least recent timestamp in
/// the response, because a backwards-anchored bound says nothing about where
/// the forwards page actually begins.
#[must_use]
pub fn build_content_range(
    interval: &TimeInterval,
    time_range: Option<(DateTime<Utc>, DateTime<Utc>)>,
    last_n_origin: LastNOrigin,
) -> Option<NgsiLdContentRange> {
    let (min_time, max_time) = time_range?;
    let least_recent = whole_seconds(min_time);
    let most_recent = whole_seconds(max_time);

    let (start, end, size) = if let Some(requested) = last_n_origin.requested() {
        (backwards_range_start(interval, &most_recent), least_recent, Some(u64::from(requested)))
    } else {
        (forwards_range_start(interval, &least_recent), most_recent, None)
    };

    let start = start.parse().ok()?;
    let end = end.parse().ok()?;
    Some(NgsiLdContentRange::new(start, end, size))
}

/// Clause 6.3.10, backwards pagination: `range-start` is `timeAt` for
/// `timerel=before`, `endTimeAt` for `timerel=between`, and otherwise the most
/// recent timestamp in the range of the response.
fn backwards_range_start(interval: &TimeInterval, most_recent: &str) -> String {
    match interval {
        TimeInterval::Before(instant) => whole_seconds(*instant),
        TimeInterval::Between { to, .. } => whole_seconds(*to),
        TimeInterval::After(_) | TimeInterval::Unbounded => most_recent.to_owned(),
    }
}

/// Clause 6.3.10, forwards pagination: `range-start` is `timeAt` for
/// `timerel=after` and `timerel=between`, and the least recent timestamp in the
/// range of the response for `timerel=before` — a backwards-anchored bound says
/// nothing about where a forwards page begins.
fn forwards_range_start(interval: &TimeInterval, least_recent: &str) -> String {
    match interval {
        TimeInterval::After(instant) | TimeInterval::Between { from: instant, .. } => whole_seconds(*instant),
        TimeInterval::Before(_) | TimeInterval::Unbounded => least_recent.to_owned(),
    }
}
