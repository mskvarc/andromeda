//! `Content-Range` header builder for NGSI-LD temporal pagination
//! (clause 6.3.10).
//!
//! This is response-construction logic, not a payload transform: it sits
//! alongside [`andromeda_http_response::pagination`] which builds `Link`
//! headers from analogous pagination state.

use andromeda_http_headers::ngsild_content_range::NgsiLdContentRange;
use andromeda_ngsi_ld_lexicon::time::rendering::instant_text;
use andromeda_ngsi_ld_query::temporal::{instance_page::PageDirection, window::TimeInterval};
use chrono::{DateTime, Utc};

/// Build a `Content-Range` header value for temporal pagination.
///
/// Returns `None` when no time range is available (empty result set).
///
/// Clause 6.3.10 fixes both the direction and the reported size. With `lastN`
/// the page runs backwards and `size` "shall be equal to the requested
/// `lastN`" (the client's own number, not how many instances were found).
/// Without `lastN` the page runs forwards and `size` "shall be equal to `*`",
/// and `range-start` is the request's `timeAt` only for `timerel=after` and
/// `timerel=between`; for `timerel=before` it is the least recent timestamp in
/// the response, because a backwards-anchored bound says nothing about where
/// the forwards page actually begins.
#[must_use]
pub fn build_content_range(
    interval: &TimeInterval,
    time_range: Option<(DateTime<Utc>, DateTime<Utc>)>,
    direction: PageDirection,
) -> Option<NgsiLdContentRange> {
    let (min_time, max_time) = time_range?;
    let least_recent = instant_text(min_time);
    let most_recent = instant_text(max_time);

    let (start, end, size) = match direction {
        PageDirection::Backward { requested } => (backwards_range_start(interval, &most_recent), least_recent, Some(u64::from(requested.get()))),
        PageDirection::Forward => (forwards_range_start(interval, &least_recent), most_recent, None),
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
        TimeInterval::Before(instant) => instant_text(*instant),
        TimeInterval::Between { to, .. } => instant_text(*to),
        TimeInterval::After(_) | TimeInterval::Unbounded => most_recent.to_owned(),
    }
}

/// Clause 6.3.10, forwards pagination: `range-start` is `timeAt` for
/// `timerel=after` and `timerel=between`, and the least recent timestamp in the
/// range of the response for `timerel=before`, because a backwards-anchored bound
/// says nothing about where a forwards page begins.
fn forwards_range_start(interval: &TimeInterval, least_recent: &str) -> String {
    match interval {
        TimeInterval::After(instant) | TimeInterval::Between { from: instant, .. } => instant_text(*instant),
        TimeInterval::Before(_) | TimeInterval::Unbounded => least_recent.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use crate::content_range::build_content_range;
    use andromeda_ngsi_ld_lexicon::time::date_time::NgsiLdDateTime;
    use andromeda_ngsi_ld_query::temporal::{instance_page::PageDirection, window::TimeInterval};
    use chrono::{DateTime, Utc};
    use std::num::NonZeroU32;

    fn instant(raw: &str) -> DateTime<Utc> {
        raw.parse::<NgsiLdDateTime>().expect("valid instant").get()
    }

    /// The response of every case below carries 01:01 to 01:19.
    fn observed() -> (DateTime<Utc>, DateTime<Utc>) {
        (instant("2020-01-01T01:01:00Z"), instant("2020-01-01T01:19:00Z"))
    }

    fn rendered(interval: TimeInterval, direction: PageDirection) -> String {
        build_content_range(&interval, Some(observed()), direction).expect("a range").to_string()
    }

    /// Clause 6.3.10 forwards: range-start is `timeAt` for `after` and
    /// `between`, the least recent timestamp for `before`; range-end is the
    /// most recent timestamp; size is `*`.
    #[test]
    fn a_forward_page_reports_its_start_bound_its_most_recent_instant_and_no_size() {
        let at = instant("2020-01-01T00:00:00Z");
        pretty_assertions::assert_eq!(
            rendered(TimeInterval::After(at), PageDirection::Forward),
            "DateTime 2020-01-01T00:00:00Z-2020-01-01T01:19:00Z/*"
        );
        pretty_assertions::assert_eq!(
            rendered(
                TimeInterval::Between {
                    from: at,
                    to: instant("2021-01-01T00:00:00Z")
                },
                PageDirection::Forward
            ),
            "DateTime 2020-01-01T00:00:00Z-2020-01-01T01:19:00Z/*"
        );
        pretty_assertions::assert_eq!(
            rendered(TimeInterval::Before(instant("2021-01-01T00:00:00Z")), PageDirection::Forward),
            "DateTime 2020-01-01T01:01:00Z-2020-01-01T01:19:00Z/*"
        );
    }

    /// Clause 6.3.10 backwards: range-start is `timeAt` for `before`,
    /// `endTimeAt` for `between`, the most recent timestamp for `after`;
    /// range-end is the least recent timestamp; size is the requested `lastN`.
    #[test]
    fn a_backward_page_reports_its_end_bound_its_least_recent_instant_and_the_requested_last_n() {
        let direction = PageDirection::Backward {
            requested: NonZeroU32::new(5000).expect("a positive lastN"),
        };
        pretty_assertions::assert_eq!(
            rendered(TimeInterval::Before(instant("2021-01-01T00:00:00Z")), direction),
            "DateTime 2021-01-01T00:00:00Z-2020-01-01T01:01:00Z/5000"
        );
        pretty_assertions::assert_eq!(
            rendered(
                TimeInterval::Between {
                    from: instant("2019-01-01T00:00:00Z"),
                    to: instant("2021-06-01T00:00:00Z")
                },
                direction
            ),
            "DateTime 2021-06-01T00:00:00Z-2020-01-01T01:01:00Z/5000"
        );
        pretty_assertions::assert_eq!(
            rendered(TimeInterval::After(instant("2019-01-01T00:00:00Z")), direction),
            "DateTime 2020-01-01T01:19:00Z-2020-01-01T01:01:00Z/5000"
        );
    }

    /// An empty response carries no range at all.
    #[test]
    fn an_empty_response_carries_no_range() {
        assert!(build_content_range(&TimeInterval::Unbounded, None, PageDirection::Forward).is_none());
    }
}
