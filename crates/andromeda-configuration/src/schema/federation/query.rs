//! The `[federation.query]` table: what a federated query means and how much
//! it may gather from the Context Sources it reaches.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

/// The default of [`FederatedQueryConfig::max_entities`]: ten times the
/// largest page the core API guarantees, so a hub serves any client page of a
/// federated collection of that size.
pub const DEFAULT_MAX_ENTITIES: NonZeroU32 = NonZeroU32::MIN.saturating_add(9_999);

/// The default of [`FederatedQueryConfig::max_source_rows`]: three times the
/// default entity limit, so three sources may each answer every entity of it.
pub const DEFAULT_MAX_SOURCE_ROWS: NonZeroU32 = NonZeroU32::MIN.saturating_add(29_999);

/// The default of [`FederatedQueryConfig::max_instances`]: ten instances per
/// entity of the default entity budget.
pub const DEFAULT_MAX_INSTANCES: NonZeroU32 = NonZeroU32::MIN.saturating_add(99_999);

/// Whether a deployment assumes single entities may be split across context
/// sources when a request leaves `splitEntities` unset (clauses 4.3.6.7 and
/// 5.2.12: "default decided by implementation; it should be configurable").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(description = "What a request or subscription that leaves `splitEntities` unset assumes about how context sources hold entities.")]
pub enum SplitEntitiesDefault {
    /// Entities may be split: filters are applied only after re-assembly.
    #[schemars(description = "A single entity may be split across context sources, so filters are applied only after its parts are re-assembled.")]
    Split,
    /// Every source holds complete entities: filters can be applied locally.
    #[schemars(description = "Every context source holds complete entities, so filters can be applied by each source.")]
    Complete,
}

/// How a distributed temporal query asking for an aggregated representation
/// reads its Context Sources.
///
/// Clause 5.7.4.4 aggregates an entity over the instances merged from every
/// part of it, so an entity held by more than one part is aggregated by this
/// broker over their instances. The two choices find those entities
/// differently and trade one round trip against the instances moved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(
    description = "How a distributed temporal query asking for `aggregatedValues` reads its context sources. An entity held by more than one part is always aggregated by this broker over the instances of every part; the choices differ in how those entities are found."
)]
pub enum FederatedAggregation {
    /// The sources are asked for every instance of the window and every
    /// entity they answer is aggregated here.
    #[schemars(
        description = "Ask the context sources for every instance of the queried window and aggregate every entity they answer here. One round trip, but every instance travels to and is held by this broker. Suits deployments where most entities are held by several sources."
    )]
    Instances,
    /// The sources are asked for their aggregates, and only the entities more
    /// than one part answered are asked again for their instances.
    #[default]
    #[schemars(
        description = "Ask the context sources for their aggregates, keep the aggregate of an entity only one part holds, and ask again for the instances of the entities more than one part holds. A second round trip only when parts overlap, and only their instances travel."
    )]
    OverlappingInstances,
}

/// What a federated query means and how much it may gather.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[schemars(deny_unknown_fields)]
#[schemars(description = "What a federated query means and how much it may gather from context sources.")]
pub struct FederatedQueryConfig {
    /// What `splitEntities` means when a request or subscription leaves it
    /// unset (clauses 4.3.6.7, 5.2.12, 5.8.6, 5.14.4.4): `complete` or `split`.
    #[schemars(description = "What `splitEntities` means when a request or subscription leaves it unset.")]
    pub split_entities: SplitEntitiesDefault,
    /// The most distinct entities one federated query gathers from the local
    /// store and every Context Source it walks together, an entity several
    /// sources answer counting once, one only an auxiliary source answers
    /// included; a query whose answers exceed it is refused with
    /// `TooManyResults` (403, clause 4.12) rather than answered with a
    /// truncated collection.
    #[schemars(
        description = "The most distinct entities one federated query gathers from the local store and every context source together. An entity several sources answer counts once, and one only an auxiliary source answers counts too. A query that would gather more is refused with `403` `TooManyResults` rather than answered with a truncated collection."
    )]
    pub max_entities: NonZeroU32,
    /// The most entities the Context Sources of one federated query send
    /// together, every source's copy of an entity counting, auxiliary sources
    /// included; a query whose sources send more is refused with
    /// `TooManyResults` (403, clause 4.12). At least `max_entities`.
    #[schemars(
        description = "The most entities the context sources of one federated query send together, duplicates included: an entity three sources answer counts three times, and auxiliary sources count too. A query whose sources send more is refused with `403` `TooManyResults`."
    )]
    pub max_source_rows: NonZeroU32,
    /// The most attribute instances one federated temporal query gathers from
    /// the Context Sources whose history it reads to the end, which an
    /// aggregated representation of an entity held in several parts needs
    /// (clause 5.7.4.4); a query whose sources exceed it is refused with
    /// `TooManyResults` (403, clause 4.12).
    #[schemars(
        description = "The most attribute instances one federated temporal query gathers from the context sources whose history it reads to the end, as an aggregated representation of an entity held in several parts needs. A query that would gather more is refused with `403` `TooManyResults`."
    )]
    pub max_instances: NonZeroU32,
    /// How a distributed temporal query asking for an aggregated
    /// representation reads its Context Sources: `instances` or
    /// `overlapping-instances`.
    #[schemars(
        description = "How a distributed temporal query asking for `aggregatedValues` reads its context sources. An entity held by more than one part is aggregated by this broker over the instances of every part. `\"overlapping-instances\"` asks the sources for their aggregates and asks again only for the instances of the entities more than one part holds: cheapest when parts rarely overlap. `\"instances\"` asks for every instance in one round trip: faster when most entities are held by several sources, at the cost of moving and holding every instance."
    )]
    pub aggregation: FederatedAggregation,
}

impl Default for FederatedQueryConfig {
    fn default() -> FederatedQueryConfig {
        FederatedQueryConfig {
            split_entities: SplitEntitiesDefault::Complete,
            max_entities: DEFAULT_MAX_ENTITIES,
            max_source_rows: DEFAULT_MAX_SOURCE_ROWS,
            max_instances: DEFAULT_MAX_INSTANCES,
            aggregation: FederatedAggregation::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::federation::query::{FederatedAggregation, FederatedQueryConfig, SplitEntitiesDefault};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_federated_aggregation_asks_again_only_for_overlapping_entities_by_default() {
        assert_eq!(FederatedQueryConfig::default().aggregation, FederatedAggregation::OverlappingInstances);
    }

    #[test]
    fn an_unset_split_entities_means_complete_entities_by_default() {
        assert_eq!(FederatedQueryConfig::default().split_entities, SplitEntitiesDefault::Complete);
    }

    #[test]
    fn each_federated_aggregation_is_spelled_in_kebab_case() {
        let spelled = [
            (FederatedAggregation::Instances, "\"instances\""),
            (FederatedAggregation::OverlappingInstances, "\"overlapping-instances\""),
        ];
        for (choice, spelling) in spelled {
            assert_eq!(serde_json::to_string(&choice).ok().as_deref(), Some(spelling));
        }
    }

    #[test]
    fn the_query_budgets_default_to_ten_thousand_entities_thirty_thousand_rows_and_a_hundred_thousand_instances() {
        let config = FederatedQueryConfig::default();
        assert_eq!(config.max_entities.get(), 10_000);
        assert_eq!(config.max_source_rows.get(), 30_000);
        assert_eq!(config.max_instances.get(), 100_000);
    }
}
