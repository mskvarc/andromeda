//! Federation operation configuration.

use andromeda_configuration_scalars::human_duration::HumanDuration;
use serde::{Deserialize, Serialize};
use std::{num::NonZeroU32, time::Duration};
use thiserror::Error;
use toml_comment::TomlComment;

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

impl SplitEntitiesDefault {
    /// Resolve a request's explicit `splitEntities` against this default.
    #[must_use]
    pub const fn resolve(self, explicit: Option<bool>) -> SplitEntities {
        match explicit {
            Some(true) => SplitEntities::Split,
            Some(false) => SplitEntities::Complete,
            None => match self {
                SplitEntitiesDefault::Split => SplitEntities::Split,
                SplitEntitiesDefault::Complete => SplitEntities::Complete,
            },
        }
    }
}

/// What one request or subscription resolved `splitEntities` to (clauses
/// 4.3.6.7, 5.7.2.4 and 5.7.4.4).
///
/// Under [`SplitEntities::Split`] no single source can be assumed to hold a
/// whole entity, so the filters that read attributes (`q`, the geoquery, the
/// scope query and the attribute list) are removed before forwarding and
/// applied once to the aggregated entities; under
/// [`SplitEntities::Complete`] every source holds complete entities and the
/// filters travel with the forwarded request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitEntities {
    /// Entities may be distributed across sources.
    Split,
    /// Every source holds complete entities.
    Complete,
}

impl SplitEntities {
    /// Whether filters are deferred until after the aggregation.
    #[must_use]
    pub const fn is_split(self) -> bool {
        matches!(self, SplitEntities::Split)
    }
}

/// Invalid federation configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum FederationConfigError {
    /// Forwarding requires at least one execution slot.
    #[error("federation.forward_concurrency must be at least 1")]
    ZeroForwardConcurrency,
    /// A forwarded request needs a positive deadline.
    #[error("federation.request_timeout must be greater than zero")]
    ZeroRequestTimeout,
    /// Connecting to a source needs a positive deadline.
    #[error("federation.connect_timeout must be greater than zero")]
    ZeroConnectTimeout,
}

/// The default of [`FederationConfig::max_query_entities`]: ten times the
/// largest page the core API guarantees, so a hub serves any client page of a
/// federated collection of that size.
pub const DEFAULT_MAX_QUERY_ENTITIES: NonZeroU32 = NonZeroU32::MIN.saturating_add(9_999);

/// The default of [`FederationConfig::max_query_instances`]: ten instances per
/// entity of the default entity budget.
pub const DEFAULT_MAX_QUERY_INSTANCES: NonZeroU32 = NonZeroU32::MIN.saturating_add(99_999);

/// Distributed Context Source forwarding limits.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone)]
pub struct FederationConfig {
    /// Maximum parallel Context Source forwarding requests.
    pub forward_concurrency: usize,
    /// Time allowed to establish a connection to a Context Source, in human-readable form (`"3s"`).
    #[toml_comment(inline)]
    pub connect_timeout: HumanDuration,
    /// Time a forwarded request may take before it is assumed to have failed, unless the registration's `management.timeout` says otherwise (clause 5.2.34), in human-readable form (`"5s"`, `"500ms"`).
    #[toml_comment(inline)]
    pub request_timeout: HumanDuration,
    /// Most cached forwarded GET responses held per tenant (clause 6.3.17).
    pub cache_max_entries: usize,
    /// What `splitEntities` means when a request or subscription leaves it
    /// unset (clauses 4.3.6.7, 5.2.12, 5.8.6, 5.14.4.4).
    #[toml_comment(inline)]
    pub split_entities_default: SplitEntitiesDefault,
    /// Most entities one federated Query Entities gathers from the local store
    /// and every Context Source it walks together; a query whose answers
    /// exceed it is refused with `TooManyResults` (403, clause 4.12) rather than
    /// answered with a truncated collection.
    #[toml_comment(inline)]
    pub max_query_entities: NonZeroU32,
    /// Most attribute instances one federated temporal query gathers from the
    /// Context Sources whose history it reads to the end, which an aggregated
    /// representation of an entity held in several parts needs (clause
    /// 5.7.4.4); a query whose sources exceed it is refused with
    /// `TooManyResults` (403, clause 4.12).
    #[toml_comment(inline)]
    pub max_query_instances: NonZeroU32,
}

impl Default for FederationConfig {
    fn default() -> FederationConfig {
        FederationConfig {
            forward_concurrency: 16,
            connect_timeout: HumanDuration::from_millis(3000),
            request_timeout: HumanDuration::from_millis(5000),
            cache_max_entries: 1024,
            split_entities_default: SplitEntitiesDefault::Complete,
            max_query_entities: DEFAULT_MAX_QUERY_ENTITIES,
            max_query_instances: DEFAULT_MAX_QUERY_INSTANCES,
        }
    }
}

impl FederationConfig {
    /// Validate Context Source forwarding limits.
    ///
    /// # Errors
    ///
    /// Returns [`FederationConfigError::ZeroForwardConcurrency`] when no
    /// forwarding slot is allowed, [`FederationConfigError::ZeroRequestTimeout`]
    /// when the per-request deadline is zero, and
    /// [`FederationConfigError::ZeroConnectTimeout`] when the connection
    /// deadline is zero.
    pub const fn validate(&self) -> Result<(), FederationConfigError> {
        if self.forward_concurrency == 0 {
            return Err(FederationConfigError::ZeroForwardConcurrency);
        }
        if self.request_timeout.is_zero() {
            return Err(FederationConfigError::ZeroRequestTimeout);
        }
        if self.connect_timeout.is_zero() {
            return Err(FederationConfigError::ZeroConnectTimeout);
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
    use crate::config::{
        DEFAULT_MAX_QUERY_ENTITIES,
        DEFAULT_MAX_QUERY_INSTANCES,
        FederationConfig,
        FederationConfigError,
        SplitEntities,
        SplitEntitiesDefault,
    };
    use andromeda_configuration_scalars::human_duration::HumanDuration;

    fn config() -> FederationConfig {
        FederationConfig {
            forward_concurrency: 16,
            connect_timeout: HumanDuration::from_millis(3000),
            request_timeout: HumanDuration::from_millis(5000),
            cache_max_entries: 1024,
            split_entities_default: SplitEntitiesDefault::Complete,
            max_query_entities: DEFAULT_MAX_QUERY_ENTITIES,
            max_query_instances: DEFAULT_MAX_QUERY_INSTANCES,
        }
    }

    /// The federated query budget defaults to ten thousand entities.
    #[test]
    fn the_query_budget_defaults_to_ten_thousand_entities() {
        pretty_assertions::assert_eq!(FederationConfig::default().max_query_entities.get(), 10_000);
    }

    /// The federated temporal instance budget defaults to one hundred thousand
    /// instances.
    #[test]
    fn the_instance_budget_defaults_to_one_hundred_thousand_instances() {
        pretty_assertions::assert_eq!(FederationConfig::default().max_query_instances.get(), 100_000);
    }

    #[test]
    fn an_explicit_split_entities_wins_and_an_absent_one_takes_the_default() {
        pretty_assertions::assert_eq!(SplitEntitiesDefault::Complete.resolve(Some(true)), SplitEntities::Split);
        pretty_assertions::assert_eq!(SplitEntitiesDefault::Split.resolve(Some(false)), SplitEntities::Complete);
        pretty_assertions::assert_eq!(SplitEntitiesDefault::Split.resolve(None), SplitEntities::Split);
        pretty_assertions::assert_eq!(SplitEntitiesDefault::Complete.resolve(None), SplitEntities::Complete);
        assert!(SplitEntities::Split.is_split());
        assert!(!SplitEntities::Complete.is_split());
    }

    #[test]
    fn zero_forward_concurrency_is_rejected() {
        let config = FederationConfig {
            forward_concurrency: 0,
            ..config()
        };
        assert_eq!(config.validate(), Err(FederationConfigError::ZeroForwardConcurrency));
    }

    #[test]
    fn zero_timeouts_are_rejected() {
        let request = FederationConfig {
            request_timeout: HumanDuration::from_millis(0),
            ..config()
        };
        assert_eq!(request.validate(), Err(FederationConfigError::ZeroRequestTimeout));
        let connect = FederationConfig {
            connect_timeout: HumanDuration::from_millis(0),
            ..config()
        };
        assert_eq!(connect.validate(), Err(FederationConfigError::ZeroConnectTimeout));
        assert!(config().validate().is_ok());
    }
}
