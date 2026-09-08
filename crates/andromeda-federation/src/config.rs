//! Federation operation configuration.

use confique::Config;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

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
    #[error("federation.request_timeout_ms must be at least 1")]
    ZeroRequestTimeout,
    /// Connecting to a source needs a positive deadline.
    #[error("federation.connect_timeout_ms must be at least 1")]
    ZeroConnectTimeout,
}

/// Distributed Context Source forwarding limits.
#[derive(Config, Debug, Clone, Serialize)]
pub struct FederationConfig {
    /// Maximum parallel Context Source forwarding requests.
    #[config(env = "ANDROMEDA_FEDERATION_FORWARD_CONCURRENCY", default = 16)]
    pub forward_concurrency: usize,
    /// Milliseconds allowed to establish a connection to a Context Source.
    #[config(env = "ANDROMEDA_FEDERATION_CONNECT_TIMEOUT_MS", default = 3000)]
    pub connect_timeout_ms: u64,
    /// Milliseconds a forwarded request may take before it is assumed to have
    /// failed, unless the registration's `management.timeout` says otherwise
    /// (clause 5.2.34).
    #[config(env = "ANDROMEDA_FEDERATION_REQUEST_TIMEOUT_MS", default = 5000)]
    pub request_timeout_ms: u64,
    /// Most cached forwarded GET responses held per tenant (clause 6.3.17).
    #[config(env = "ANDROMEDA_FEDERATION_CACHE_MAX_ENTRIES", default = 1024)]
    pub cache_max_entries: usize,
    /// What `splitEntities` means when a request or subscription leaves it
    /// unset (clauses 4.3.6.7, 5.2.12, 5.8.6, 5.14.4.4).
    #[config(env = "ANDROMEDA_FEDERATION_SPLIT_ENTITIES_DEFAULT", default = "complete")]
    pub split_entities_default: SplitEntitiesDefault,
}

impl FederationConfig {
    /// Validate Context Source forwarding limits.
    pub const fn validate(&self) -> Result<(), FederationConfigError> {
        if self.forward_concurrency == 0 {
            return Err(FederationConfigError::ZeroForwardConcurrency);
        }
        if self.request_timeout_ms == 0 {
            return Err(FederationConfigError::ZeroRequestTimeout);
        }
        if self.connect_timeout_ms == 0 {
            return Err(FederationConfigError::ZeroConnectTimeout);
        }
        Ok(())
    }

    /// The connection deadline.
    #[must_use]
    pub const fn connect_timeout(&self) -> Duration {
        Duration::from_millis(self.connect_timeout_ms)
    }

    /// The default per-request deadline.
    #[must_use]
    pub const fn request_timeout(&self) -> Duration {
        Duration::from_millis(self.request_timeout_ms)
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{FederationConfig, FederationConfigError, SplitEntities, SplitEntitiesDefault};

    fn config() -> FederationConfig {
        FederationConfig {
            forward_concurrency: 16,
            connect_timeout_ms: 3000,
            request_timeout_ms: 5000,
            cache_max_entries: 1024,
            split_entities_default: SplitEntitiesDefault::Complete,
        }
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
            request_timeout_ms: 0,
            ..config()
        };
        assert_eq!(request.validate(), Err(FederationConfigError::ZeroRequestTimeout));
        let connect = FederationConfig {
            connect_timeout_ms: 0,
            ..config()
        };
        assert_eq!(connect.validate(), Err(FederationConfigError::ZeroConnectTimeout));
        assert!(config().validate().is_ok());
    }
}
