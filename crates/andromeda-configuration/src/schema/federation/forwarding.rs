//! The `[federation.forwarding]` table: how requests are forwarded to Context
//! Sources.

use crate::{address_rule::AddressRule, human_duration::HumanDuration, schema::private_ip_blocking::PrivateIpBlocking};
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
        description = "Whether a context source whose endpoint resolves into private or reserved address space is contacted. Checked against the address actually resolved, not the host name. `allowed_addresses` exempts addresses from a `refused`, and `denied_addresses` refuses addresses whatever this is set to. A redirect from a context source is never followed, whatever this is set to."
    )]
    pub private_ip_blocking: PrivateIpBlocking,
    /// Addresses and subnets a Context Source may resolve into even when
    /// `private_ip_blocking` is `refused`.
    #[schemars(
        description = "IP addresses and subnets (`\"10.8.0.0/16\"`, `\"10.3.0.17\"`, `\"fd00:1234::/48\"`) that a context source may resolve into even though `private_ip_blocking` is `refused`. Empty by default. An entry in `denied_addresses` takes precedence."
    )]
    pub allowed_addresses: Vec<AddressRule>,
    /// Addresses and subnets a Context Source is never contacted at.
    #[schemars(
        description = "IP addresses and subnets (`\"203.0.113.0/24\"`, `\"198.51.100.9\"`) that a context source must not resolve into, public ones included and whatever `private_ip_blocking` is set to. Empty by default. Takes precedence over `allowed_addresses`."
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
}

impl Default for ForwardingConfig {
    fn default() -> ForwardingConfig {
        ForwardingConfig {
            max_concurrent: NonZeroUsize::MIN.saturating_add(15),
            connect_timeout: HumanDuration::from_secs(3),
            request_timeout: HumanDuration::from_secs(5),
            private_ip_blocking: PrivateIpBlocking::Allowed,
            allowed_addresses: Vec::new(),
            denied_addresses: Vec::new(),
            split_entities: SplitEntitiesDefault::Complete,
            max_query_entities: DEFAULT_MAX_QUERY_ENTITIES,
            max_query_source_rows: DEFAULT_MAX_QUERY_SOURCE_ROWS,
            max_query_instances: DEFAULT_MAX_QUERY_INSTANCES,
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
    use crate::schema::federation::forwarding::ForwardingConfig;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_query_budgets_default_to_ten_thousand_entities_thirty_thousand_rows_and_a_hundred_thousand_instances() {
        let config = ForwardingConfig::default();
        assert_eq!(config.max_query_entities.get(), 10_000);
        assert_eq!(config.max_query_source_rows.get(), 30_000);
        assert_eq!(config.max_query_instances.get(), 100_000);
    }
}
