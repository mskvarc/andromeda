//! How a Context Source says a page of its temporal answer was cut short
//! (clause 6.3.10).
//!
//! A temporal answer a source cannot give at once is a 206 whose
//! `Content-Range` names the instants it answered for, and the rest is asked
//! for with the window resumed at the range's end. A 206 without a
//! `Content-Range`, or with one that cannot be read, still says the answer is
//! incomplete, but names no instant the rest can be asked from.

use andromeda_http_headers::ngsild_content_range::NgsiLdContentRange;
use andromeda_ngsi_ld_query::temporal::instance_page::PageDirection;
use chrono::{DateTime, Utc};
use derive_more::IsVariant;

/// A page of a temporal answer the source cut short (clause 6.3.10).
#[derive(Debug, Clone, PartialEq, Eq, IsVariant)]
pub enum PartialPage {
    /// A 206 whose `Content-Range` names how far into each entity's history
    /// the page reaches.
    Ranged(NgsiLdContentRange),
    /// A 206 with no `Content-Range`, or with one that cannot be read: the
    /// page is incomplete and names no instant to resume from.
    Unranged,
}

impl PartialPage {
    /// The `Content-Range` the page was cut at; `None` when it named none.
    #[must_use]
    pub const fn range(&self) -> Option<&NgsiLdContentRange> {
        match self {
            PartialPage::Ranged(range) => Some(range),
            PartialPage::Unranged => None,
        }
    }
}

/// The instant any of `pages` was cut at that is reached least far in
/// `direction`: the earliest forwards, the latest backwards, where clause
/// 6.3.10 makes a range end the least recent instant of a page. Every page has
/// answered every instance on its held side. A page that named no range names
/// no such instant.
pub(crate) fn nearest_range_end(pages: &[PartialPage], direction: PageDirection) -> Option<DateTime<Utc>> {
    pages
        .iter()
        .filter_map(PartialPage::range)
        .map(NgsiLdContentRange::end)
        .reduce(|left, right| direction.nearer(left, right))
}

#[cfg(test)]
mod tests {
    use crate::partial_page::{PartialPage, nearest_range_end};
    use andromeda_http_headers::ngsild_content_range::NgsiLdContentRange;
    use andromeda_ngsi_ld_query::temporal::instance_page::PageDirection;
    use chrono::{DateTime, Utc};
    use std::num::NonZeroU32;

    fn ranged(text: &str) -> PartialPage {
        PartialPage::Ranged(text.parse::<NgsiLdContentRange>().expect("a valid content range"))
    }

    fn instant(text: &str) -> DateTime<Utc> {
        text.parse().expect("a valid instant")
    }

    #[test]
    fn the_nearest_forward_range_end_is_the_earliest_end_of_the_ranged_pages() {
        let pages = [
            ranged("DateTime 2020-01-01T00:00:00Z-2020-03-01T00:00:00Z/*"),
            PartialPage::Unranged,
            ranged("DateTime 2020-01-01T00:00:00Z-2020-02-01T00:00:00Z/*"),
        ];

        pretty_assertions::assert_eq!(nearest_range_end(&pages, PageDirection::Forward), Some(instant("2020-02-01T00:00:00Z")));
    }

    /// Clause 6.3.10: a backward range ends at the least recent instant its
    /// page holds, so the page reached least far is the one ending latest.
    #[test]
    fn the_nearest_backward_range_end_is_the_latest_end_of_the_ranged_pages() {
        let pages = [
            ranged("DateTime 2020-04-01T00:00:00Z-2020-02-01T00:00:00Z/5"),
            PartialPage::Unranged,
            ranged("DateTime 2020-04-01T00:00:00Z-2020-03-01T00:00:00Z/5"),
        ];
        let backward = PageDirection::Backward {
            requested: NonZeroU32::new(5).expect("a positive lastN"),
        };

        pretty_assertions::assert_eq!(nearest_range_end(&pages, backward), Some(instant("2020-03-01T00:00:00Z")));
    }

    #[test]
    fn pages_that_named_no_range_name_no_nearest_range_end() {
        pretty_assertions::assert_eq!(nearest_range_end(&[PartialPage::Unranged], PageDirection::Forward), None);
        pretty_assertions::assert_eq!(nearest_range_end(&[], PageDirection::Forward), None);
    }

    #[test]
    fn only_a_ranged_page_names_a_range() {
        assert!(ranged("DateTime 2020-01-01T00:00:00Z-2020-02-01T00:00:00Z/*").range().is_some());
        assert!(PartialPage::Unranged.range().is_none());
    }
}
