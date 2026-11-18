//! The engine's admission of the clause 4.9 hops a `q` filter takes, as the
//! implementation of the hop admission port every `q` pipeline compiles its
//! filter through.

use crate::admission::{AdmissionError, LinkedOperation, LinkedReference, admit};
use andromeda_ngsi_ld_query::filter::{Filter, hop_admission::FilterHopAdmission};
use andromeda_ngsi_ld_representation::join_mode::JoinMode;
use std::num::NonZeroU32;

/// The hops of a `q` filter, held to the Linked Entity rule of one operation
/// under the request's `join` and `joinLevel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FilterHops {
    /// The operation the filter belongs to.
    operation: LinkedOperation,
    /// The request's clause 4.5.23 `join`, `None` when it names none.
    join: Option<JoinMode>,
    /// The request's clause 4.5.23 `joinLevel`, `None` when it names none.
    join_level: Option<NonZeroU32>,
}

impl FilterHops {
    /// The hops `operation` admits under `join` and `join_level`.
    #[must_use]
    pub const fn new(operation: LinkedOperation, join: Option<JoinMode>, join_level: Option<NonZeroU32>) -> FilterHops {
        FilterHops { operation, join, join_level }
    }
}

impl FilterHopAdmission for FilterHops {
    type Refusal = AdmissionError;

    fn admit_hops(&self, filter: &Filter) -> Result<(), AdmissionError> {
        LinkedReference::of_filter(Some(filter)).map_or(Ok(()), |reference| admit(self.operation, reference, self.join, self.join_level))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        admission::{AdmissionError, LinkedMember, LinkedOperation, ReachingMember, TemporalJoin},
        filter_hops::FilterHops,
    };
    use andromeda_ngsi_ld_query::filter::{Filter, hop_admission::FilterHopAdmission};
    use andromeda_ngsi_ld_representation::join_mode::JoinMode;
    use std::num::NonZeroU32;

    fn admitted(operation: LinkedOperation, join: Option<JoinMode>, join_level: Option<u32>, q: &str) -> Result<(), AdmissionError> {
        FilterHops::new(operation, join, join_level.and_then(NonZeroU32::new)).admit_hops(&q.parse::<Filter>().expect("the filter parses"))
    }

    fn depth(hops: u32) -> NonZeroU32 {
        NonZeroU32::new(hops).expect("a hop count above zero")
    }

    /// Clauses 5.7.2.4 and 5.14.4.4: a hop without a retrieving `join`
    /// reaches no Linked Entity.
    #[test]
    fn a_hop_without_join_is_refused_by_query_entities_and_entity_map_creation() {
        for operation in [LinkedOperation::QueryEntities, LinkedOperation::EntityMapQuery] {
            pretty_assertions::assert_eq!(
                admitted(operation, None, None, "r{a}==1"),
                Err(AdmissionError::WithoutJoin(ReachingMember::Filter)),
                "{operation}"
            );
        }
    }

    /// Clauses 5.7.2.4 and 5.14.4.4: a hop deeper than `joinLevel` is a too deep
    /// query.
    #[test]
    fn a_hop_deeper_than_join_level_is_refused() {
        pretty_assertions::assert_eq!(
            admitted(LinkedOperation::QueryEntities, Some(JoinMode::Flat), Some(1), "r{s{a}}==1"),
            Err(AdmissionError::DeeperThanJoinLevel {
                member: ReachingMember::Filter,
                depth: depth(2),
                join_level: depth(1),
            })
        );
        admitted(LinkedOperation::QueryEntities, Some(JoinMode::Flat), Some(2), "r{s{a}}==1").expect("two levels reach two hops");
    }

    /// Clause 5.7.4.4: a temporal query retrieves no Linked Entity, and
    /// with the temporal Linked Entity extension it follows the Core rule.
    #[test]
    fn a_temporal_query_refuses_a_hop_unless_the_extension_retrieves_it() {
        pretty_assertions::assert_eq!(
            admitted(LinkedOperation::QueryTemporal(TemporalJoin::Spec), Some(JoinMode::Flat), Some(1), "r{a}==1"),
            Err(AdmissionError::NotRetrievable {
                operation: LinkedOperation::QueryTemporal(TemporalJoin::Spec),
                member: LinkedMember::Reaching(ReachingMember::Filter),
            })
        );
        admitted(LinkedOperation::QueryTemporal(TemporalJoin::Extended), Some(JoinMode::Flat), Some(1), "r{a}==1")
            .expect("the extension retrieves a hop within its join");
    }

    /// Clause 5.8.1.4: no clause bounds a Subscription's hops. Clause
    /// 5.10.2.4 bounds none of a discovery query's either.
    #[test]
    fn a_subscription_and_discovery_admit_every_hop() {
        for operation in [LinkedOperation::Subscription, LinkedOperation::QueryRegistrations] {
            admitted(operation, None, None, "r{s{a}}==1").unwrap_or_else(|refusal| panic!("{operation}: {refusal}"));
        }
    }

    /// The temporal delete-by-query takes no `join`, so it refuses a hop as
    /// Purge Entities does (clause 5.6.21.4), and a filter without one passes.
    #[test]
    fn the_temporal_delete_by_query_refuses_a_hop() {
        pretty_assertions::assert_eq!(
            admitted(LinkedOperation::TemporalDeleteByQuery, None, None, "r{a}==1"),
            Err(AdmissionError::NotRetrievable {
                operation: LinkedOperation::TemporalDeleteByQuery,
                member: LinkedMember::Reaching(ReachingMember::Filter),
            })
        );
        admitted(LinkedOperation::TemporalDeleteByQuery, None, None, "a==1").expect("a filter without a hop is admitted");
    }
}
