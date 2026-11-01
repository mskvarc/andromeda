//! The `[federation]` tables: forwarding to Context Sources, what a federated
//! query means, caching their answers, holding back the failing ones, and
//! this broker's own identity.

pub mod cooldown;
pub mod forwarding;
pub mod identity;
pub mod query;

use crate::schema::federation::{cooldown::CooldownConfig, forwarding::ForwardingConfig, identity::IdentityConfig, query::FederatedQueryConfig};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;

/// The per-tenant cache of forwarded `GET` responses (clause 6.3.17).
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[schemars(description = "The per-tenant cache of forwarded `GET` responses.")]
pub struct ResponseCacheConfig {
    /// The most responses one tenant's cache holds.
    #[schemars(
        description = "The most forwarded `GET /entities` and `GET /entities/{id}` answers each tenant's cache holds, for registrations that set `management.cacheDuration`. An answer served from the cache carries a `110` warning. Zero is refused."
    )]
    pub max_entries: NonZeroUsize,
}

impl Default for ResponseCacheConfig {
    fn default() -> ResponseCacheConfig {
        ResponseCacheConfig {
            max_entries: NonZeroUsize::MIN.saturating_add(1023),
        }
    }
}

/// Forwarding to Context Sources, caching their answers, holding back the
/// failing ones, and this broker's own identity.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Default)]
#[serde(deny_unknown_fields)]
#[schemars(
    description = "How the broker forwards requests to the context sources its registrations name, and the identity it reports as a context source itself."
)]
pub struct FederationConfig {
    /// How requests are forwarded to Context Sources.
    #[schemars(description = "How requests reach context sources: fan-out, timeouts and the addresses a source may resolve into.")]
    pub forwarding: ForwardingConfig,
    /// What a federated query means and how much it gathers.
    #[schemars(
        description = "What a federated query means and how much it may gather from context sources: the `splitEntities` default, the result caps and how aggregated temporal reads are gathered."
    )]
    pub query: FederatedQueryConfig,
    /// The per-tenant cache of forwarded responses.
    #[schemars(description = "The per-tenant cache of forwarded `GET` responses.")]
    pub response_cache: ResponseCacheConfig,
    /// How a failing Context Source is held back. The cooldown schedule is
    /// checked by the forwarding layer that builds it.
    #[schemars(description = "How a context source is held back after a failed forwarded request.")]
    pub cooldown: CooldownConfig,
    /// The identity this broker reports as a Context Source.
    #[schemars(description = "The identity the broker reports at `/ngsi-ld/v1/info/sourceIdentity` and uses to detect forwarding loops.")]
    pub identity: IdentityConfig,
}
