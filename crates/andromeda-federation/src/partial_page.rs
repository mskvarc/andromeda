//! How a Context Source says a page of its temporal answer was cut short
//! (clause 6.3.10).
//!
//! A temporal answer a source cannot give at once is a 206 whose
//! `Content-Range` names the instants it answered for, and the rest is asked
//! for with the window resumed at the range's end. A 206 without a
//! `Content-Range`, or with one that cannot be read, still says the answer is
//! incomplete, but names no instant the rest can be asked from.

use andromeda_http_headers::ngsild_content_range::NgsiLdContentRange;
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

/// The earliest instant any of `pages` was cut at: every instance up to it has
/// been answered on every page. A page that named no range names no such
/// instant.
pub(crate) fn least_range_end(pages: &[PartialPage]) -> Option<DateTime<Utc>> {
    pages.iter().filter_map(PartialPage::range).map(NgsiLdContentRange::end).min()
}

#[cfg(test)]
mod tests {
    use crate::partial_page::{PartialPage, least_range_end};
    use andromeda_http_headers::ngsild_content_range::NgsiLdContentRange;
    use chrono::{DateTime, Utc};

    fn ranged(text: &str) -> PartialPage {
        PartialPage::Ranged(text.parse::<NgsiLdContentRange>().expect("a valid content range"))
    }

    fn instant(text: &str) -> DateTime<Utc> {
        text.parse().expect("a valid instant")
    }

    #[test]
    fn the_least_range_end_is_the_earliest_end_of_the_ranged_pages() {
        let pages = [
            ranged("DateTime 2020-01-01T00:00:00Z-2020-03-01T00:00:00Z/*"),
            PartialPage::Unranged,
            ranged("DateTime 2020-01-01T00:00:00Z-2020-02-01T00:00:00Z/*"),
        ];

        pretty_assertions::assert_eq!(least_range_end(&pages), Some(instant("2020-02-01T00:00:00Z")));
    }

    #[test]
    fn pages_that_named_no_range_name_no_least_range_end() {
        pretty_assertions::assert_eq!(least_range_end(&[PartialPage::Unranged]), None);
        pretty_assertions::assert_eq!(least_range_end(&[]), None);
    }

    #[test]
    fn only_a_ranged_page_names_a_range() {
        assert!(ranged("DateTime 2020-01-01T00:00:00Z-2020-02-01T00:00:00Z/*").range().is_some());
        assert!(PartialPage::Unranged.range().is_none());
    }
}
