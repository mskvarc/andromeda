//! The temporal query for the instance each stream held just before an
//! instant (clauses 4.11 and 5.7.4).
//!
//! The Scope event in force when a window opens is the latest placed before
//! its start, and it is asked of a holder with standard parameters alone:
//! `timerel=before` at the start, which clause 4.11 makes an exclusive bound,
//! and `lastN=1`, which Table 6.18.3.2-1 applies "per Attribute, per Entity".

use crate::query::temporal::TemporalQueryParameters;
use andromeda_ngsi_ld_lexicon::{time_prop::TimeProp, time_rel::TimeRel};
use chrono::{DateTime, Utc};
use std::num::NonZeroU32;

impl TemporalQueryParameters {
    /// The query for the latest instance of every attribute and the Scope
    /// placed on `timeproperty` before `start`. It selects no entity: the
    /// caller names the entities it asks about.
    #[must_use]
    pub fn latest_before(start: DateTime<Utc>, timeproperty: TimeProp) -> TemporalQueryParameters {
        TemporalQueryParameters {
            timerel: Some(TimeRel::Before),
            time_at: Some(start),
            timeproperty: Some(timeproperty),
            last_n: Some(NonZeroU32::MIN),
            ..TemporalQueryParameters::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::query::temporal::TemporalQueryParameters;
    use andromeda_ngsi_ld_lexicon::{time_prop::TimeProp, time_rel::TimeRel};
    use chrono::{DateTime, Utc};
    use std::num::NonZeroU32;

    fn noon() -> DateTime<Utc> {
        "2020-08-01T12:00:00Z".parse().expect("a valid instant")
    }

    /// Clause 4.11 with Table 6.18.3.2-1: the last instance before the start,
    /// on the Temporal Property asked for, and nothing else of a query.
    #[test]
    fn the_query_asks_for_the_last_instance_before_the_start() {
        let params = TemporalQueryParameters::latest_before(noon(), TimeProp::Deleted);
        pretty_assertions::assert_eq!(params.timerel, Some(TimeRel::Before));
        pretty_assertions::assert_eq!(params.time_at, Some(noon()));
        pretty_assertions::assert_eq!(params.timeproperty, Some(TimeProp::Deleted));
        pretty_assertions::assert_eq!(params.last_n, Some(NonZeroU32::MIN));
        pretty_assertions::assert_eq!(params.attrs, None);
        pretty_assertions::assert_eq!(params.dataset_id, None);
        pretty_assertions::assert_eq!(params.end_time_at, None);
        pretty_assertions::assert_eq!(params.id, None);
        pretty_assertions::assert_eq!(params.q, None);
    }
}
