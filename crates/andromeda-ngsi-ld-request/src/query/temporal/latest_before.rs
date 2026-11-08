//! The temporal query for the instance each stream held just before an
//! instant (clauses 4.11 and 5.7.4).
//!
//! The instance in effect when a window opens is the latest placed before its
//! start, and it is asked of a holder with standard parameters alone:
//! `timerel=before` at the start, which clause 4.11 makes an exclusive bound,
//! and `lastN=1`, which Table 6.18.3.2-1 applies "per Attribute, per Entity".
//! The `datasetId` filter runs before the `lastN` cut (clause 5.7.4.4, step S8
//! ahead of the instance selection), so naming one dataset reads that
//! dataset's latest instance.

use crate::query::temporal::TemporalQueryParameters;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, time_prop::TimeProp, time_rel::TimeRel};
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use chrono::{DateTime, Utc};
use std::num::NonZeroU32;

impl TemporalQueryParameters {
    /// The query for the latest instance of each of `attrs`, every attribute
    /// when `None`, placed on `timeproperty` before `start`, in `dataset`
    /// alone when one is named. It selects no entity: the caller names the
    /// entities it asks about.
    #[must_use]
    pub fn latest_before(
        start: DateTime<Utc>,
        timeproperty: TimeProp,
        attrs: Option<Vec<AttributeName>>,
        dataset: Option<DatasetSelector>,
    ) -> TemporalQueryParameters {
        TemporalQueryParameters {
            attrs,
            timerel: Some(TimeRel::Before),
            time_at: Some(start),
            timeproperty: Some(timeproperty),
            last_n: Some(NonZeroU32::MIN),
            dataset_id: dataset.map(|dataset| vec![dataset]),
            ..TemporalQueryParameters::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::query::temporal::TemporalQueryParameters;
    use andromeda_ngsi_ld_lexicon::{time_prop::TimeProp, time_rel::TimeRel};
    use andromeda_ngsi_ld_query::dataset::DatasetSelector;
    use chrono::{DateTime, Utc};
    use std::num::NonZeroU32;

    fn noon() -> DateTime<Utc> {
        "2020-08-01T12:00:00Z".parse().expect("a valid instant")
    }

    /// Clause 4.11 with Table 6.18.3.2-1: the last instance before the start,
    /// on the Temporal Property asked for, of the attributes named, and
    /// nothing else of a query.
    #[test]
    fn the_query_asks_for_the_last_instance_before_the_start() {
        let attrs = vec!["speed".parse().expect("a valid attribute name")];
        let params = TemporalQueryParameters::latest_before(noon(), TimeProp::Deleted, Some(attrs.clone()), None);
        pretty_assertions::assert_eq!(params.timerel, Some(TimeRel::Before));
        pretty_assertions::assert_eq!(params.time_at, Some(noon()));
        pretty_assertions::assert_eq!(params.timeproperty, Some(TimeProp::Deleted));
        pretty_assertions::assert_eq!(params.last_n, Some(NonZeroU32::MIN));
        pretty_assertions::assert_eq!(params.attrs, Some(attrs));
        pretty_assertions::assert_eq!(params.dataset_id, None);
        pretty_assertions::assert_eq!(params.end_time_at, None);
        pretty_assertions::assert_eq!(params.id, None);
        pretty_assertions::assert_eq!(params.q, None);
    }

    /// Clause 4.5.5: a query naming one dataset reads that dataset alone, the
    /// default instance as `@none`.
    #[test]
    fn a_named_dataset_is_the_only_one_asked_about() {
        let params = TemporalQueryParameters::latest_before(noon(), TimeProp::Observed, None, Some(DatasetSelector::Default));
        pretty_assertions::assert_eq!(params.dataset_id, Some(vec![DatasetSelector::Default]));
        pretty_assertions::assert_eq!(params.attrs, None);
    }
}
