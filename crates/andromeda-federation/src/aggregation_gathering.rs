//! How a distributed temporal query asking for an aggregated representation
//! gathers the instances of the entities held in more than one part.
//!
//! Clause 5.7.4.4 merges every part of an entity into the result set before it
//! renders "aggregated values of Attribute instances", so an aggregate one
//! holder computed over its own part is not the aggregate of the entity once
//! another holder has a part too. Such an entity is aggregated by this broker
//! over the instances of every part; the deployment chooses how they are
//! found.

use andromeda_configuration::schema::federation::query::FederatedAggregation;

/// How the entities a query aggregates here are found and their instances
/// gathered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggregationGathering {
    /// The sources are asked for every instance of the window, and every
    /// entity a source answers is aggregated here: one round trip, every
    /// instance moved.
    EveryInstance,
    /// The sources are asked for their aggregates; an entity only one part
    /// holds keeps its holder's aggregate, and the entities more than one part
    /// holds are asked again for their instances: a second round trip only
    /// when parts overlap, and only their instances moved.
    OverlapsAgain,
}

impl AggregationGathering {
    /// The gathering the deployment's `federation.query.aggregation` names.
    #[must_use]
    pub const fn configured(configured: FederatedAggregation) -> AggregationGathering {
        match configured {
            FederatedAggregation::Instances => AggregationGathering::EveryInstance,
            FederatedAggregation::OverlappingInstances => AggregationGathering::OverlapsAgain,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::aggregation_gathering::AggregationGathering;
    use andromeda_configuration::schema::federation::query::FederatedAggregation;

    #[test]
    fn each_configured_choice_names_its_gathering() {
        pretty_assertions::assert_eq!(
            AggregationGathering::configured(FederatedAggregation::Instances),
            AggregationGathering::EveryInstance
        );
        pretty_assertions::assert_eq!(
            AggregationGathering::configured(FederatedAggregation::OverlappingInstances),
            AggregationGathering::OverlapsAgain
        );
    }
}
