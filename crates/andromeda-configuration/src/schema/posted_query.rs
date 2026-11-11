//! The `[posted_query]` table: how long the broker holds a paged query sent by
//! POST so the page links of its result can name it, and how many one tenant
//! may keep.

use crate::{human_byte_size::HumanByteSize, human_duration::HumanDuration, schema::ceiling::Ceiling};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;

/// How many posted queries, and how much storage, one tenant keeps;
/// `"unlimited"` lifts a quota.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[schemars(
    description = "How many queries sent by POST, and how much storage, one tenant keeps. Over a quota, the kept queries expiring soonest are deleted first, never the one just sent. A page link naming a deleted query is refused with `BadRequestData`."
)]
#[schemars(deny_unknown_fields)]
pub struct PostedQueryQuotaConfig {
    /// The most posted queries one tenant keeps, or `"unlimited"`.
    #[schemars(description = "The most queries sent by POST one tenant keeps, or `\"unlimited\"` to keep any number.")]
    pub max_per_tenant: Ceiling<NonZeroUsize>,
    /// The most storage one tenant's posted queries take, in human-readable
    /// form (`"256 MiB"`), or `"unlimited"`; at least `request_limits.max_body`.
    #[schemars(
        description = "The most storage one tenant's kept queries take, or `\"unlimited\"` to let them take any amount. The largest query a client may send fits within it."
    )]
    pub max_storage_per_tenant: Ceiling<HumanByteSize>,
}

impl Default for PostedQueryQuotaConfig {
    fn default() -> PostedQueryQuotaConfig {
        PostedQueryQuotaConfig {
            max_per_tenant: Ceiling::At(NonZeroUsize::MIN.saturating_add(9_999)),
            max_storage_per_tenant: Ceiling::At(HumanByteSize::mib(256)),
        }
    }
}

/// How long a posted query is held, and how many one tenant keeps.
///
/// A paged query sent by POST travels in the request body, so the page links
/// of its result, which a client follows with GET, name the query the broker
/// holds instead of carrying it. Holding them turns a read into a write, so the
/// quota is finite by default: it is what keeps a client that may only read
/// from filling the tenant's storage.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[schemars(
    description = "How long the broker keeps a paged query sent by POST, so the `next` and `prev` links of its result can name it, and how many one tenant may keep. A link names the kept query instead of carrying it, and is refused with `BadRequestData` once the query is gone."
)]
#[schemars(deny_unknown_fields)]
pub struct PostedQueryConfig {
    /// How long a posted query is held after the last POST that sent it, in
    /// human-readable form (`"1h"`).
    #[schemars(description = "How long a query sent by POST is kept after the last POST that sent it. Following a page link does not extend it.")]
    pub lifetime: HumanDuration,
    /// How many posted queries one tenant keeps.
    #[schemars(description = "How many queries sent by POST, and how much storage, one tenant keeps.")]
    pub quota: PostedQueryQuotaConfig,
}

impl Default for PostedQueryConfig {
    fn default() -> PostedQueryConfig {
        PostedQueryConfig {
            lifetime: HumanDuration::from_secs(3_600),
            quota: PostedQueryQuotaConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{human_byte_size::HumanByteSize, human_duration::HumanDuration, schema::posted_query::PostedQueryConfig};
    use pretty_assertions::assert_eq;
    use std::num::NonZeroUsize;

    #[test]
    fn the_defaults_hold_a_query_an_hour_within_finite_quotas() {
        let config = PostedQueryConfig::default();
        assert_eq!(config.lifetime, HumanDuration::from_secs(3_600));
        assert_eq!(config.quota.max_per_tenant.limit().map(NonZeroUsize::get), Some(10_000));
        assert_eq!(config.quota.max_storage_per_tenant.limit(), Some(HumanByteSize::mib(256)));
    }

    #[test]
    fn a_quota_can_be_lifted() {
        let config: PostedQueryConfig =
            toml::from_str("lifetime = \"30m\"\n[quota]\nmax_per_tenant = \"unlimited\"\nmax_storage_per_tenant = \"unlimited\"\n").expect("a table");
        assert_eq!(config.lifetime, HumanDuration::from_secs(1_800));
        assert_eq!(config.quota.max_per_tenant.limit(), None);
        assert_eq!(config.quota.max_storage_per_tenant.limit(), None);
    }
}
