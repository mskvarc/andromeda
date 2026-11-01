//! The `[federation.forwarding]` table: how requests are forwarded to Context
//! Sources.

use crate::{address_rule::AddressRule, human_duration::HumanDuration, schema::private_addresses::PrivateAddresses};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    num::{NonZeroU32, NonZeroUsize},
    time::Duration,
};

/// The default of [`ForwardingConfig::max_query_entities`]: ten times the
/// largest page the core API guarantees, so a hub serves any client page of a
/// federated collection of that size.
pub const DEFAULT_MAX_QUERY_ENTITIES: NonZeroU32 = NonZeroU32::MIN.saturating_add(9_999);

/// The default of [`ForwardingConfig::max_query_source_rows`]: three times the
/// default entity limit, so three sources may each answer every entity of it.
pub const DEFAULT_MAX_QUERY_SOURCE_ROWS: NonZeroU32 = NonZeroU32::MIN.saturating_add(29_999);

/// The default of [`ForwardingConfig::max_query_instances`]: ten instances per
/// entity of the default entity budget.
pub const DEFAULT_MAX_QUERY_INSTANCES: NonZeroU32 = NonZeroU32::MIN.saturating_add(99_999);

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

/// How requests are forwarded to Context Sources.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[schemars(description = "How requests are forwarded to context sources.")]
pub struct ForwardingConfig {
    /// The most forwarded requests in flight at once.
    #[schemars(description = "The most forwarded requests one operation has in flight at once. Zero is refused.")]
    pub max_concurrent: NonZeroUsize,
    /// How long connecting to a Context Source may take, in human-readable
    /// form (`"3s"`).
    #[schemars(description = "How long connecting to a context source may take.")]
    pub connect_timeout: HumanDuration,
    /// How long a forwarded request may take before it is taken to have
    /// failed, unless the registration's `management.timeout` says otherwise
    /// (clause 5.2.34), in human-readable form (`"5s"`).
    #[schemars(
        description = "How long a forwarded request may take before it counts as failed, connecting and reading the answer included. A registration's own `management.timeout` takes precedence."
    )]
    pub request_timeout: HumanDuration,
    /// Whether a Context Source whose endpoint resolves into private or
    /// reserved address space is `allowed` or `refused`. Checked against the
    /// address actually resolved. Redirects are never followed.
    #[schemars(
        description = "Whether a context source whose endpoint resolves into private or reserved address space is contacted. Checked against the address actually resolved, not the host name. `federation.forwarding.allowed_addresses` exempts addresses from `\"refused\"`, and `federation.forwarding.denied_addresses` refuses addresses whatever this is set to. A redirect from a context source is never followed, whatever this is set to."
    )]
    pub private_addresses: PrivateAddresses,
    /// Addresses and subnets a Context Source may resolve into even when
    /// `private_addresses` is `refused`.
    #[schemars(
        description = "IP addresses and subnets (`\"10.8.0.0/16\"`, `\"10.3.0.17\"`, `\"fd00:1234::/48\"`) that a context source may resolve into even though `federation.forwarding.private_addresses` is `\"refused\"`. Empty by default. An entry in `federation.forwarding.denied_addresses` takes precedence."
    )]
    pub allowed_addresses: Vec<AddressRule>,
    /// Addresses and subnets a Context Source is never contacted at.
    #[schemars(
        description = "IP addresses and subnets (`\"203.0.113.0/24\"`, `\"198.51.100.9\"`) that a context source must not resolve into, public ones included and whatever `federation.forwarding.private_addresses` is set to. Empty by default. Takes precedence over `federation.forwarding.allowed_addresses`."
    )]
    pub denied_addresses: Vec<AddressRule>,
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
        description = "The most distinct entities one federated query gathers from the local store and every context source together. An entity several sources answer counts once, and one only an auxiliary source answers counts too. A query that would gather more is refused with `403` `TooManyResults` rather than answered with a truncated collection. Zero is refused."
    )]
    pub max_query_entities: NonZeroU32,
    /// The most entities the Context Sources of one federated query send
    /// together, every source's copy of an entity counting, auxiliary sources
    /// included; a query whose sources send more is refused with
    /// `TooManyResults` (403, clause 4.12). At least `max_query_entities`.
    #[schemars(
        description = "The most entities the context sources of one federated query send together, duplicates included: an entity three sources answer counts three times, and auxiliary sources count too. A query whose sources send more is refused with `403` `TooManyResults`."
    )]
    pub max_query_source_rows: NonZeroU32,
    /// The most attribute instances one federated temporal query gathers from
    /// the Context Sources whose history it reads to the end, which an
    /// aggregated representation of an entity held in several parts needs
    /// (clause 5.7.4.4); a query whose sources exceed it is refused with
    /// `TooManyResults` (403, clause 4.12).
    #[schemars(
        description = "The most attribute instances one federated temporal query gathers from the context sources whose history it reads to the end, as an aggregated representation of an entity held in several parts needs. A query that would gather more is refused with `403` `TooManyResults`. Zero is refused."
    )]
    pub max_query_instances: NonZeroU32,
    /// How a distributed temporal query asking for an aggregated
    /// representation reads its Context Sources: `instances` or
    /// `overlapping-instances`.
    #[schemars(
        description = "How a distributed temporal query asking for `aggregatedValues` reads its context sources. An entity held by more than one part is aggregated by this broker over the instances of every part. `overlapping-instances` asks the sources for their aggregates and asks again only for the instances of the entities more than one part holds: cheapest when parts rarely overlap. `instances` asks for every instance in one round trip: faster when most entities are held by several sources, at the cost of moving and holding every instance."
    )]
    pub federated_aggregation: FederatedAggregation,
}

impl Default for ForwardingConfig {
    fn default() -> ForwardingConfig {
        ForwardingConfig {
            max_concurrent: NonZeroUsize::MIN.saturating_add(15),
            connect_timeout: HumanDuration::from_secs(3),
            request_timeout: HumanDuration::from_secs(5),
            private_addresses: PrivateAddresses::Allowed,
            allowed_addresses: Vec::new(),
            denied_addresses: Vec::new(),
            split_entities: SplitEntitiesDefault::Complete,
            max_query_entities: DEFAULT_MAX_QUERY_ENTITIES,
            max_query_source_rows: DEFAULT_MAX_QUERY_SOURCE_ROWS,
            max_query_instances: DEFAULT_MAX_QUERY_INSTANCES,
            federated_aggregation: FederatedAggregation::default(),
        }
    }
}

impl ForwardingConfig {
    /// The connection deadline.
    #[must_use]
    pub const fn connect_timeout(&self) -> Duration {
        self.connect_timeout.duration()
    }

    /// The default per-request deadline.
    #[must_use]
    pub const fn request_timeout(&self) -> Duration {
        self.request_timeout.duration()
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::federation::forwarding::{FederatedAggregation, ForwardingConfig};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_federated_aggregation_asks_again_only_for_overlapping_entities_by_default() {
        assert_eq!(ForwardingConfig::default().federated_aggregation, FederatedAggregation::OverlappingInstances);
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
        let config = ForwardingConfig::default();
        assert_eq!(config.max_query_entities.get(), 10_000);
        assert_eq!(config.max_query_source_rows.get(), 30_000);
        assert_eq!(config.max_query_instances.get(), 100_000);
    }
}
