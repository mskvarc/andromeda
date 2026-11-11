//! How long the broker holds a posted query, and how many one tenant keeps.
//!
//! Holding a posted query turns a read into a write, so the quota is what
//! keeps a client that may only read from filling its tenant's storage.

use andromeda_configuration::{human_byte_size::HumanByteSize, schema::posted_query::PostedQueryConfig};
use std::{num::NonZeroUsize, time::Duration};

/// The ceilings one tenant's posted queries are kept within.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostedQueryQuota {
    /// The most posted queries one tenant keeps; `None` keeps any number.
    pub max_count: Option<NonZeroUsize>,
    /// The most bytes one tenant's posted queries take in storage; `None` lets
    /// them take any amount.
    pub max_bytes: Option<u64>,
}

/// How long a posted query is held, and the quota one tenant's are kept
/// within.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostedQueryRetention {
    /// How long a posted query is held after the last POST that sent it;
    /// following a page link does not extend it.
    pub lifetime: Duration,
    /// The ceilings one tenant's posted queries are kept within.
    pub quota: PostedQueryQuota,
}

impl From<&PostedQueryConfig> for PostedQueryRetention {
    fn from(config: &PostedQueryConfig) -> PostedQueryRetention {
        PostedQueryRetention {
            lifetime: config.lifetime.duration(),
            quota: PostedQueryQuota {
                max_count: config.quota.max_per_tenant.limit(),
                max_bytes: config.quota.max_storage_per_tenant.limit().map(HumanByteSize::as_u64),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::retention::{PostedQueryQuota, PostedQueryRetention};
    use andromeda_configuration::schema::{
        ceiling::{Ceiling, Unlimited},
        posted_query::PostedQueryConfig,
    };
    use pretty_assertions::assert_eq;
    use std::{num::NonZeroUsize, time::Duration};

    #[test]
    fn the_default_table_holds_a_query_an_hour_within_finite_quotas() {
        assert_eq!(
            PostedQueryRetention::from(&PostedQueryConfig::default()),
            PostedQueryRetention {
                lifetime: Duration::from_secs(3_600),
                quota: PostedQueryQuota {
                    max_count: NonZeroUsize::new(10_000),
                    max_bytes: Some(268_435_456),
                },
            }
        );
    }

    #[test]
    fn an_unlimited_quota_sets_no_ceiling() {
        let mut config = PostedQueryConfig::default();
        config.quota.max_per_tenant = Ceiling::Unlimited(Unlimited::Unlimited);
        config.quota.max_storage_per_tenant = Ceiling::Unlimited(Unlimited::Unlimited);
        assert_eq!(
            PostedQueryRetention::from(&config).quota,
            PostedQueryQuota {
                max_count: None,
                max_bytes: None,
            }
        );
    }
}
