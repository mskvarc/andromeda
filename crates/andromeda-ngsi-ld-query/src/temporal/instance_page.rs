//! How many instances of each attribute a temporal read returns, and in which
//! direction clause 6.3.10 pages through the rest.

use std::num::NonZeroU32;

/// The direction a partial temporal response is paged in (clause 6.3.10).
///
/// Clause 6.3.10: "for requests where the parameter lastN is present,
/// pagination shall happen 'backwards' (from the most recent to the least
/// recent timestamp in the requested time range). For requests without the
/// parameter lastN, pagination shall happen 'forwards' (from the least recent
/// to the most recent timestamp in the requested time range)."
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageDirection {
    /// No `lastN`: the least recent instances come first.
    Forward,
    /// A `lastN` the broker's maximum narrowed: the most recent instances come
    /// first.
    Backward {
        /// The `lastN` the client asked for, which clause 6.3.10 reports as the
        /// `Content-Range` `size` ("shall be equal to the requested lastN")
        /// rather than the number of instances the page holds.
        requested: NonZeroU32,
    },
}

/// Which instances of every attribute stream a temporal read keeps.
///
/// Four readings, because they differ in more than a number: a complete answer
/// is never a 206, and only a page whose remainder the client must fetch keeps
/// the instances that share the timestamp at its edge together, so the resumed
/// request neither skips nor splits them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InstancePage {
    /// Every instance in the window. Aggregated representations are bounded by
    /// their aggregation period (clause 4.5.19) rather than by a count.
    #[default]
    Whole,
    /// A client `lastN` within the broker's maximum: exactly the last `n`
    /// instances (clauses 5.7.3.4 and 5.7.4.4), which is the complete answer to
    /// the request and never a partial response.
    LastN {
        /// The number of instances the client asked for.
        n: NonZeroU32,
    },
    /// No client `lastN`: the least recent `size` instances of every attribute,
    /// the broker's default page (clause 6.3.10 "a default limit is enforced").
    Forward {
        /// The number of instances the page counts per attribute stream.
        size: u32,
    },
    /// A client `lastN` above the broker's maximum: the most recent `size`
    /// instances of every attribute, the rest reached by paging backwards.
    Backward {
        /// The number of instances the page counts per attribute stream.
        size: u32,
        /// The `lastN` the client asked for.
        requested: NonZeroU32,
    },
}

impl InstancePage {
    /// The direction the remainder is paged in, or `None` when the read returns
    /// a complete answer and cannot be partial.
    #[must_use]
    pub const fn direction(self) -> Option<PageDirection> {
        match self {
            InstancePage::Whole | InstancePage::LastN { .. } => None,
            InstancePage::Forward { .. } => Some(PageDirection::Forward),
            InstancePage::Backward { requested, .. } => Some(PageDirection::Backward { requested }),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::temporal::instance_page::{InstancePage, PageDirection};
    use std::num::NonZeroU32;

    /// `n` as a client `lastN`.
    fn count(n: u32) -> NonZeroU32 {
        NonZeroU32::new(n).expect("a positive count")
    }

    /// Clause 6.3.10: only a page the broker narrowed is partial, forwards
    /// without `lastN` and backwards with it.
    #[test]
    fn only_a_narrowed_page_has_a_paging_direction() {
        pretty_assertions::assert_eq!(InstancePage::Whole.direction(), None);
        pretty_assertions::assert_eq!(InstancePage::LastN { n: count(5) }.direction(), None);
        pretty_assertions::assert_eq!(InstancePage::Forward { size: 100 }.direction(), Some(PageDirection::Forward));
        pretty_assertions::assert_eq!(
            InstancePage::Backward {
                size: 1000,
                requested: count(5000)
            }
            .direction(),
            Some(PageDirection::Backward { requested: count(5000) })
        );
    }
}
