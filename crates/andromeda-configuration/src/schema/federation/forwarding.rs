//! The `[federation.forwarding]` table: how requests are forwarded to Context
//! Sources.

use crate::human_duration::HumanDuration;
use serde::{Deserialize, Serialize};
use std::{
    num::{NonZeroU32, NonZeroUsize},
    time::Duration,
};
use thiserror::Error;
use toml_comment::TomlComment;

/// The default of [`ForwardingConfig::max_query_entities`]: ten times the
/// largest page the core API guarantees, so a hub serves any client page of a
/// federated collection of that size.
pub const DEFAULT_MAX_QUERY_ENTITIES: NonZeroU32 = NonZeroU32::MIN.saturating_add(9_999);

/// The default of [`ForwardingConfig::max_query_instances`]: ten instances per
/// entity of the default entity budget.
pub const DEFAULT_MAX_QUERY_INSTANCES: NonZeroU32 = NonZeroU32::MIN.saturating_add(99_999);

/// Whether a deployment assumes single entities may be split across context
/// sources when a request leaves `splitEntities` unset (clauses 4.3.6.7 and
/// 5.2.12: "default decided by implementation; it should be configurable").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SplitEntitiesDefault {
    /// Entities may be split: filters are applied only after re-assembly.
    Split,
    /// Every source holds complete entities: filters can be applied locally.
    Complete,
}

/// An unusable `[federation.forwarding]` table.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ForwardingConfigError {
    /// A forwarded request needs a positive deadline.
    #[error("federation.forwarding.request_timeout must be greater than zero")]
    ZeroRequestTimeout,
    /// Connecting to a source needs a positive deadline.
    #[error("federation.forwarding.connect_timeout must be greater than zero")]
    ZeroConnectTimeout,
}

/// How requests are forwarded to Context Sources.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ForwardingConfig {
    /// The most forwarded requests in flight at once.
    #[toml_comment(inline)]
    pub max_concurrent: NonZeroUsize,
    /// How long connecting to a Context Source may take, in human-readable
    /// form (`"3s"`).
    #[toml_comment(inline)]
    pub connect_timeout: HumanDuration,
    /// How long a forwarded request may take before it is taken to have
    /// failed, unless the registration's `management.timeout` says otherwise
    /// (clause 5.2.34), in human-readable form (`"5s"`).
    #[toml_comment(inline)]
    pub request_timeout: HumanDuration,
    /// What `splitEntities` means when a request or subscription leaves it
    /// unset (clauses 4.3.6.7, 5.2.12, 5.8.6, 5.14.4.4): `complete` or `split`.
    #[toml_comment(inline)]
    pub split_entities: SplitEntitiesDefault,
    /// The most entities one federated Query Entities gathers from the local
    /// store and every Context Source it walks together; a query whose
    /// answers exceed it is refused with `TooManyResults` (403, clause 4.12)
    /// rather than answered with a truncated collection.
    #[toml_comment(inline)]
    pub max_query_entities: NonZeroU32,
    /// The most attribute instances one federated temporal query gathers from
    /// the Context Sources whose history it reads to the end, which an
    /// aggregated representation of an entity held in several parts needs
    /// (clause 5.7.4.4); a query whose sources exceed it is refused with
    /// `TooManyResults` (403, clause 4.12).
    #[toml_comment(inline)]
    pub max_query_instances: NonZeroU32,
}

impl Default for ForwardingConfig {
    fn default() -> ForwardingConfig {
        ForwardingConfig {
            max_concurrent: NonZeroUsize::MIN.saturating_add(15),
            connect_timeout: HumanDuration::from_secs(3),
            request_timeout: HumanDuration::from_secs(5),
            split_entities: SplitEntitiesDefault::Complete,
            max_query_entities: DEFAULT_MAX_QUERY_ENTITIES,
            max_query_instances: DEFAULT_MAX_QUERY_INSTANCES,
        }
    }
}

impl ForwardingConfig {
    /// Validate the deadlines.
    ///
    /// # Errors
    ///
    /// Returns [`ForwardingConfigError::ZeroRequestTimeout`] or
    /// [`ForwardingConfigError::ZeroConnectTimeout`] for a zero deadline.
    pub const fn validate(&self) -> Result<(), ForwardingConfigError> {
        if self.request_timeout.is_zero() {
            return Err(ForwardingConfigError::ZeroRequestTimeout);
        }
        if self.connect_timeout.is_zero() {
            return Err(ForwardingConfigError::ZeroConnectTimeout);
        }
        Ok(())
    }

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
    use crate::{
        human_duration::HumanDuration,
        schema::federation::forwarding::{ForwardingConfig, ForwardingConfigError},
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn the_query_budgets_default_to_ten_thousand_entities_and_a_hundred_thousand_instances() {
        let config = ForwardingConfig::default();
        assert_eq!(config.max_query_entities.get(), 10_000);
        assert_eq!(config.max_query_instances.get(), 100_000);
        assert_eq!(config.validate(), Ok(()));
    }

    #[test]
    fn zero_timeouts_are_rejected() {
        let request = ForwardingConfig {
            request_timeout: HumanDuration::from_millis(0),
            ..ForwardingConfig::default()
        };
        assert_eq!(request.validate(), Err(ForwardingConfigError::ZeroRequestTimeout));
        let connect = ForwardingConfig {
            connect_timeout: HumanDuration::from_millis(0),
            ..ForwardingConfig::default()
        };
        assert_eq!(connect.validate(), Err(ForwardingConfigError::ZeroConnectTimeout));
    }
}
