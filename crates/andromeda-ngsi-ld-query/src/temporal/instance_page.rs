//! How many instances of each attribute a temporal read returns, and in which
//! direction clause 6.3.10 pages through the rest.

use crate::temporal::period_budget::PeriodBudget;
use chrono::{DateTime, Utc};
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

impl PageDirection {
    /// Whether a page cut at `edge` holds an instance at `instant`: at or
    /// before the edge forwards, at or after it backwards. The instances at
    /// the edge itself are held, since a page never splits the instances one
    /// stream holds at the instant it is cut at.
    #[must_use]
    pub fn holds(self, edge: DateTime<Utc>, instant: DateTime<Utc>) -> bool {
        match self {
            PageDirection::Forward => instant <= edge,
            PageDirection::Backward { .. } => instant >= edge,
        }
    }

    /// Of the edges two pages were cut at, the one reached least far in this
    /// direction: the earlier forwards, the later backwards. Both pages hold
    /// every instance on its held side.
    #[must_use]
    pub fn nearer(self, left: DateTime<Utc>, right: DateTime<Utc>) -> DateTime<Utc> {
        match self {
            PageDirection::Forward => left.min(right),
            PageDirection::Backward { .. } => left.max(right),
        }
    }
}

/// Which instances of every attribute stream a temporal read keeps.
///
/// Six readings, because they differ in more than a number: a complete answer
/// is never a 206, and only a page whose remainder the client must fetch keeps
/// the instances that share the timestamp at its edge together, so the resumed
/// request neither skips nor splits them.
///
/// An instance representation pages by instances and an aggregated one by
/// whole periods (clause 4.5.19), the element each of them holds. A period
/// page read in an instance representation counts its `periods` as instances,
/// and an instance page read in the aggregated one counts its `size` as
/// periods with no bound on the instances they aggregate, so every reading is
/// total over both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InstancePage {
    /// Every instance in the window: an aggregation over the whole time range
    /// (clause 4.5.19.1), which is one period and so never paged.
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
    /// An aggregated representation without `lastN`: the least recent whole
    /// periods of every attribute the budget admits, the rest reached by
    /// paging forwards.
    Periods {
        /// The periods and instances one page holds at most per stream.
        budget: PeriodBudget,
    },
    /// An aggregated representation with a `lastN` above the broker's maximum:
    /// the most recent whole periods of the last `requested` instances of
    /// every attribute the budget admits, the rest reached by paging
    /// backwards.
    PeriodsBackward {
        /// The periods and instances one page holds at most per stream.
        budget: PeriodBudget,
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
            InstancePage::Forward { .. } | InstancePage::Periods { .. } => Some(PageDirection::Forward),
            InstancePage::Backward { requested, .. } | InstancePage::PeriodsBackward { requested, .. } => Some(PageDirection::Backward { requested }),
        }
    }

    /// The same selection read as one complete answer: every instance, or the
    /// last `lastN` of them.
    ///
    /// A read whose result is aggregated again with instances held elsewhere
    /// cannot stop at a page of its own, since the aggregate is over every
    /// part together (clause 5.7.4.4).
    #[must_use]
    pub const fn unpaged(self) -> InstancePage {
        match self {
            InstancePage::Whole | InstancePage::Forward { .. } | InstancePage::Periods { .. } => InstancePage::Whole,
            InstancePage::LastN { n } => InstancePage::LastN { n },
            InstancePage::Backward { requested, .. } | InstancePage::PeriodsBackward { requested, .. } => InstancePage::LastN { n: requested },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::temporal::{
        instance_page::{InstancePage, PageDirection},
        period_budget::PeriodBudget,
    };
    use chrono::{DateTime, TimeZone, Utc};
    use std::num::{NonZeroU32, NonZeroU64};

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
        pretty_assertions::assert_eq!(InstancePage::Periods { budget: budget() }.direction(), Some(PageDirection::Forward));
        pretty_assertions::assert_eq!(
            InstancePage::PeriodsBackward {
                budget: budget(),
                requested: count(5000)
            }
            .direction(),
            Some(PageDirection::Backward { requested: count(5000) })
        );
    }

    fn hour(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2020, 1, 1, hour, 0, 0).single().expect("a valid instant")
    }

    fn backward() -> PageDirection {
        PageDirection::Backward { requested: count(5000) }
    }

    /// Clause 6.3.10: a forward page holds what lies at or before its edge, a
    /// backward one what lies at or after it, the edge itself on both.
    #[test]
    fn a_page_holds_the_side_of_its_edge_it_was_read_from() {
        let edge = hour(5);
        let held = |direction: PageDirection| [hour(4), hour(5), hour(6)].map(|instant| direction.holds(edge, instant));
        pretty_assertions::assert_eq!(held(PageDirection::Forward), [true, true, false]);
        pretty_assertions::assert_eq!(held(backward()), [false, true, true]);
    }

    /// Forwards the earlier edge is reached least far, backwards the later.
    #[test]
    fn the_nearer_edge_is_the_one_reached_least_far() {
        pretty_assertions::assert_eq!(PageDirection::Forward.nearer(hour(7), hour(5)), hour(5));
        pretty_assertions::assert_eq!(backward().nearer(hour(5), hour(7)), hour(7));
    }

    /// One thousand periods of at most a million instances.
    fn budget() -> PeriodBudget {
        PeriodBudget {
            periods: 1000,
            instances: NonZeroU64::new(1_000_000).expect("a positive budget"),
        }
    }

    /// Clause 5.7.4.4 aggregates an entity held in parts over every part
    /// together, so the read of one part keeps the whole selection: a forward
    /// page becomes the window and a backward one the `lastN` it pages.
    #[test]
    fn an_unpaged_read_keeps_the_window_or_the_requested_last_n() {
        pretty_assertions::assert_eq!(InstancePage::Periods { budget: budget() }.unpaged(), InstancePage::Whole);
        pretty_assertions::assert_eq!(InstancePage::Forward { size: 100 }.unpaged(), InstancePage::Whole);
        pretty_assertions::assert_eq!(InstancePage::LastN { n: count(5) }.unpaged(), InstancePage::LastN { n: count(5) });
        pretty_assertions::assert_eq!(
            InstancePage::PeriodsBackward {
                budget: budget(),
                requested: count(5000)
            }
            .unpaged(),
            InstancePage::LastN { n: count(5000) }
        );
        pretty_assertions::assert_eq!(
            InstancePage::Backward {
                size: 1000,
                requested: count(5000)
            }
            .unpaged(),
            InstancePage::LastN { n: count(5000) }
        );
    }
}
