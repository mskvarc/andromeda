//! The compiled scope of a temporal history delete-by-query (an Andromeda
//! extension).
//!
//! This is the neutral input a store lowers to a windowed `DELETE` over the
//! attribute-history relation. It is deliberately separate from
//! [`CompiledTemporalQuery`](crate::temporal::compiled::CompiledTemporalQuery):
//! a query returns instances, a delete removes them, and the attribute axis is
//! not the query's positive `attrs` filter but a drop/keep choice that reuses
//! the semantics of clause 5.6.13's `deleteAll`/`datasetId` and of the standard
//! Purge (clause 5.6.21) `drop`/`keep`. The entity selection and the time
//! window are resolved by the ordinary temporal query machinery and reach the
//! store as resolved entity identifiers plus this scope.

use crate::{dataset::DatasetFilter, temporal::window::TemporalWindow};
use iri_rs::IriBuf;

/// Which attributes a temporal history delete removes instances of.
///
/// [`Drop`](AttributeDeleteScope::Drop) names the attributes to remove;
/// [`Keep`](AttributeDeleteScope::Keep) names the attributes to spare and
/// removes the rest; [`All`](AttributeDeleteScope::All) applies to every
/// attribute the entity carries. The IRIs are already context-expanded, so the
/// store compares them against the attribute-name dictionary directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttributeDeleteScope {
    /// Every attribute of the selected entities qualifies.
    All,
    /// Only the listed attributes are removed.
    Drop(Vec<IriBuf>),
    /// Every attribute except the listed ones is removed.
    Keep(Vec<IriBuf>),
}

/// The compiled scope of one temporal history delete-by-query.
///
/// The entity set is resolved separately (by the shared stage-1 selection) and
/// handed to the store beside this scope; everything here is the per-instance
/// predicate applied within those entities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemporalHistoryDeleteScope {
    /// Which attributes' instances are removed.
    pub attributes: AttributeDeleteScope,
    /// Which `datasetId` partitions are removed (clause 4.5.5).
    ///
    /// [`DatasetFilter::Unfiltered`] removes every instance regardless of
    /// `datasetId`, which is what both a bare delete and an explicit
    /// `deleteAll` reduce to.
    pub dataset: DatasetFilter,
    /// The slice of the time axis whose instances are removed.
    ///
    /// An unbounded window removes the whole history of the scoped attributes;
    /// a bounded one removes only the instances that fall in it.
    pub window: TemporalWindow,
    /// Whether the request asked for `deleteAll` (clause 5.6.13-equivalent).
    ///
    /// Only an unbounded `deleteAll` empties an attribute's stream entirely and
    /// so is allowed to remove the now-empty stream catalogue row; a bounded
    /// window leaves streams standing because later instants may still exist.
    pub delete_all: bool,
}

impl TemporalHistoryDeleteScope {
    /// Whether the delete empties the scoped attributes' streams outright, and
    /// so may remove the empty stream rows.
    ///
    /// True only for an unbounded window combined with `deleteAll`: a bounded
    /// window removes a slice and leaves the stream, and a bare unbounded delete
    /// does not carry the `deleteAll` intent that authorises removing the
    /// catalogue row (mirroring clause 5.6.13's `deleteAll`).
    #[must_use]
    pub const fn empties_streams(&self) -> bool {
        self.delete_all && !self.window.interval.is_bounded()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        dataset::DatasetFilter,
        temporal::{
            history_delete::{AttributeDeleteScope, TemporalHistoryDeleteScope},
            window::{TemporalWindow, TimeInterval},
        },
    };
    use andromeda_ngsi_ld_lexicon::{time::date_time::NgsiLdDateTime, time_prop::TimeProp};
    use chrono::{DateTime, Utc};

    fn instant(raw: &str) -> DateTime<Utc> {
        raw.parse::<NgsiLdDateTime>().expect("valid instant").get()
    }

    fn scope(interval: TimeInterval, delete_all: bool) -> TemporalHistoryDeleteScope {
        TemporalHistoryDeleteScope {
            attributes: AttributeDeleteScope::All,
            dataset: DatasetFilter::Unfiltered,
            window: TemporalWindow::builder().interval(interval).timeproperty(TimeProp::Observed).build(),
            delete_all,
        }
    }

    /// The empty-stream cleanup is exactly clause 5.6.13's `deleteAll` over the
    /// whole history, so it runs only when the window is unbounded.
    #[test]
    fn only_an_unbounded_delete_all_empties_streams() {
        assert!(scope(TimeInterval::Unbounded, true).empties_streams());
        assert!(!scope(TimeInterval::Unbounded, false).empties_streams());
        assert!(!scope(TimeInterval::After(instant("2024-01-01T00:00:00Z")), true).empties_streams());
    }
}
